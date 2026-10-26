#!/usr/bin/env python3
from __future__ import annotations

import http.client
import hashlib
import json
import os
from pathlib import Path
import socket
import ssl
import subprocess
import sys
import time
import uuid


TIMEOUT = 10


def private_write(path, content):
    path = Path(path)
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    with path.open("x", encoding="utf-8") as stream:
        os.chmod(path, 0o600)
        stream.write(content)


def run(arguments, timeout=3):
    result = subprocess.run(arguments, capture_output=True, text=True, timeout=timeout)
    if result.returncode:
        raise RuntimeError("subprocess failed")
    return result.stdout


def unit_state(unit, timeout=3):
    fields = (
        "LoadState", "ActiveState", "SubState", "ExecMainPID", "ExecMainCode",
        "ExecMainStatus", "Result", "ExecMainStartTimestampMonotonic",
        "ExecMainExitTimestampMonotonic",
    )
    output = run(["systemctl", "--user", "show", unit,
                  "--property=" + ",".join(fields)], timeout=timeout)
    state = dict(line.split("=", 1) for line in output.splitlines() if "=" in line)
    if state.get("LoadState") != "loaded":
        raise RuntimeError("recorded unit is missing")
    return state


def wait_exit(unit, terminate):
    evidence = {"unit": unit, "clean": False, "states": [], "cleanup_errors": []}
    deadline = time.monotonic() + TIMEOUT
    try:
        state = unit_state(unit, timeout=min(3, deadline - time.monotonic()))
        evidence["states"].append(state)
        if terminate:
            run(["systemctl", "--user", "kill", "--signal=SIGTERM",
                 "--kill-whom=main", unit], timeout=min(3, deadline - time.monotonic()))
        while True:
            if time.monotonic() >= deadline:
                raise TimeoutError("unit exit deadline")
            state = unit_state(unit, timeout=min(3, deadline - time.monotonic()))
            if state != evidence["states"][-1]:
                evidence["states"].append(state)
            if state["SubState"] == "exited":
                evidence["clean"] = (
                    state["ExecMainStatus"] == "0" and state["ExecMainCode"] == "1"
                    and state["Result"] == "success"
                    and int(state["ExecMainPID"]) > 0
                    and int(state["ExecMainExitTimestampMonotonic"]) > 0
                )
                break
            if state["ActiveState"] == "failed":
                break
            if time.monotonic() >= deadline:
                raise TimeoutError("unit exit deadline")
            time.sleep(min(0.1, max(0, deadline - time.monotonic())))
    except Exception as error:
        evidence["error"] = type(error).__name__
    finally:
        # Reset before stopping, while the transient unit still exists.
        for verb in ("reset-failed", "stop"):
            try:
                run(["systemctl", "--user", verb, unit])
            except Exception as error:
                evidence["cleanup_errors"].append(verb + ":" + type(error).__name__)
    evidence["clean"] = evidence["clean"] and not evidence["cleanup_errors"]
    return evidence


def context(request):
    ctx = ssl.create_default_context(cafile=request["ca"])
    ctx.load_cert_chain(request["cert"], request["key"])
    return ctx


def http_request(request):
    host, port = request["address"].rsplit(":", 1)
    timeout = min(TIMEOUT, request.get("timeout", TIMEOUT))
    if request.get("tls"):
        connection = http.client.HTTPSConnection(host, int(port), timeout=timeout,
                                                 context=context(request))
    else:
        if host != "127.0.0.1":
            raise ValueError("owner HTTP must use manager loopback")
        connection = http.client.HTTPConnection(host, int(port), timeout=timeout)
    headers = {"Content-Type": "application/json", "Connection": "close"}
    if request.get("token"):
        headers["Authorization"] = "Bearer " + request["token"]
    try:
        connection.request(request["method"], request["path"],
                           json.dumps(request.get("body", {})), headers)
        response = connection.getresponse()
        data = response.read()
        return {"status": response.status, "body": json.loads(data) if data else None}
    finally:
        connection.close()


def tls_rejection(request):
    ctx = context(request)
    host, port = request["address"].rsplit(":", 1)
    try:
        with socket.create_connection((host, int(port)), timeout=TIMEOUT) as raw:
            with ctx.wrap_socket(raw, server_hostname=request.get("hostname", host)) as stream:
                stream.sendall(b"GET /worker/v1/connect HTTP/1.1\r\nHost: probe\r\nConnection: close\r\n\r\n")
                response = stream.recv(4096)
                # TLS 1.3 can defer a client-certificate alert until the first read.
                if not response:
                    raise RuntimeError("EOF is not explicit TLS rejection evidence")
    except ssl.SSLError as error:
        return {"rejected": True, "reason": error.reason,
                "verify_code": getattr(error, "verify_code", None)}
    return {"rejected": False}


def websocket_probe(request):
    import websocket

    connection = websocket.create_connection(
        "wss://" + request["address"] + "/worker/v1/connect", timeout=TIMEOUT,
        sslopt={"cert_reqs": ssl.CERT_REQUIRED, "ca_certs": request["ca"],
                "certfile": request["cert"], "keyfile": request["key"]},
        http_no_proxy=["*"], http_proxy_host=None, http_proxy_port=None, suppress_origin=True,
    )
    try:
        request_id = str(uuid.uuid4())
        connection.send(json.dumps({"version": 1, "request_id": request_id,
                                    "message": request["message"]}))
        response = json.loads(connection.recv())
        if response["version"] != 1:
            raise ValueError("unexpected envelope version")
        uuid.UUID(response["request_id"])
        return response["message"]
    finally:
        connection.close(timeout=1)


def dispatch(request):
    op = request["op"]
    if op == "identity":
        addresses = json.loads(run(["ip", "-j", "address", "show"]))
        return {
            "hostname": socket.gethostname(),
            "machine_id_sha256": hashlib.sha256(Path("/etc/machine-id").read_bytes()).hexdigest(),
            "kernel": os.uname().release,
            "addresses": [item["local"] for interface in addresses
                          for item in interface["addr_info"]
                          if item["family"] == "inet" and item["scope"] == "global"],
            "binaries": {path: hashlib.sha256(Path(path).read_bytes()).hexdigest()
                         for path in request["binaries"]},
        }
    if op == "write":
        for path, content in request["files"].items():
            private_write(path, content)
        return True
    if op == "read":
        path = Path(request["path"])
        if not path.exists() and request.get("optional"):
            return None
        return path.read_text()
    if op == "http":
        return http_request(request)
    if op == "http_batch":
        return [http_request(item) for item in request["requests"]]
    if op == "tls_rejection":
        return tls_rejection(request)
    if op == "ws":
        return websocket_probe(request)
    if op == "start":
        private_write(request["log"], "")
        run(["systemd-run", "--user", "--unit=" + request["unit"],
             "--remain-after-exit", "--property=UMask=0077",
             "--property=StandardOutput=append:" + request["log"],
             "--property=StandardError=append:" + request["log"],
             "--", *request["args"]])
        return unit_state(request["unit"])
    if op == "exit":
        return wait_exit(request["unit"], request["terminate"])
    if op == "permissions":
        if not Path(request["root"]).is_dir():
            raise FileNotFoundError("fixture root missing")
        failures = []
        for root, directories, files in os.walk(request["root"]):
            for path in [Path(root), *(Path(root) / name for name in directories + files)]:
                expected = 0o700 if path.is_dir() else 0o600
                if path.is_symlink() or path.stat().st_mode & 0o777 != expected:
                    failures.append(str(path))
        return failures
    if op == "scan_state":
        root = Path(request["root"])
        if not root.is_dir():
            raise FileNotFoundError("state root missing")
        needles = [value.encode() for value in request["secrets"] if value]
        needles.append(b"PRIVATE KEY")
        files, leaks = [], []
        for path in root.rglob("*"):
            if path.is_file():
                relative = str(path.relative_to(root))
                files.append(relative)
                data = path.read_bytes()
                if any(needle in data for needle in needles):
                    leaks.append(relative)
        return {"files": files, "leaks": leaks}
    raise ValueError("unknown operation")


def main():
    os.umask(0o077)
    try:
        value = dispatch(json.load(sys.stdin))
        print(json.dumps({"ok": True, "value": value}))
    except Exception as error:
        print(json.dumps({"ok": False, "error": type(error).__name__}))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
