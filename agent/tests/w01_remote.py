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


def systemctl(*arguments):
    result = subprocess.run(["systemctl", "--user", *arguments], timeout=8,
                            capture_output=True, text=True, check=False)
    if result.returncode != 0:
        raise RuntimeError("fixture service operation failed")
    return result.stdout


def stop_fixture_unit(unit):
    def state():
        output = systemctl("show", unit, "--property=LoadState,ActiveState,ControlGroup,InvocationID")
        return dict(line.split("=", 1) for line in output.splitlines() if "=" in line)

    before = state()
    if before.get("LoadState") == "not-found":
        return {"unit": unit, "scope": "absent"}
    if before.get("LoadState") != "loaded":
        raise RuntimeError("fixture service state unavailable")
    group = before.get("ControlGroup", "")
    if group and (not group.startswith("/") or ".." in group.split("/") or not group.endswith(unit)):
        raise RuntimeError("fixture service scope mismatch")
    systemctl("stop", unit)
    after = state()
    if after.get("LoadState") != "not-found" and after.get("ActiveState") not in ("inactive", "failed"):
        raise RuntimeError("fixture service remains active")
    scope = "absent"
    if group:
        try:
            events = (Path("/sys/fs/cgroup") / group.lstrip("/") / "cgroup.events").read_text()
        except FileNotFoundError:
            scope = "removed"
        else:
            if "populated 0" not in events.splitlines():
                raise RuntimeError("fixture service scope remains populated")
            scope = "empty"
    if after.get("LoadState") == "loaded" and after.get("ActiveState") == "failed":
        systemctl("reset-failed", unit)
    return {"unit": unit, "scope": scope, "before": before, "after": after}


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
        paths = [state / "process-probe" / "manifest.json", state / "computer-probe" / "manifest.json", *state.glob("processes/*/manifest.json")]
        units = set()
        for path in paths:
            if not path.exists():
                continue
            manifest = json.loads(path.read_text())
            unit = f"botstral-{uuid.UUID(manifest['journal'])}-{uuid.UUID(manifest['execution'])}.service"
            units.add(unit)
        for unit in request.get("units", []):
            prefix = "m02-" + Path(request["root"]).name + "-production-"
            if not unit.startswith(prefix) or not unit.endswith(".service"):
                raise ValueError("fixture unit mismatch")
            units.add(unit)
        return [stop_fixture_unit(unit) for unit in sorted(units)]
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
