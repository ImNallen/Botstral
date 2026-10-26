#!/usr/bin/env python3
"""Bounded remote operations for the isolated W01 fixture."""
import os
os.umask(0o077)
import json
from pathlib import Path
import subprocess
import sys
import uuid
import m02_remote


def dispatch(request):
    op = request["op"]
    if op == "liveness":
        values = []
        for pid in request["pids"]:
            if not isinstance(pid, int) or pid <= 1:
                raise ValueError("invalid process identity")
            path = Path(f"/proc/{pid}/stat")
            value = path.read_text() if path.exists() else None
            values.append({"pid": pid, "live": value is not None and not value.rsplit(") ", 1)[1].startswith("Z"),
                           "stat": value})
        return values
    if op == "kill":
        return m02_remote.run(["systemctl", "--user", "kill", "--signal=SIGKILL", "--kill-whom=main", request["unit"]])
    if op == "remove_listeners":
        Path(request["root"], "state", "listeners.json").unlink(missing_ok=True)
        return True
    if op == "cleanup_processes":
        state = Path(request["root"], "agent")
        paths = [state / "process-probe" / "manifest.json", *state.glob("processes/*/manifest.json")]
        stopped = []
        for path in paths:
            if not path.exists():
                continue
            manifest = json.loads(path.read_text())
            unit = f"botstral-{uuid.UUID(manifest['journal'])}-{uuid.UUID(manifest['execution'])}.service"
            subprocess.run(["systemctl", "--user", "stop", unit], timeout=8, capture_output=True, check=False)
            subprocess.run(["systemctl", "--user", "reset-failed", unit], timeout=3, capture_output=True, check=False)
            stopped.append(unit)
        for unit in request.get("units", []):
            prefix = "m02-" + Path(request["root"]).name + "-production-"
            if not unit.startswith(prefix) or not unit.endswith(".service"):
                raise ValueError("fixture unit mismatch")
            subprocess.run(["systemctl", "--user", "stop", unit], timeout=8, capture_output=True, check=False)
            subprocess.run(["systemctl", "--user", "reset-failed", unit], timeout=3, capture_output=True, check=False)
        return stopped
    if op == "versions":
        return {"kernel": os.uname().release, "systemd": m02_remote.run(["systemd-run", "--version"]),
                "python": sys.version}
    return m02_remote.dispatch(request)


def main():
    try:
        value = dispatch(json.load(sys.stdin))
        print(json.dumps({"ok": True, "value": value}))
        return 0
    except Exception as error:
        print(json.dumps({"ok": False, "error": type(error).__name__}))
        return 1


if __name__ == "__main__":
    sys.exit(main())
