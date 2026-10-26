import json
import os
import platform
from pathlib import Path
import socket
import subprocess


def run(*arguments):
    return subprocess.check_output(arguments, text=True)


units = json.loads(run("systemctl", "--user", "list-units", "--all", "--output=json", "--no-pager"))
assert isinstance(units, list), units
fixtures = [unit for unit in units if unit["unit"].startswith(("m02-", "botstral-m02-probe-"))]
listeners = [line.split()[3] for line in run("ss", "-ltnH").splitlines()]
result = {
    "hostname": socket.gethostname(),
    "kernel": os.uname().release,
    "operating_system": platform.freedesktop_os_release()["PRETTY_NAME"],
    "cpus": os.cpu_count(),
    "memory_kib": int(next(line.split()[1] for line in Path("/proc/meminfo").read_text().splitlines()
                           if line.startswith("MemTotal:"))),
    "root_disk_bytes": int(run("lsblk", "-bndo", "SIZE", "/dev/sda").strip()),
    "fixture_units": fixtures,
    "listening_tcp": listeners,
}
print(json.dumps(result, indent=2))
raise SystemExit(1 if fixtures or any(address.rsplit(":", 1)[-1] in {"43122", "43123"}
                                    for address in listeners) else 0)
