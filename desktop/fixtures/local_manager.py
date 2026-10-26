#!/usr/bin/env python3
"""Run an isolated real manager with synthetic owner and bot records for native UI checks."""
import argparse
import json
import pathlib
import signal
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid

PASSWORD = "SYNTHETIC desktop password"
VAULT_PHRASE = "SYNTHETIC separate vault phrase"
ROOT = pathlib.Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--listen", default="127.0.0.1:43120")
    parser.add_argument("--empty", action="store_true", help="Start without bots.")
    args = parser.parse_args()
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def request(address, method, route, body=None, bearer=None):
        data = None if body is None else json.dumps(body).encode()
        headers = {"Content-Type": "application/json"}
        if bearer is not None:
            headers["Authorization"] = "Bearer " + bearer
        with opener.open(urllib.request.Request("http://" + address + route, data, headers, method=method), timeout=5) as response:
            data = response.read()
            return json.loads(data) if data else None

    with tempfile.TemporaryDirectory(prefix="botstral-desktop-fixture-") as temporary:
        state = pathlib.Path(temporary) / "state"
        manager = subprocess.Popen([
            str(ROOT / "target/debug/botstral-manager"), "--state-dir", str(state),
            "--listen", args.listen, "--setup-listen", "127.0.0.1:0",
        ], stdout=subprocess.DEVNULL)
        try:
            deadline = time.monotonic() + 20
            while not (state / "listeners.json").exists():
                if manager.poll() is not None or time.monotonic() > deadline:
                    raise RuntimeError("The fixture manager did not start. Check that its port is free.")
                time.sleep(0.05)
            addresses = json.loads((state / "listeners.json").read_text())
            request(addresses["setup"], "POST", "/v1/owner", {
                "name": "Local owner", "password": PASSWORD, "vault_passphrase": VAULT_PHRASE,
                "acknowledge_no_escrow": True,
            }, (state / "bootstrap.token").read_text())
            grant = request(addresses["listen"], "POST", "/v1/sessions", {"password": PASSWORD})
            token = grant.pop("token")
            if not args.empty:
                for name in ["Research", "Writing", "Planning"]:
                    request(addresses["listen"], "PUT", "/v1/bots/" + str(uuid.uuid4()), {"name": name}, token)
            request(addresses["listen"], "DELETE", "/v1/session", {}, token)
            del token, grant
            print("Synthetic fixture manager at " + addresses["listen"], flush=True)
            print("Synthetic owner password: " + PASSWORD, flush=True)
            print("Press Control-C to stop the fixture manager and remove its temporary state.", flush=True)
            manager.wait()
        except KeyboardInterrupt:
            pass
        finally:
            if manager.poll() is None:
                manager.send_signal(signal.SIGTERM)
                try:
                    manager.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    manager.kill()
                    manager.wait()


if __name__ == "__main__":
    main()
