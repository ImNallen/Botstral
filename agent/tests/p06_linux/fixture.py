#!/usr/bin/env python3
"""Exercise production Linux supervision and paired approval without a private bridge."""
import argparse
import base64
import hashlib
from http.client import HTTPConnection, HTTPSConnection
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import secrets
import socket
import ssl
import subprocess
import threading
import time
import urllib.parse
import uuid

import websocket

PRIVATE_VALUES = []


def emit(name, **details):
    encoded = json.dumps({"check": name, **details})
    for value in PRIVATE_VALUES:
        forms = (value, json.dumps(value)[1:-1], urllib.parse.quote(value, safe=""),
                 base64.b64encode(value.encode()).decode())
        if any(form in encoded for form in forms):
            raise RuntimeError("private fixture value withheld from evidence")
    print(encoded, flush=True)


def command(args):
    result = subprocess.run(args, capture_output=True, timeout=30)
    emit("command", argv=args, exit=result.returncode)
    if result.returncode:
        raise RuntimeError("command refused " + args[0])
    return result.stdout.decode()


def poll(read, predicate, label, seconds=35):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = read()
        if predicate(value):
            return value
        time.sleep(0.2)
    raise RuntimeError("deadline " + label)


class Owner:
    def __init__(self, ws):
        self.ws = ws
        self.last_ping = 0

    def ping(self):
        if time.monotonic() - self.last_ping >= 5:
            self.ws.send(json.dumps({"type": "ping"}))
            assert json.loads(self.ws.recv())["type"] == "pong"
            self.last_ping = time.monotonic()

    def call(self, operation):
        request = str(uuid.uuid4())
        self.ws.send(json.dumps({"type": "call", "id": request, "operation": operation}))
        chunks = []
        start = None
        while True:
            reply = json.loads(self.ws.recv())
            if reply["type"] == "error":
                raise RuntimeError("paired owner refused " + json.dumps(reply["code"]))
            if reply["type"] == "transfer_start":
                assert reply["request"] == request
                start = reply
            elif reply["type"] == "transfer_chunk":
                assert start and reply["id"] == start["id"]
                assert reply["index"] == len(chunks)
                chunks.append(base64.b64decode(reply["data"]))
            elif reply["type"] == "transfer_complete":
                data = b"".join(chunks)
                assert start and len(data) == start["bytes"]
                assert hashlib.sha256(data).hexdigest() == start["digest"]
                return json.loads(data)


def exercise(root, runtime_directory):
    os.umask(0o077)
    root.mkdir(mode=0o700)
    os.chdir(root)
    environment = {"machine": os.uname().machine,
                   "cgroup": Path("/proc/self/cgroup").read_text().strip(),
                   "systemd": command(["systemd", "--version"]).splitlines()[0]}
    emit("environment", **environment)
    supervisor = subprocess.run(["botstral-agent-supervisor-check", "--scratch", str(root / "supervisor")],
                                capture_output=True, text=True, timeout=30)
    emit("production_supervisor", exit=supervisor.returncode,
         stdout=supervisor.stdout.strip(), stderr=supervisor.stderr.strip())
    if supervisor.returncode:
        return 2
    command(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", "ca.key",
             "-out", "ca.pem", "-days", "1", "-subj", "/CN=P06 isolated fixture CA",
             "-addext", "basicConstraints=critical,CA:TRUE"])
    for name, usage in (("manager", "serverAuth"), ("worker", "clientAuth")):
        command(["openssl", "req", "-new", "-newkey", "rsa:2048", "-nodes", "-keyout", name + ".key",
                 "-out", name + ".csr", "-subj", "/CN=" + name])
        Path(name + ".ext").write_text("subjectAltName=IP:127.0.0.1\nbasicConstraints=CA:FALSE\n"
                                      "keyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=" + usage + "\n")
        command(["openssl", "x509", "-req", "-in", name + ".csr", "-CA", "ca.pem", "-CAkey", "ca.key",
                 "-CAcreateserial", "-out", name + ".pem", "-days", "1", "-extfile", name + ".ext"])
    processes = []
    owner = None
    server = None
    def start(name, args):
        log = open(name + ".log", "wb")
        process = subprocess.Popen(args, stdout=log, stderr=log)
        log.close()
        processes.append(process)
        return process
    token = ""
    listeners = None
    def http(method, path, body=None, setup=False, expected=200, override=None):
        if owner:
            owner.ping()
        address = listeners["setup" if setup else "listen"]
        host, port = address.rsplit(":", 1)
        connection = HTTPConnection(host, int(port), timeout=10)
        credential = token if override is None else override
        headers = {"content-type": "application/json", "authorization": "Bearer " + credential}
        connection.request(method, path, json.dumps(body or {}), headers)
        response = connection.getresponse()
        data = response.read()
        connection.close()
        if response.status != expected:
            raise RuntimeError("HTTP " + method + " " + path + " status " + str(response.status))
        return json.loads(data) if data else None
    try:
        start("manager", ["botstral-manager", "--state-dir", str(root / "manager-state"),
                          "--listen", "127.0.0.1:0", "--setup-listen", "127.0.0.1:0",
                          "--worker-listen", "127.0.0.1:44319", "--worker-tls-cert", str(root / "manager.pem"),
                          "--worker-tls-key", str(root / "manager.key"), "--worker-client-ca", str(root / "ca.pem")])
        listener_path = root / "manager-state/listeners.json"
        listeners = poll(lambda: json.loads(listener_path.read_text()) if listener_path.exists() else None,
                         bool, "manager listeners")
        password, passphrase = secrets.token_urlsafe(32), secrets.token_urlsafe(32)
        PRIVATE_VALUES.extend((password, passphrase))
        bootstrap = (root / "manager-state/bootstrap.token").read_text().strip()
        http("POST", "/v1/owner", {"name": "P06 synthetic owner", "password": password,
             "vault_passphrase": passphrase, "acknowledge_no_escrow": True}, True, 201, bootstrap)
        token = http("POST", "/v1/sessions", {"password": password}, expected=201, override="")["token"]
        emit("manager_started", listeners=listeners)
        device_secret = secrets.token_urlsafe(32)
        PRIVATE_VALUES.append(device_secret)
        fingerprint = hashlib.sha256(b"botstral-owner-device-v1\0" + device_secret.encode()).hexdigest()
        invitation = http("POST", "/v1/device-invitations", {"request": {"fingerprint": fingerprint,
                          "label": "P06 Linux test lane"}, "lifetime_seconds": 600})
        ws = websocket.create_connection("wss://127.0.0.1:44319/owner/v1/connect", timeout=10,
             sslopt={"ca_certs": str(root / "ca.pem"), "cert_reqs": ssl.CERT_REQUIRED},
             suppress_origin=True, http_no_proxy=["*"])
        ws.send(json.dumps({"type": "enroll", "version": 3, "manager": invitation["manager"],
                           "role": "owner", "credential": device_secret, "invitation": invitation["invitation"]}))
        assert json.loads(ws.recv())["type"] == "welcome"
        owner = Owner(ws)
        emit("paired_owner_authenticated")
        unlock = {"id": str(uuid.uuid4()), "operation": {"type": "unlock", "passphrase": passphrase}}
        ws.send_binary(b"botstral-protected-owner-v1\0" + json.dumps(unlock).encode())
        reply = json.loads(ws.recv())
        assert reply["type"] == "protected_reply"
        emit("paired_private_unlock", receipt=reply)
        der = ssl.PEM_cert_to_DER_cert((root / "worker.pem").read_text())
        grant = http("POST", "/v1/worker-invitations", {"fingerprint": hashlib.sha256(der).hexdigest(),
                     "lifetime_seconds": 600}, expected=201)
        Path("invitation.json").write_text(json.dumps(grant))
        command(["botstral-agent", "enroll", "--state-dir", str(root / "agent"), "--manager", "wss://127.0.0.1:44319",
                 "--manager-ca", str(root / "ca.pem"), "--client-cert", str(root / "worker.pem"),
                 "--client-key", str(root / "worker.key"), "--invitation-file", str(root / "invitation.json")])
        runtime = hashlib.sha256((runtime_directory / "runtime.json").read_bytes()).hexdigest()
        limits = {"runtime_seconds": 120, "output_bytes": 1024, "processes": 128, "memory_bytes": 1073741824}
        process_limits = {**limits, "processes": 32, "memory_bytes": 268435456}
        account = {"uid": os.getuid(), "gid": os.getgid()}
        Path("agent/process.json").write_text(json.dumps({"profile": "linux_systemd_v1", "account": account,
                                                       "ceilings": process_limits}))
        Path("agent/computer.json").write_text(json.dumps({"account": account, "runtime": {"sha256": runtime},
                                   "directory": str(runtime_directory), "ceilings": limits}))
        emit("runtime_identity", directory=str(runtime_directory), sha256=runtime)
        start("worker", ["botstral-agent", "run", "--state-dir", str(root / "agent")])
        workers = poll(lambda: http("GET", "/v1/workers"),
                       lambda items: any(i["phase"] == "ready" for i in items), "production worker", seconds=180)
        worker = next(i for i in workers if i["phase"] == "ready")
        emit("worker_ready", worker=worker)
        bot = str(uuid.uuid4())
        http("PUT", "/v1/bots/" + bot, {"name": "P06 Linux synthetic bot"})
        http("PUT", "/v1/bots/" + bot + "/permission", {"expected_revision": 1, "allow": True})
        session = http("POST", "/v1/computer-sessions", {"bot_id": bot, "worker_id": worker["id"]})
        session = poll(lambda: http("GET", "/v1/computer-sessions/" + session["id"]),
                       lambda s: s["status"] == "confirmed", "session confirmation")
        def submit(operations):
            deadline = int(time.time()) + 120
            task = {"id": str(uuid.uuid4()), "session_id": session["id"], "conversation_id": None,
                    "settings": {"model": "unavailable", "max_turns": 1, "max_tokens": 1, "deadline": deadline,
                                 "max_cost_micros": 0},
                    "work": {"type": "computer", "spec": {"profile": "linux_playwright_v1", "account": account,
                             "runtime": {"sha256": runtime}, "deadline": deadline, "limits": limits,
                             "viewport": {"width": 800, "height": 600}, "operations": operations, "outputs": []}}}
            result = http("POST", "/v1/tasks", task)
            http("PUT", "/v1/tasks/" + task["id"] + "/approval", {"expires_at": int(time.time()) + 100})
            return result
        warm = submit([{"type": "write_file", "path": str(root / "warm.txt"), "text": "synthetic warmup"}])
        warm = poll(lambda: http("GET", "/v1/tasks/" + warm["id"]),
                    lambda t: t["state"] in ("completed", "failed", "awaiting_attention"), "warmup")
        emit("sealed_runtime_warmup", task=warm)
        assert warm["state"] == "completed"
        profile = root / "agent/automation-profile"
        nss = profile / ".pki/nssdb"
        nss.mkdir(parents=True)
        command(["certutil", "-N", "--empty-password", "-d", "sql:" + str(nss)])
        command(["certutil", "-A", "-d", "sql:" + str(nss), "-n", "P06 fixture CA", "-t", "C,,",
                 "-i", str(root / "ca.pem")])
        username, secret = secrets.token_urlsafe(24), secrets.token_urlsafe(32)
        PRIVATE_VALUES.extend((username, secret))
        submissions = []
        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(200)
                self.send_header("content-type", "text/html")
                self.end_headers()
                self.wfile.write(b'<!doctype html><form id="login" method="post" action="/session" data-botstral-login="v1">'
                    b'<input id="username" name="username" type="text"><input id="password" name="password" type="password">'
                    b'<button id="submit" type="submit">Sign in</button></form>')
            def do_POST(self):
                body = self.rfile.read(int(self.headers["content-length"]))
                fields = urllib.parse.parse_qs(body.decode())
                valid = self.path == "/session" and fields == {"username": [username], "password": [secret]}
                submissions.append({"valid": valid, "path": self.path, "method": "POST"})
                self.send_response(200 if valid else 401)
                if valid:
                    self.send_header("set-cookie", "synthetic-session=authenticated; Secure; HttpOnly; SameSite=Strict")
                self.end_headers()
                self.wfile.write(b"Signed in" if valid else b"Refused")
            def log_message(self, *_):
                pass
        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        tls.load_cert_chain("manager.pem", "manager.key")
        server.socket = tls.wrap_socket(server.socket, server_side=True)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        origin = "https://127.0.0.1:" + str(server.server_port)
        write = {"id": str(uuid.uuid4()), "operation": {"type": "write", "target": {"type": "create",
             "metadata": {"label": "Linux synthetic sign-in", "bots": {"type": "selected", "bots": [bot]},
                          "allowed_origins": [origin], "password_policy": "every_fill"}},
             "values": {"username": username, "password": secret, "totp_seed": ""}}}
        ws.send_binary(b"botstral-protected-owner-v1\0" + json.dumps(write).encode())
        receipt = json.loads(ws.recv())
        emit("protected_write_receipt", receipt=receipt)
        assert receipt["type"] == "protected_reply" and receipt["status"]["type"] == "committed"
        item = receipt["status"]["item"]
        emit("protected_item_committed", item=item)
        plan = {"version": 1, "origin": origin, "route": "/login", "frame": None, "form": "#login",
                "username": "#username", "password": "#password", "totp": "#totp", "submitter": "#submit", "action": "/session"}
        task = submit([{"type": "protected_login", "item": item["id"], "revision": item["revision"],
                        "purpose": "password", "plan": plan}])
        reviews = poll(lambda: owner.call({"method": "credential_uses"})["value"],
                       lambda reviews: any(r["state"] == "awaiting_approval" for r in reviews), "live credential review")
        review = next(r for r in reviews if r["state"] == "awaiting_approval")
        execution = task["command"]["execution"]
        execution_dir = root / "agent/processes" / execution
        ready = json.loads((execution_dir / "ready.json").read_text())
        group = ready["scope"]["cgroup"]
        pids = (Path("/sys/fs/cgroup") / group.lstrip("/") / "cgroup.procs").read_text().split()
        peers = [{"pid": int(pid), "comm": Path("/proc", pid, "comm").read_text().strip(),
                  "cgroup": Path("/proc", pid, "cgroup").read_text().strip()} for pid in pids]
        emit("supervised_browser_awaiting_approval", review=review, scope=ready["scope"], peers=peers)
        authority = {"task": task["id"], "execution": execution, "fence": review["fence"],
                     "journal": review["incarnation"]}
        def private_http(operation, authority=authority, authenticated=True):
            context = ssl.create_default_context(cafile="ca.pem")
            if authenticated:
                context.load_cert_chain("worker.pem", "worker.key")
            connection = HTTPSConnection("127.0.0.1", 44319, context=context, timeout=10)
            connection.request("POST", "/worker/v1/protected-browser",
                               json.dumps({"authority": authority, "operation": operation}),
                               {"content-type": "application/octet-stream"})
            response = connection.getresponse()
            data = response.read()
            connection.close()
            return response.status, data
        poll_operation = {"type": "poll", "id": review["id"]}
        status, data = private_http(poll_operation)
        assert status == 200 and json.loads(data)["type"] == "review"
        emit("authenticated_worker_tls_poll", status=status)
        status, _ = private_http(poll_operation, authenticated=False)
        assert status == 403
        emit("missing_client_certificate_refused", status=status)
        stale = {**authority, "fence": {**authority["fence"], "generation": authority["fence"]["generation"] + 1}}
        status, _ = private_http(poll_operation, authority=stale)
        assert status == 403
        emit("changed_generation_refused", status=status)
        outsider = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        outsider.settimeout(5)
        outsider.connect(str(execution_dir / "protected.sock"))
        outsider.sendall(b'{"type":"poll","id":"' + review["id"].encode() + b'"}')
        outsider.shutdown(socket.SHUT_WR)
        try:
            refused = outsider.recv(1024) == b""
        except ConnectionResetError:
            refused = True
        outsider.close()
        assert refused and not submissions
        emit("same_uid_outside_scope_refused", response_bytes=0, submissions=len(submissions))
        decision = owner.call({"method": "decide_credential_use", "review": review, "decision": "allow_once"})
        assert decision["value"]["state"] == "authorized"
        final = poll(lambda: http("GET", "/v1/tasks/" + task["id"]),
                     lambda t: t["state"] in ("completed", "failed", "awaiting_attention"), "sign-in settlement", seconds=60)
        review_after = owner.call({"method": "credential_uses"})["value"]
        history = json.loads((root / "agent/automation-protection.json").read_text())
        emit("sign_in_result", task=final, reviews=review_after, submissions=submissions, provenance=history)
        assert final["state"] == "completed"
        assert submissions == [{"valid": True, "path": "/session", "method": "POST"}]
        assert any(r["id"] == review["id"] and r["state"] == "applied" for r in review_after)
        assert history["consumed"] == [review["release"]]
        status, _ = private_http({"type": "release", "id": review["id"],
                                 "protection": history["protection"], "destination": review["destination"]})
        assert status == 403
        emit("repeated_tls_release_refused", status=status)
        emit("production_linux_protected_sign_in_passed")
        return 0
    finally:
        if owner:
            owner.ws.close()
        if server:
            server.shutdown()
        for process in reversed(processes):
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        manifests = list((root / "agent").glob("*/manifest.json"))
        manifests += list((root / "agent/processes").glob("*/manifest.json"))
        for manifest_path in manifests:
            manifest = json.loads(manifest_path.read_text())
            unit = "botstral-" + manifest["journal"] + "-" + manifest["execution"] + ".service"
            loaded = subprocess.run(["systemctl", "--user", "show", unit, "--property=LoadState", "--value"],
                                    capture_output=True, text=True)
            if loaded.stdout.strip() == "not-found":
                emit("owned_scope_already_removed", unit=unit)
                continue
            stopped = subprocess.run(["systemctl", "--user", "stop", unit], capture_output=True)
            emit("owned_scope_cleanup", unit=unit, exit=stopped.returncode)
        emit("fixture_processes_stopped", exits=[p.returncode for p in processes], root=str(root))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--runtime", type=Path, default=Path("/opt/p06-runtime"))
    args = parser.parse_args()
    try:
        raise SystemExit(exercise(args.root, args.runtime))
    except Exception as error:
        emit("fixture_failed", error=str(error), exception=type(error).__name__)
        raise SystemExit(1)
