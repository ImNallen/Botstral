#!/usr/bin/env python3
"""Verify a real, authorized Linux guest reboot with the production manager."""
import argparse
from datetime import datetime, timezone
import hashlib
from http.client import HTTPSConnection
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import re
import secrets
import shlex
import shutil
import sqlite3
import ssl
import struct
import subprocess
import threading
import time
import urllib.parse
import uuid

import m05_acceptance as support


def require(value, message):
    if not value:
        raise RuntimeError(message)


def uid():
    return str(uuid.uuid4())


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(argv, timeout=40):
    result = subprocess.run(argv, capture_output=True, text=True, timeout=timeout)
    require(result.returncode == 0, "command failed: " + argv[0])
    return result.stdout.strip()


def save(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def poll(read, predicate, label, seconds=30):
    until = time.monotonic() + seconds
    while time.monotonic() < until:
        value = read()
        if predicate(value):
            return value
        time.sleep(0.2)
    raise RuntimeError("deadline: " + label)


class Owner(support.WebSocket):
    def protected(self, operation):
        request = uid()
        data = b"botstral-protected-owner-v1\0" + json.dumps({"id": request, "operation": operation}).encode()
        mask = os.urandom(4)
        require(len(data) < 16384, "protected frame bound")
        self.socket.sendall(b"\x82\xfe" + struct.pack("!H", len(data)) + mask
                            + bytes(v ^ mask[i % 4] for i, v in enumerate(data)))
        reply = self.receive()
        require(reply["type"] == "protected_reply" and reply["id"] == request,
                "protected operation refused")
        return reply["status"]


class Worker(support.WebSocket):
    def __init__(self, address, context):
        super().__init__(address, context, path="/worker/v1/connect")
        self.journal = uid()
        self.send_message({"type": "hello", "versions": [4], "channel": "stable",
                           "capabilities": ["logical_sessions", "input_fencing", "process_systemd_v1", "computer_v1"],
                           "required": ["logical_sessions", "input_fencing"]})
        welcome = self.until("welcome")
        self.generation = welcome["generation"]
        self.worker = welcome["worker_id"]
        self.send_message({"type": "snapshot_applied", "generation": self.generation})
        self.send_message({"type": "recovery_hello", "generation": self.generation,
                           "incarnation": self.journal, "high_water": 0})
        self.until("inventory_request")
        self.send_message({"type": "inventory", "generation": self.generation,
                           "incarnation": self.journal, "offset": 0, "total": 0, "entries": []})
        self.until("recovery_ready")

    def send_message(self, message):
        self.send({"version": 4, "request_id": uid(), "message": message})

    def until(self, kind, predicate=lambda message: True):
        for _ in range(128):
            message = self.receive()["message"]
            require(message["type"] not in ("error", "claim_denied"), "worker protocol refused")
            if message["type"] == "control":
                session = message["session"]
                fence = {"generation": self.generation, "session_id": session["id"],
                         "revision": session["revision"], "owner": session["owner"]}
                self.send_message({"type": "control_applied", "fence": fence})
            if message["type"] == kind and predicate(message):
                return message
        raise RuntimeError("worker message bound")


def inspect(root):
    ancestors = set()
    pid = os.getpid()
    while pid:
        ancestors.add(pid)
        pid = int(Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()[1])
    busy = []
    processes = []
    for candidate in Path("/proc").iterdir():
        if not candidate.name.isdecimal():
            continue
        try:
            if candidate.stat().st_uid != os.getuid():
                continue
            name = (candidate / "comm").read_text().strip()
            processes.append({"pid": int(candidate.name), "name": name})
            own = root.name in (candidate / "cgroup").read_text()
            if int(candidate.name) not in ancestors and name not in ("systemd", "(sd-pam)", "dbus-daemon") and not own:
                busy.append(int(candidate.name))
        except (FileNotFoundError, ProcessLookupError):
            pass
    services = run(["systemctl", "list-units", "--type=service", "--state=running", "--no-legend", "--no-pager"])
    standard = {"cron", "dbus", "ModemManager", "multipathd", "polkit", "qemu-guest-agent", "rsyslog", "ssh",
                "systemd-journald", "systemd-logind", "systemd-networkd", "systemd-resolved", "systemd-timesyncd",
                "systemd-udevd", "udisks2", "unattended-upgrades", "fwupd", "packagekit"}
    unexpected = []
    for line in services.splitlines():
        unit = line.split()[0]
        if unit == "botstral-" + root.name + ".service":
            continue
        if unit.removesuffix(".service") not in standard and not unit.startswith(("getty@", "serial-getty@", "user@")):
            unexpected.append(unit)
    package_locks = subprocess.run(["sudo", "-n", "fuser", "/var/lib/dpkg/lock", "/var/lib/dpkg/lock-frontend",
                                    "/var/lib/apt/lists/lock", "/var/cache/apt/archives/lock"], capture_output=True)
    require(package_locks.returncode in (0, 1), "could not inspect package locks")
    result = {"uid": os.getuid(), "machine": os.uname().machine, "hostname": os.uname().nodename,
              "boot_id": Path("/proc/sys/kernel/random/boot_id").read_text().strip(),
              "user_processes": processes, "system_services": services,
              "user_services": run(["systemctl", "--user", "list-units", "--type=service", "--state=running", "--no-pager"]),
              "busy_user_pids": busy, "unexpected_system_services": unexpected, "package_locks_held": package_locks.returncode == 0}
    save(root / "inspection.json", result)
    require(os.getuid() == 1000 and os.uname().machine == "x86_64", "requires the authorized Linux guest account")
    require(not busy and not unexpected and package_locks.returncode == 1, "guest has unrelated workloads; inspect inventory")
    return result


class Fixture:
    def __init__(self, root):
        self.root = root
        require(re.fullmatch(r"/home/botstral/p06-reboot-[0-9a-f]{12}", str(root)), "owned fixture path")
        require(root.stat().st_uid == os.getuid() and root.stat().st_mode & 0o777 == 0o700, "private owned root")
        self.unit = "botstral-" + root.name + ".service"
        self.private_path = root / "private.json"
        self.private = json.loads(self.private_path.read_text()) if self.private_path.exists() else {}
        self.listeners = None
        self.token = None

    def emit(self, check, **details):
        line = json.dumps({"check": check, **details})
        require(not any(value in line for key, value in self.private.items()
                        if key in ("password", "passphrase", "device", "username", "secret")), "private value in evidence")
        with (self.root / "events.jsonl").open("a") as output:
            output.write(line + "\n")

    def http(self, method, path, body=None, expected=200, token=None, setup=False):
        code, value = support.http(self.listeners["setup" if setup else "listen"], method, path,
                                   body, self.token if token is None else token)
        require(code == expected, f"HTTP {method} {path} returned {code}, expected {expected}")
        return value

    def identity(self):
        state = dict(line.split("=", 1) for line in run(["systemctl", "show", self.unit,
                     "--property=MainPID,ActiveState,SubState,UnitFileState,ExecMainStartTimestamp,InvocationID"]).splitlines())
        require(state["ActiveState"] == "active" and state["SubState"] == "running"
                and state["UnitFileState"] == "enabled" and int(state["MainPID"]) > 0, "boot-enabled service is not running")
        state["binary_sha256"] = digest(Path("/proc") / state["MainPID"] / "exe")
        state["boot_id"] = Path("/proc/sys/kernel/random/boot_id").read_text().strip()
        return state

    def connect(self, before):
        self.listeners = json.loads((self.root / "state/listeners.json").read_text())
        self.token = self.http("POST", "/v1/sessions", {"password": self.private["password"]}, expected=201, token="")["token"]
        context = ssl.create_default_context(cafile=str(self.root / "ca.pem"))
        owner = Owner(self.listeners["public"], context)
        hello = {"type": "enroll" if before else "authenticate", "version": 3,
                 "manager": self.private["manager"], "role": "owner", "credential": self.private["device"]}
        if before:
            hello["invitation"] = self.private["invitation"]
        owner.send(hello)
        require(owner.receive()["type"] == "welcome", "paired owner authentication")
        return owner, context

    def ciphertext(self):
        with sqlite3.connect(self.root / "state/manager.sqlite3") as db:
            require(db.execute("PRAGMA integrity_check").fetchone() == ("ok",), "SQLite integrity")
            rows = db.execute("SELECT ciphertext FROM credential_epochs ORDER BY item,epoch").fetchall()
        require(len(rows) == 1, "one encrypted reusable login")
        require(all(self.private[key].encode() not in rows[0][0] for key in ("username", "secret")), "login ciphertext contains plaintext")
        return hashlib.sha256(rows[0][0]).hexdigest()

    def prepare(self):
        self.emit("initial_inventory", inventory=inspect(self.root))
        certificates = self.root / "certificates"
        certificates.mkdir(mode=0o700)
        support.certificates(certificates)
        for path in certificates.iterdir():
            path.rename(self.root / path.name)
        certificates.rmdir()
        self.private = {key: secrets.token_urlsafe(32) for key in ("password", "passphrase", "device", "username", "secret")}
        save(self.private_path, self.private)
        command = [str(self.root / "botstral-manager"), "--state-dir", str(self.root / "state"),
                   "--listen", "127.0.0.1:0", "--setup-listen", "127.0.0.1:0", "--public-listen", "127.0.0.1:0",
                   "--public-tls-cert", str(self.root / "manager.pem"), "--public-tls-key", str(self.root / "manager.key"),
                   "--public-worker-ca", str(self.root / "ca.pem")]
        unit = ("[Unit]\nDescription=P06 owned reboot fixture\nAfter=network.target\n[Service]\nType=exec\n"
                "User=botstral\nGroup=botstral\nUMask=0077\nWorkingDirectory=" + str(self.root) + "\nExecStart="
                + shlex.join(command) + "\nStandardOutput=append:" + str(self.root / "manager.log")
                + "\nStandardError=append:" + str(self.root / "manager.log") + "\n[Install]\nWantedBy=multi-user.target\n")
        (self.root / self.unit).write_text(unit)
        (self.root / "manager.log").touch(mode=0o600)
        require(not Path("/etc/systemd/system", self.unit).exists(), "unit already exists")
        run(["sudo", "-n", "install", "-m", "644", str(self.root / self.unit), "/etc/systemd/system/" + self.unit])
        run(["sudo", "-n", "systemctl", "daemon-reload"])
        run(["sudo", "-n", "systemctl", "enable", "--now", self.unit])
        poll(lambda: (self.root / "state/listeners.json").exists(), bool, "manager listeners")
        self.listeners = json.loads((self.root / "state/listeners.json").read_text())
        bootstrap = (self.root / "state/bootstrap.token").read_text().strip()
        self.http("POST", "/v1/owner", {"name": "P06 reboot fixture owner", "password": self.private["password"],
                  "vault_passphrase": self.private["passphrase"], "acknowledge_no_escrow": True},
                  expected=201, token=bootstrap, setup=True)
        self.token = self.http("POST", "/v1/sessions", {"password": self.private["password"]}, expected=201, token="")["token"]
        fingerprint = hashlib.sha256(b"botstral-owner-device-v1\0" + self.private["device"].encode()).hexdigest()
        grant = self.http("POST", "/v1/device-invitations", {"request": {"fingerprint": fingerprint,
                          "label": "P06 reboot fixture"}, "lifetime_seconds": 600})
        self.private.update(manager=grant["manager"], invitation=grant["invitation"], bot=uid())
        save(self.private_path, self.private)
        self.http("PUT", "/v1/bots/" + self.private["bot"], {"name": "P06 reboot fixture bot"})
        self.http("PUT", "/v1/bots/" + self.private["bot"] + "/permission", {"expected_revision": 1, "allow": True})

    def exercise(self, before):
        owner, context = self.connect(before)
        site = None
        worker = None
        try:
            if before:
                owner.protected({"type": "unlock", "passphrase": self.private["passphrase"]})
                require(self.http("GET", "/v1/status")["vault"] == "unlocked", "initial vault unlock")
            else:
                require(self.http("GET", "/v1/status")["vault"] == "locked", "vault auto-unlocked after reboot")
                initial = json.loads((self.root / "before.json").read_text())
                current = self.identity()
                require(current["boot_id"] != initial["boot_id"], "boot ID did not change")
                require(current["InvocationID"] != initial["InvocationID"], "service invocation did not change")
                require(current["binary_sha256"] == initial["binary_sha256"], "manager binary changed")
                require(self.ciphertext() == initial["ciphertext_sha256"], "encrypted login changed")
                self.emit("boot_enabled_manager_started_locked", service=current, ciphertext_unchanged=True)

            fixture = self
            submissions = []
            class Site(BaseHTTPRequestHandler):
                def do_POST(self):
                    fields = urllib.parse.parse_qs(self.rfile.read(int(self.headers["content-length"])).decode())
                    valid = self.path == "/session" and fields == {"username": [fixture.private["username"]], "password": [fixture.private["secret"]]}
                    submissions.append(valid)
                    self.send_response(200 if valid else 401)
                    self.end_headers()

                def log_message(self, *_):
                    pass

            site = ThreadingHTTPServer(("127.0.0.1", self.private.get("site_port", 0)), Site)
            tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            tls.load_cert_chain(self.root / "manager.pem", self.root / "manager.key")
            site.socket = tls.wrap_socket(site.socket, server_side=True)
            threading.Thread(target=site.serve_forever, daemon=True).start()
            self.private["site_port"] = site.server_port
            origin = "https://127.0.0.1:" + str(site.server_port)
            if before:
                receipt = owner.protected({"type": "write", "target": {"type": "create", "metadata": {
                    "label": "P06 reboot reusable login", "bots": {"type": "selected", "bots": [self.private["bot"]]},
                    "allowed_origins": [origin], "password_policy": "every_fill"}},
                    "values": {"username": self.private["username"], "password": self.private["secret"], "totp_seed": ""}})
                require(receipt["type"] == "committed", "protected login write")
                self.private["item"] = receipt["item"]
                save(self.private_path, self.private)
            fingerprint = hashlib.sha256(ssl.PEM_cert_to_DER_cert((self.root / "worker.pem").read_text())).hexdigest()
            worker_context = ssl.create_default_context(cafile=str(self.root / "ca.pem"))
            worker_context.load_cert_chain(self.root / "worker.pem", self.root / "worker.key")
            if before:
                grant = self.http("POST", "/v1/worker-invitations", {"fingerprint": fingerprint, "lifetime_seconds": 600}, expected=201)
                code, _ = support.http(self.listeners["public"], "POST", "/worker/v1/enroll",
                                       {"invitation": grant["invitation"]}, context=worker_context)
                require(code == 201, "synthetic worker enrollment")
            worker = Worker(self.listeners["public"], worker_context)
            session = self.http("POST", "/v1/computer-sessions", {"bot_id": self.private["bot"], "worker_id": worker.worker})
            if session["status"] != "confirmed":
                worker.until("control", lambda m: m["session"]["id"] == session["id"])
            poll(lambda: self.http("GET", "/v1/computer-sessions/" + session["id"]), lambda s: s["status"] == "confirmed", "session confirmation")
            plan = {"version": 1, "origin": origin, "route": "/login", "frame": None, "form": "#login",
                    "username": "#username", "password": "#password", "totp": "#totp", "submitter": "#submit", "action": "/session"}
            item = self.private["item"]
            deadline = int(time.time()) + 120
            task = self.http("POST", "/v1/tasks", {"id": uid(), "session_id": session["id"], "conversation_id": None,
                "settings": {"model": "unavailable", "max_turns": 1, "max_tokens": 1, "deadline": deadline, "max_cost_micros": 0},
                "work": {"type": "computer", "spec": {"profile": "linux_playwright_v1", "account": {"uid": 1000, "gid": 1000},
                    "runtime": {"sha256": hashlib.sha256(b"P06 synthetic protocol worker; no browser").hexdigest()},
                    "deadline": deadline, "limits": {"runtime_seconds": 120, "output_bytes": 1024, "processes": 32, "memory_bytes": 268435456},
                    "viewport": {"width": 800, "height": 600}, "operations": [{"type": "protected_login", "item": item["id"],
                    "revision": item["revision"], "purpose": "password", "plan": plan}], "outputs": []}}})
            self.http("PUT", "/v1/tasks/" + task["id"] + "/approval", {"expires_at": deadline - 10})
            delivery = worker.until("delivery", lambda m: m["body"]["task"] == task["id"] and m["authority"] is not None)
            worker.send_message({"type": "claim", "incarnation": worker.journal, "fence": delivery["fence"], "binding": delivery["authority"]})
            worker.until("permit", lambda m: m["binding"]["task"] == task["id"])
            authority = {"task": task["id"], "execution": task["command"]["execution"], "fence": delivery["fence"], "journal": worker.journal}
            destination = {key: uid() for key in ("profile", "browser", "page", "top_document", "frame", "frame_document", "form", "submitter")}
            destination.update(top_origin=origin, frame_origin=origin, plan=plan, certain=True,
                fields=[{"object": uid(), "role": role, "name": role, "input_type": kind} for role, kind in (("username", "text"), ("password", "password"))])
            def protected(operation):
                return support.http(self.listeners["public"], "POST", "/worker/v1/protected-browser",
                    {"authority": authority, "operation": operation}, context=worker_context, headers={"Content-Type": "application/octet-stream"})
            prepare = {"type": "prepare", "destination": destination, "previous": None}
            if not before:
                code, value = protected(prepare)
                require(code == 403 and value == {"error": "unauthorized"}, "locked protected use was not refused")
                require(not submissions, "locked request submitted credentials")
                self.emit("locked_protected_prepare_refused", status=code, site_submissions=0, task=task["id"])
                owner.protected({"type": "unlock", "passphrase": self.private["passphrase"]})
                require(self.http("GET", "/v1/status")["vault"] == "unlocked", "explicit passphrase unlock failed")
                require(owner.call({"method": "login_items"})["value"] == [item], "metadata changed after reboot")
            code, value = protected(prepare)
            require(code == 200 and value["type"] == "review", "unlocked prepare refused")
            review = value["review"]
            require(review["state"] == "awaiting_approval" and review["item"] == item, "persisted every-fill policy changed")
            release = {"type": "release", "id": review["id"], "destination": destination,
                       "protection": {"type": "tracked", "profile": destination["profile"], "releases": [review["release"]], "complete": True}}
            require(protected(release)[0] == 403, "release without fill approval")
            approved = owner.call({"method": "decide_credential_use", "review": review, "decision": "allow_once"})
            require(approved["value"]["state"] == "authorized", "fill approval refused")
            code, reply = protected(release)
            require(code == 200 and reply["type"] == "released" and reply["values"] == {
                "type": "password", "username": self.private["username"], "password": self.private["secret"]}, "released login mismatch")
            connection = HTTPSConnection("127.0.0.1", site.server_port, context=context, timeout=10)
            connection.request("POST", "/session", urllib.parse.urlencode({key: reply["values"][key] for key in ("username", "password")}),
                               {"Content-Type": "application/x-www-form-urlencoded"})
            response = connection.getresponse()
            response.read()
            require(response.status == 200 and submissions == [True], "controlled site rejected released login")
            connection.close()
            code, settled = protected({"type": "settle", "id": review["id"], "applied": True})
            require(code == 200 and settled["review"]["state"] == "applied", "protected use settlement")
            require(protected(release)[0] == 403, "repeated release accepted")
            worker.send_message({"type": "reports", "generation": worker.generation, "incarnation": worker.journal,
                "high_water": 1, "reports": [{"sequence": 1, "command": task["command"]["id"],
                "execution": task["command"]["execution"], "event": {"type": "failed", "code": "unsupported_execution"}}]})
            worker.until("report_ack")
            log = (self.root / "manager.log").read_text()
            require(not any(self.private[key] in log for key in ("password", "passphrase", "device", "username", "secret")),
                    "private fixture value appeared in manager log")
            self.emit("unlocked_production_release_and_loopback_post", phase="before" if before else "after", task=task["id"],
                      item=item, approval_required=True, valid_site_posts=1, repeated_release_refused=True,
                      browser_executed=False, synthetic_worker=True)
            state = {**self.identity(), "ciphertext_sha256": self.ciphertext(), "vault": self.http("GET", "/v1/status")["vault"]}
            save(self.root / ("before.json" if before else "after.json"), state)
            self.emit("ready_for_reboot" if before else "reboot_verification_passed", service=state)
        finally:
            owner.close()
            if worker:
                worker.close()
            if site:
                site.shutdown()
                site.server_close()

    def export(self):
        names = ("password", "passphrase", "device", "username", "secret", "invitation")
        private = [str(self.private[k]) for k in names if k in self.private]
        for name in ("state/bootstrap.token",):
            path = self.root / name
            if path.exists():
                private.append(path.read_text().strip())
        evidence = {}
        for name in ("events.jsonl", "inspection.json", "before.json", "after.json", "manager.log", self.unit):
            path = self.root / name
            if path.exists():
                value = path.read_text()
                for secret in private:
                    value = value.replace(secret, "[REDACTED]")
                evidence[name] = value
        return evidence

    def cleanup(self):
        if Path("/etc/systemd/system", self.unit).exists():
            require(Path("/etc/systemd/system", self.unit).read_bytes() == (self.root / self.unit).read_bytes(), "fixture unit ownership mismatch")
            run(["sudo", "-n", "systemctl", "disable", "--now", self.unit])
            require(run(["systemctl", "show", self.unit, "--property=MainPID", "--value"]) == "0", "fixture process remains")
            require(run(["systemctl", "show", self.unit, "--property=ActiveState", "--value"]) == "inactive", "fixture service remains active")
            run(["sudo", "-n", "rm", "/etc/systemd/system/" + self.unit])
            run(["sudo", "-n", "systemctl", "daemon-reload"])
        require(run(["systemctl", "show", self.unit, "--property=LoadState", "--value"]) == "not-found", "fixture unit remains loaded")
        shutil.rmtree(self.root)
        return {"unit": self.unit, "stopped": True, "unit_removed": True, "root_removed": not self.root.exists()}


def orchestrate(args):
    require(args.guest == "botstral-manager", "only the authorized manager guest may reboot")
    root = Path(__file__).resolve().parents[2]
    evidence = args.evidence.resolve()
    evidence.mkdir(mode=0o700, parents=True, exist_ok=False)
    fixture_root = "/home/botstral/p06-reboot-" + secrets.token_hex(6)
    ledger = {"guest": args.guest, "root": fixture_root, "unit": "botstral-" + fixture_root.rsplit("/", 1)[1] + ".service",
              "started_utc": datetime.now(timezone.utc).isoformat(), "source_commit": run(["git", "-C", str(root), "rev-parse", "HEAD"]),
              "binary_sha256": digest(args.binary), "harness_sha256": digest(Path(__file__)),
              "support_sha256": digest(Path(__file__).with_name("m05_acceptance.py")), "reconnection_timeout_seconds": args.timeout}
    save(evidence / "resources.json", ledger)
    ssh = ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "ServerAliveInterval=5", "-o", "ServerAliveCountMax=2", args.guest]
    def remote(operation):
        result = subprocess.run([*ssh, shlex.join(["python3", fixture_root + "/p06_reboot.py", operation, "--root", fixture_root])],
                                capture_output=True, text=True, timeout=100)
        if result.returncode:
            (evidence / (operation + ".stderr")).write_text(result.stderr)
        require(result.returncode == 0, "remote " + operation + " failed; inspect private stderr")
        return json.loads(result.stdout) if result.stdout else None
    run([*ssh, shlex.join(["mkdir", "-m", "700", fixture_root])])
    run(["scp", str(Path(__file__).resolve()), str(Path(__file__).with_name("m05_acceptance.py")), args.guest + ":" + fixture_root + "/"])
    try:
        ledger["inventory"] = remote("inspect")
        run([*ssh, "sudo -n true"])
        run(["scp", str(args.binary.resolve()), args.guest + ":" + fixture_root + "/botstral-manager"], timeout=180)
        remote("before")
        artifacts = remote("export")
        initial = json.loads(artifacts["before.json"])
        require(initial["binary_sha256"] == ledger["binary_sha256"], "running binary differs from supplied build")
        for name, contents in artifacts.items():
            (evidence / name).write_text(contents)
        ledger["pre_reboot_inventory"] = remote("inspect")
        ledger["reboot_requested_utc"] = datetime.now(timezone.utc).isoformat()
        save(evidence / "resources.json", ledger)
        result = subprocess.run([*ssh, "sudo -n systemctl reboot"], capture_output=True, text=True, timeout=20)
        require(result.returncode in (0, 255), "guest reboot command failed")
        until = time.monotonic() + args.timeout
        reconnected = False
        attempts = 0
        while time.monotonic() < until:
            attempts += 1
            probe = subprocess.run([*ssh, "cat /proc/sys/kernel/random/boot_id"], capture_output=True, text=True, timeout=15)
            if probe.returncode == 0 and probe.stdout.strip() != initial["boot_id"]:
                reconnected = True
                break
            time.sleep(2)
        require(reconnected, "real guest reboot did not reconnect with a changed boot ID")
        ledger["reconnection_attempts"] = attempts
        remote("after")
        ledger["passed"] = True
    except Exception as error:
        ledger["passed"] = False
        ledger["failure"] = str(error)
        raise
    finally:
        try:
            for name, contents in remote("export").items():
                (evidence / name).write_text(contents)
        except Exception as error:
            ledger["export_error"] = str(error)
        try:
            ledger["cleanup"] = remote("cleanup")
            run([*ssh, shlex.join(["test", "!", "-e", fixture_root])])
        except Exception as error:
            ledger["cleanup_error"] = str(error)
        ledger["finished_utc"] = datetime.now(timezone.utc).isoformat()
        save(evidence / "resources.json", ledger)
    require("cleanup_error" not in ledger, "cleanup incomplete; inspect resources.json")
    print(json.dumps(ledger, indent=2))


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="operation", required=True)
    controller = commands.add_parser("run", help="Reboot the authorized manager guest and preserve value-free evidence.")
    controller.add_argument("--guest", default="botstral-manager")
    controller.add_argument("--binary", type=Path, required=True, help="Production Linux manager built from the recorded current source.")
    controller.add_argument("--evidence", type=Path, required=True)
    controller.add_argument("--timeout", type=int, default=180)
    for operation in ("inspect", "before", "after", "export", "cleanup"):
        subcommand = commands.add_parser(operation, help=argparse.SUPPRESS)
        subcommand.add_argument("--root", type=Path, required=True)
    args = parser.parse_args()
    if args.operation == "run":
        orchestrate(args)
        return
    fixture = Fixture(args.root)
    if args.operation == "inspect":
        print(json.dumps(inspect(args.root)))
    elif args.operation == "before":
        fixture.prepare()
        fixture.exercise(True)
    elif args.operation == "after":
        fixture.exercise(False)
    elif args.operation == "export":
        print(json.dumps(fixture.export()))
    elif args.operation == "cleanup":
        print(json.dumps(fixture.cleanup()))


if __name__ == "__main__":
    main()
