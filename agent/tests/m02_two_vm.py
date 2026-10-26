#!/usr/bin/env python3
"""Run the M02 contract against installed binaries on two existing SSH hosts."""

from __future__ import annotations

import argparse
from contextlib import contextmanager
from dataclasses import dataclass, field
import hashlib
import ipaddress
import json
import os
from pathlib import Path
import re
import shlex
import ssl
import subprocess
import sys
import time
import uuid


BASE = "/home/botstral/m02-acceptance"
PASSWORD = "synthetic M02 owner password"
VAULT = "synthetic M02 vault passphrase"
TIMEOUT = 10


class CheckFailure(Exception):
    pass


def require(condition, details):
    if not condition:
        raise CheckFailure(details)


def identifier():
    return str(uuid.uuid4())


def private_json(path, value):
    with path.open("x", encoding="utf-8") as stream:
        os.chmod(path, 0o600)
        json.dump(value, stream, indent=2)
        stream.write("\n")


@dataclass
class Remote:
    alias: str
    root: str
    deadline: float | None = None

    def ssh(self, arguments, payload):
        timeout = 20 if payload.get("op") == "exit" else TIMEOUT
        if self.deadline is not None:
            timeout = min(timeout, self.deadline - time.monotonic())
        require(timeout > 0, "poll deadline exceeded")
        try:
            result = subprocess.run(
                ["ssh", "-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=yes",
                 "-o", "ConnectTimeout=5", "-o", "ConnectionAttempts=1", "--",
                 self.alias, shlex.join(arguments)],
                input=json.dumps(payload), text=True, capture_output=True, timeout=timeout,
            )
        except subprocess.TimeoutExpired:
            raise CheckFailure("SSH operation exceeded deadline") from None
        try:
            response = json.loads(result.stdout)
        except (ValueError, TypeError):
            raise CheckFailure("SSH helper returned no valid JSON") from None
        require(result.returncode == 0 and response.get("ok"),
                "SSH helper failed for " + payload.get("op", "bootstrap")
                + " (" + response.get("error", "remote exit") + ")")
        return response["value"]

    def install(self):
        source = Path(__file__).with_name("m02_remote.py").read_text()
        bootstrap = (
            "import json,os,pathlib,sys; os.umask(0o077); p=json.load(sys.stdin); "
            "r=pathlib.Path(p['root']); r.mkdir(mode=0o700,parents=True,exist_ok=False); "
            "f=r/'m02_remote.py'; f.write_text(p['source']); f.chmod(0o600); "
            "print(json.dumps({'ok':True,'value':True}))"
        )
        self.ssh(["python3", "-c", bootstrap], {"root": self.root, "source": source})

    def call(self, op, **values):
        if self.deadline is not None:
            values["timeout"] = max(0.01, min(TIMEOUT, self.deadline - time.monotonic()))
        return self.ssh(["python3", self.root + "/m02_remote.py"], {"op": op, **values})


@dataclass
class Process:
    remote: Remote
    unit: str
    log: str
    terminate: bool
    stopped: bool = False
    evidence: dict = field(default_factory=dict)


@dataclass
class Fixture:
    phase: str
    manager: Remote
    worker: Remote
    address: str
    output: Path
    secrets: list[str]
    processes: list[Process] = field(default_factory=list)
    owner_token: str = ""
    worker_id: str = ""
    generation: int = 0
    listen: str = ""
    setup_address: str = ""
    daemon: Process | None = None

    @contextmanager
    def check(self, name):
        details = {}
        try:
            yield details
        except Exception as error:
            details = str(error) if isinstance(error, CheckFailure) else type(error).__name__
            record(self.phase + "." + name, "failed", details)
            raise
        else:
            record(self.phase + "." + name, "passed", details or "Contract assertions satisfied.")

    def poll(self, read, predicate, label):
        deadline = time.monotonic() + TIMEOUT
        previous = (self.manager.deadline, self.worker.deadline)
        self.manager.deadline = self.worker.deadline = deadline
        try:
            while True:
                value = read()
                require(time.monotonic() <= deadline, label + " deadline exceeded")
                if predicate(value):
                    return value
                remaining = deadline - time.monotonic()
                require(remaining > 0, label + " deadline exceeded")
                time.sleep(min(0.1, remaining))
        finally:
            self.manager.deadline, self.worker.deadline = previous

    def launch(self, remote, name, args, terminate=True):
        unit = "m02-" + self.output.name + "-" + self.phase + "-" + name + ".service"
        process = Process(remote, unit, remote.root + "/" + name + ".log", terminate)
        self.processes.append(process)
        state = remote.call("start", unit=unit, log=process.log, args=args)
        require(state["ActiveState"] in ("active", "activating"), "binary unit failed to start")
        return process

    def stop(self, process):
        require(not process.stopped, "unit was already stopped")
        process.evidence = process.remote.call("exit", unit=process.unit, terminate=process.terminate)
        process.stopped = True
        private_json(self.output / (process.unit + ".shutdown.json"), process.evidence)
        require(process.evidence["clean"], "unit did not exit cleanly; inspect shutdown evidence")

    def owner(self, method, path, body=None, status=200, token=None, setup=False):
        response = self.manager.call(
            "http", address=self.setup_address if setup else self.listen, method=method, path=path,
            body={} if body is None else body,
            token=self.owner_token if token is None else token,
        )
        require(response["status"] == status,
                "owner " + method + " " + path + " expected " + str(status)
                + ", received " + str(response["status"]))
        return response["body"]

    def tls_options(self, identity="worker", ca="ca.pem"):
        return {"address": self.address, "ca": self.worker.root + "/certs/" + ca,
                "cert": self.worker.root + "/certs/" + identity + ".pem",
                "key": self.worker.root + "/certs/" + identity + ".key"}

    def worker_http(self, method, path, body=None, identity="worker"):
        return self.worker.call("http", **self.tls_options(identity), tls=True,
                                method=method, path=path, body={} if body is None else body)

    def worker_error(self, body, expected, identity="worker"):
        response = self.worker_http("POST", "/worker/v1/enroll", body, identity)
        require(response["status"] >= 400 and response["body"] == {"error": expected},
                "enrollment did not explicitly reject with " + expected)

    def ws_error(self, message, expected, identity="worker"):
        response = self.worker.call("ws", **self.tls_options(identity), message=message)
        require(response == {"type": "error", "code": expected},
                "WSS probe did not explicitly return " + expected)

    def invitation(self, certificates, identity="worker", seconds=600):
        der = ssl.PEM_cert_to_DER_cert(certificates[identity + ".pem"])
        fingerprint = hashlib.sha256(der).hexdigest()
        grant = self.owner("POST", "/v1/worker-invitations",
                           {"fingerprint": fingerprint, "lifetime_seconds": seconds}, status=201)
        self.secrets.append(grant["invitation"])
        return grant

    def workers(self):
        return self.owner("GET", "/v1/workers")

    def phase_wait(self, phase, minimum):
        def matching():
            return next((worker for worker in self.workers()
                         if worker["id"] == self.worker_id and worker["phase"] == phase
                         and worker["generation"] >= minimum), None)
        return self.poll(matching, lambda value: value is not None, "worker " + phase)

    def enroll(self, grant, certificates):
        state = self.worker.root + "/agent"
        invitation = self.worker.root + "/invitation.json"
        self.worker.call("write", files={invitation: json.dumps(grant)})
        args = [BASE + "/bin/production/botstral-agent", "enroll", "--state-dir", state,
                "--manager", "wss://" + self.address,
                "--manager-ca", self.worker.root + "/certs/ca.pem",
                "--client-cert", self.worker.root + "/certs/worker.pem",
                "--client-key", self.worker.root + "/certs/worker.key",
                "--invitation-file", invitation]
        process = self.launch(self.worker, "enroll", args, terminate=False)
        self.stop(process)
        fingerprint = hashlib.sha256(ssl.PEM_cert_to_DER_cert(
            certificates["worker.pem"])).hexdigest()
        workers = self.workers()
        matches = [worker for worker in workers if worker["fingerprint"] == fingerprint]
        require(len(matches) == 1, "actual CLI enrollment must create one worker")
        self.worker_id = matches[0]["id"]
        require(matches[0]["generation"] == 0, "enrollment must leave generation zero")

    def start_daemon(self, name):
        binary = "production/botstral-agent" if self.phase == "production" else "test/botstral-test-agent"
        self.daemon = self.launch(self.worker, name,
                                  [BASE + "/bin/" + binary, "run", "--state-dir", self.worker.root + "/agent"])
        ready = self.phase_wait("ready", self.generation + 1)
        self.generation = ready["generation"]
        return ready

    def session(self, name):
        bot = identifier()
        self.owner("PUT", "/v1/bots/" + bot, {"name": "Synthetic " + name})
        return self.owner("POST", "/v1/computer-sessions", {"bot_id": bot, "worker_id": self.worker_id})

    def confirmed(self, session):
        return self.poll(lambda: self.owner("GET", "/v1/computer-sessions/" + session["id"]),
                         lambda value: value["status"] == "confirmed", "confirmed control")

    def takeover(self, session, owner):
        response = self.owner("PUT", "/v1/computer-sessions/" + session["id"] + "/owner",
                              {"owner": owner, "expected_revision": session["revision"]})
        require(response["status"] in ("pending", "confirmed"), "takeover has unexpected status")
        confirmed = self.confirmed(session)
        require(confirmed["revision"] > session["revision"] and confirmed["owner"] == owner,
                "confirmed takeover must advance revision and change owner")
        return confirmed

    def fence(self, session):
        return {"generation": self.generation, "session_id": session["id"],
                "revision": session["revision"], "owner": session["owner"]}

    def command(self, session, action, task=None):
        command = {"id": identifier(), "task_id": task or identifier(),
                   "fence": self.fence(session), "action": action}
        self.owner("POST", "/v1/worker-commands", command)
        return command

    def receipt(self, command, predicate):
        return self.poll(lambda: self.owner("GET", "/v1/workers/" + self.worker_id
                                           + "/receipts/" + command["id"]),
                         predicate, "terminal or progress receipt")

    def input(self, fence, expected=200):
        command = {"id": identifier(), "fence": fence, "value": 1}
        response = self.owner("POST", "/v1/worker-input", command, status=expected)
        return command, response

    def finish(self):
        for process in reversed(self.processes):
            if not process.stopped:
                try:
                    with self.check("shutdown." + process.unit):
                        self.stop(process)
                except Exception:
                    pass
        for remote, name in ((self.manager, "manager"), (self.worker, "worker")):
            try:
                with self.check("permissions." + name):
                    require(not remote.call("permissions", root=remote.root),
                            "fixture contains nonprivate files or directories")
            except Exception:
                pass
        for remote, name, directory in ((self.manager, "manager", "state"), (self.worker, "worker", "agent")):
            try:
                with self.check("state_redaction." + name) as details:
                    scanned = remote.call("scan_state", root=remote.root + "/" + directory, secrets=self.secrets)
                    require(scanned["files"] and not scanned["leaks"], "state contains sensitive fixture material")
                    details.update(files=scanned["files"])
            except Exception:
                pass
        for process in self.processes:
            try:
                with self.check("logs." + process.unit):
                    content = process.remote.call("read", path=process.log)
                    leaked = any(secret and secret in content for secret in self.secrets)
                    leaked = leaked or "PRIVATE KEY" in content
                    for secret in sorted(set(self.secrets), key=len, reverse=True):
                        if secret:
                            content = content.replace(secret, "[REDACTED]")
                    content = re.sub(r"-----BEGIN [^-]*PRIVATE KEY-----.*?-----END [^-]*PRIVATE KEY-----",
                                     "[REDACTED PRIVATE KEY]", content, flags=re.S)
                    content = re.sub(r"Bearer\s+[^\s\"']+", "Bearer [REDACTED]", content)
                    with (self.output / (process.unit + ".redacted.log")).open("x") as stream:
                        os.chmod(stream.name, 0o600)
                        stream.write(content)
                    require(not leaked, "service log contained sensitive fixture material; copy redacted")
            except Exception:
                pass


RESULTS = []


def record(name, status, details):
    result = {"name": name, "status": status, "details": details}
    RESULTS.append(result)
    print(json.dumps(result), flush=True)


def hello(**changes):
    value = {"type": "hello", "versions": [1], "channel": "stable",
             "capabilities": ["logical_sessions", "input_fencing"],
             "required": ["logical_sessions", "input_fencing"]}
    value.update(changes)
    return value


def setup(f, certificates):
    with f.check("private_fixture_and_manager"):
        for remote, names in (
            (f.manager, ("ca.pem", "manager.pem", "manager.key")),
            (f.worker, tuple(name for name in certificates if not name.startswith("manager"))),
        ):
            remote.install()
            remote.call("write", files={remote.root + "/certs/" + name: certificates[name] for name in names})
        with f.check("separate_hosts_and_binary_hashes") as details:
            manager = f.manager.call("identity", binaries=[BASE + "/bin/" + f.phase + "/botstral-manager"])
            worker_binary = "production/botstral-agent" if f.phase == "production" else "test/botstral-test-agent"
            worker = f.worker.call("identity", binaries=[BASE + "/bin/" + worker_binary])
            require(manager["machine_id_sha256"] != worker["machine_id_sha256"],
                    "manager and worker resolve to the same machine identity")
            address = f.address.rsplit(":", 1)[0]
            require(address in manager["addresses"] and address not in worker["addresses"],
                    "worker listener address does not belong to the separate manager host")
            details.update(manager=manager, worker=worker)
        state = f.manager.root + "/state"
        certs = f.manager.root + "/certs/"
        f.launch(f.manager, "manager", [BASE + "/bin/" + f.phase + "/botstral-manager",
                 "--state-dir", state, "--listen", "127.0.0.1:0", "--setup-listen", "127.0.0.1:0",
                 "--worker-listen", f.address, "--worker-tls-cert", certs + "manager.pem",
                 "--worker-tls-key", certs + "manager.key", "--worker-client-ca", certs + "ca.pem"])
        addresses = json.loads(f.poll(
            lambda: f.manager.call("read", path=state + "/listeners.json", optional=True),
            lambda value: value is not None, "manager listeners"))
        f.listen, f.setup_address = addresses["listen"], addresses["setup"]
        require(f.listen.startswith("127.0.0.1:") and f.setup_address.startswith("127.0.0.1:")
                and addresses["worker"] == f.address, "listener boundaries differ from contract")
        bootstrap = f.manager.call("read", path=state + "/bootstrap.token").strip()
        f.secrets.append(bootstrap)
        f.owner("POST", "/v1/owner", {"name": "Synthetic M02 owner", "password": PASSWORD,
                "vault_passphrase": VAULT, "acknowledge_no_escrow": True},
                status=201, token=bootstrap, setup=True)
        session = f.owner("POST", "/v1/sessions", {"password": PASSWORD}, status=201, token="")
        f.owner_token = session["token"]
        f.secrets.append(f.owner_token)


def production(f, certificates):
    with f.check("enrollment_identity_mismatch"):
        grant = f.invitation(certificates)
        f.worker_error({"invitation": grant["invitation"]}, "identity_mismatch", "other")
    with f.check("invitation_expiry"):
        expired = f.invitation(certificates, "other", seconds=1)
        deadline = time.monotonic() + 2
        while time.monotonic() < deadline:
            time.sleep(min(0.25, max(0, deadline - time.monotonic())))
        f.worker_error({"invitation": expired["invitation"]}, "invitation_expired", "other")
    with f.check("actual_cli_enrollment_and_replay"):
        f.enroll(grant, certificates)
        f.worker_error({"invitation": grant["invitation"]}, "invitation_used")
    with f.check("tls_certificate_boundaries"):
        require(f.worker_http("GET", "/v1/owner")["status"] == 404, "valid mTLS baseline failed")
        probes = [("wrong_ca", {**f.tls_options(), "ca": f.worker.root + "/certs/wrong-ca.pem"}),
                  ("wrong_hostname", {**f.tls_options(), "hostname": "wrong.example"}),
                  ("wrong_client", f.tls_options("wrong-client"))]
        if "expired.pem" in certificates:
            probes.append(("expired_client", f.tls_options("expired")))
        for name, options in probes:
            with f.check("tls." + name) as details:
                evidence = f.worker.call("tls_rejection", **options)
                require(evidence["rejected"], "certificate probe was accepted")
                details.update(evidence)
                if name in ("wrong_ca", "wrong_hostname"):
                    require(evidence["reason"] == "CERTIFICATE_VERIFY_FAILED", "expected verification failure")
                    if name == "wrong_hostname":
                        require(evidence["verify_code"] == 62, "expected explicit hostname mismatch")
                else:
                    require(evidence["reason"] in ("TLSV1_ALERT_UNKNOWN_CA", "SSLV3_ALERT_BAD_CERTIFICATE",
                            "SSLV3_ALERT_CERTIFICATE_EXPIRED", "TLSV1_ALERT_CERTIFICATE_EXPIRED",
                            "SSLV3_ALERT_CERTIFICATE_UNKNOWN"), "expected client certificate alert")
        if "expired.pem" not in certificates:
            record("production.tls.expired_client", "not_run", "Parent supplied no expired certificate pair.")
    with f.check("owner_routes_absent_from_worker_tls"):
        paths = ["/v1/owner", "/v1/sessions", "/v1/bots", "/v1/status", "/v1/vault/unlock",
                 "/v1/vault/lock", "/v1/provider-keys/mistral", "/v1/worker-invitations",
                 "/v1/devices", "/v1/approvals", "/v1/security"]
        for path in paths:
            responses = f.worker.call("http_batch", requests=[
                {**f.tls_options(), "tls": True, "method": method, "path": path}
                for method in ("GET", "POST", "PUT", "DELETE")])
            require(all(response["status"] == 404 for response in responses),
                    "owner route exposed over TLS at " + path)
    with f.check("invitations_are_not_owner_auth"):
        f.worker_error({"invitation": f.owner_token}, "invalid_invitation")
        f.owner("GET", "/v1/owner", token=grant["invitation"], status=401)
    with f.check("forbidden_variants_explicitly_invalid"):
        for method in ("approve", "enroll_owner_device", "get_credential", "change_security_policy"):
            f.ws_error({"type": method}, "invalid_message")
    with f.check("negotiation_rejections_leave_generation_zero"):
        f.ws_error(hello(), "unauthorized", "other")
        for message, code in ((hello(versions=[99]), "incompatible_version"),
                              (hello(channel="experimental"), "incompatible_channel"),
                              (hello(capabilities=[]), "missing_capability"),
                              (hello(required=["test_input"]), "missing_capability"),
                              ({"type": "heartbeat", "generation": 1}, "wrong_phase")):
            f.ws_error(message, code)
        worker = next(worker for worker in f.workers() if worker["id"] == f.worker_id)
        require(worker["generation"] == 0, "rejected negotiation advanced generation")
    with f.check("daemon_ready_stop_reconnect") as details:
        first = f.start_daemon("agent")
        f.stop(f.daemon)
        f.phase_wait("disconnected", f.generation)
        ready = f.start_daemon("agent-reconnect")
        require(ready["id"] == first["id"] and ready["generation"] > first["generation"],
                "reconnect did not preserve identity and advance generation")
        details.update(worker_id=f.worker_id, first_generation=first["generation"], generation=f.generation)
    with f.check("production_execution_refused_by_receipts"):
        session = f.confirmed(f.session("Production refusal"))
        for action in ({"type": "start", "work": {"type": "unavailable"}},
                       {"type": "execute_approved", "approval_id": identifier(),
                        "operation_id": identifier(),
                        "argument_digest": hashlib.sha256(b"synthetic arguments").hexdigest(),
                        "expires_at": 9999999999}):
            command = f.command(session, action)
            f.receipt(command, lambda value: value.get("acknowledgment") == {
                "type": "rejected", "code": "unsupported_execution"})
        taken = f.takeover(session, "owner")
        command, _ = f.input(f.fence(taken))
        f.receipt(command, lambda value: value.get("acknowledgment") == {
            "type": "rejected", "code": "unsupported_execution"})
    with f.check("revocation_closes_active_and_rejects_future"):
        previous_disconnects = f.worker.call("read", path=f.daemon.log).count("worker disconnected; retrying")
        f.owner("DELETE", "/v1/workers/" + f.worker_id, status=204)
        f.phase_wait("disconnected", f.generation)
        f.poll(lambda: f.worker.call("read", path=f.daemon.log),
               lambda log: log.count("worker disconnected; retrying") > previous_disconnects, "live socket revocation")
        f.ws_error(hello(), "unauthorized")


def applied(receipt):
    return (receipt.get("last_event") or {}).get("applied", 0)


def event_is(**fields):
    return lambda receipt: all((receipt.get("last_event") or {}).get(key) == value
                               for key, value in fields.items())


def test_sessions(f, certificates):
    with f.check("test_worker_enrollment_and_two_sessions"):
        grant = f.invitation(certificates)
        response = f.worker_http("POST", "/worker/v1/enroll", {"invitation": grant["invitation"]})
        require(response["status"] == 201, "test worker enrollment failed")
        f.worker_id = response["body"]["worker_id"]
        config = {"manager": "wss://" + f.address, "manager_ca": f.worker.root + "/certs/ca.pem",
                  "client_cert": f.worker.root + "/certs/worker.pem",
                  "client_key": f.worker.root + "/certs/worker.key"}
        f.worker.call("write", files={f.worker.root + "/agent/config.json": json.dumps(config)})
        a, b = f.session("A"), f.session("B")
        require(a["id"] != b["id"] and a["bot_id"] != b["bot_id"], "sessions and bots must differ")
        f.start_daemon("test-agent")
        a, b = f.confirmed(a), f.confirmed(b)
    with f.check("both_sessions_apply_input") as details:
        action = {"type": "start", "work": {"type": "test_input", "steps": 1000}}
        start_a, start_b = f.command(a, action), f.command(b, action)
        progress_a = f.receipt(start_a, lambda value: applied(value) >= 3)
        progress_b = f.receipt(start_b, lambda value: applied(value) >= 3)
        details.update(applied_a=applied(progress_a), applied_b=applied(progress_b))
    with f.check("confirmed_takeover_pauses_only_A") as details:
        owner_a = f.takeover(a, "owner")
        require(f.confirmed(b) == b, "B session changed during A takeover")
        status = f.command(owner_a, {"type": "status"}, start_a["task_id"])
        paused = f.receipt(status, event_is(status="suspended"))
        before_b = applied(f.receipt(start_b, lambda value: applied(value) >= 3))
        advanced_b = f.receipt(start_b, lambda value: applied(value) > before_b + 2)
        status = f.command(owner_a, {"type": "status"}, start_a["task_id"])
        still_paused = f.receipt(status, event_is(status="suspended"))
        require(applied(paused) >= 3 and applied(paused) == applied(still_paused),
                "A applied count did not remain fixed after confirmed takeover")
        require(f.confirmed(b) == b, "B session changed while A was paused")
        details.update(paused_a_before=applied(paused), paused_a_after=applied(still_paused),
                       applied_b_before=before_b, applied_b_after=applied(advanced_b), session_b=b)
    with f.check("stale_input_rejected_and_current_input_applied"):
        stale = {**f.fence(a), "owner": "owner"}
        _, response = f.input(stale, expected=409)
        require(response == {"error": "stale_revision"}, "stale owner input was not fenced")
        command, _ = f.input(f.fence(owner_a))
        f.receipt(command, lambda value: value.get("acknowledgment") == {"type": "applied"})
        bot_a = f.takeover(owner_a, "bot")
        _, response = f.input(f.fence(owner_a), expected=409)
        require(response == {"error": "stale_revision"}, "returning control did not invalidate owner fence")
    with f.check("cancellation_and_result_counts") as details:
        cancel = f.command(b, {"type": "cancel"}, start_b["task_id"])
        cancelled = f.receipt(cancel, event_is(type="cancelled"))
        require(type(cancelled["last_event"].get("applied")) is int
                and 3 <= applied(cancelled) < 1000, "cancellation must report applied count below 1000")
        result = f.command(b, {"type": "result"}, start_b["task_id"])
        result = f.receipt(result, event_is(status="cancelled"))
        require(type(result["last_event"].get("applied")) is int
                and applied(result) == applied(cancelled), "cancel result count differs from terminal event")
        details.update(cancelled_applied=applied(cancelled), result_applied=applied(result))
    with f.check("small_task_completes_exactly_three") as details:
        small = f.command(bot_a, {"type": "start", "work": {"type": "test_input", "steps": 3}})
        completed = {"type": "completed", "applied": 3}
        f.receipt(small, lambda value: value.get("last_event") == completed)
        result = f.command(bot_a, {"type": "result"}, small["task_id"])
        f.receipt(result, lambda value: value.get("last_event") == completed)
        details.update(terminal_event=completed, result_event=completed)
    with f.check("command_idempotency_and_conflict"):
        f.owner("POST", "/v1/worker-commands", small)
        response = f.owner("POST", "/v1/worker-commands", {**small, "action": {"type": "cancel"}}, status=409)
        require(response == {"error": "conflict"}, "changed duplicate command did not conflict")
    with f.check("reconnect_advances_generation_and_revisions") as details:
        previous_generation = f.generation
        f.stop(f.daemon)
        f.phase_wait("disconnected", previous_generation)
        ready = f.start_daemon("test-agent-reconnect")
        require(ready["id"] == f.worker_id and f.generation > previous_generation,
                "reconnect did not preserve worker identity and advance generation")
        new_a, new_b = f.confirmed(a), f.confirmed(b)
        require(new_a["revision"] > bot_a["revision"] and new_b["revision"] > b["revision"],
                "reconnect did not advance both session revisions")
        details.update(previous_generation=previous_generation, generation=f.generation,
                       old_revisions=[bot_a["revision"], b["revision"]],
                       revisions=[new_a["revision"], new_b["revision"]])
        response = f.owner("POST", "/v1/worker-commands", small, status=409)
        require(response == {"error": "stale_generation"}, "old command did not fail its generation fence")
        missing = f.command(new_a, {"type": "result"}, small["task_id"])
        f.receipt(missing, lambda value: value.get("acknowledgment") == {
            "type": "rejected", "code": "unknown_task"})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manager-ssh", default="botstral-manager")
    parser.add_argument("--worker-ssh", default="botstral-computer")
    parser.add_argument("--manager-address", default="192.168.1.40")
    parser.add_argument("--cert-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    os.umask(0o077)
    run_id = identifier()
    output = args.output_dir / run_id
    output.mkdir(mode=0o700, parents=True)
    roots = []
    try:
        require(args.manager_ssh != args.worker_ssh, "manager and worker SSH aliases must differ")
        require(all(alias and not alias.startswith("-") for alias in (args.manager_ssh, args.worker_ssh)),
                "SSH aliases cannot be options")
        socket_address = str(ipaddress.IPv4Address(args.manager_address)) + ":43122"
        names = ["ca.pem", "manager.pem", "manager.key", "worker.pem", "worker.key", "other.pem",
                 "other.key", "wrong-ca.pem", "wrong-client.pem", "wrong-client.key"]
        expired = [(args.cert_dir / name).is_file() for name in ("expired.pem", "expired.key")]
        require(expired[0] == expired[1], "expired certificate and key must be supplied together")
        if all(expired):
            names += ["expired.pem", "expired.key"]
        certificates = {name: (args.cert_dir / name).read_text() for name in names}
        for phase, exercise in (("production", production), ("test", test_sessions)):
            root = BASE + "/runs/" + run_id + "/" + phase
            fixture = Fixture(phase, Remote(args.manager_ssh, root), Remote(args.worker_ssh, root),
                              socket_address, output, [PASSWORD, VAULT, *certificates.values()])
            roots.append({"phase": phase, "manager_ssh": args.manager_ssh,
                          "worker_ssh": args.worker_ssh, "root": root})
            try:
                setup(fixture, certificates)
                exercise(fixture, certificates)
            except Exception:
                record(phase + ".remaining_contract", "not_run",
                       "Checks after the failed prerequisite were not run.")
            finally:
                fixture.finish()
        for name in ("socket_permit_limit", "silent_and_malformed_frames", "concurrent_invitation_redemption",
                     "lost_enrollment_response_recovery", "local_stress_and_additional_fencing"):
            record("remaining." + name, "not_run", "Outside this two-VM iteration; requires separate coverage.")
    except (Exception, KeyboardInterrupt) as error:
        record("runner", "failed", str(error) if isinstance(error, CheckFailure) else type(error).__name__)
    summary = {"run_id": run_id, "status": "failed" if any(result["status"] == "failed" for result in RESULTS)
               else "passed", "passed": sum(result["status"] == "passed" for result in RESULTS),
               "failed": sum(result["status"] == "failed" for result in RESULTS),
               "not_run": sum(result["status"] == "not_run" for result in RESULTS),
               "output_dir": str(output.resolve()), "retained_roots": roots}
    private_json(output / "results.json", {"checks": RESULTS, "summary": summary})
    print(json.dumps({"summary": summary}), flush=True)
    return 1 if summary["failed"] else 0


if __name__ == "__main__":
    sys.exit(main())
