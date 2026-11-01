#!/usr/bin/env bash
set -euo pipefail
umask 077

certs=${1:?Provide the fixture certificate directory.}
output=${2:?Provide the local evidence directory.}
manager=${3:?Provide the manager SSH alias.}
worker=${4:?Provide the worker SSH alias.}
manager_ip=${5:?Provide the manager IPv4 address.}
worker_ip=${6:?Provide the worker IPv4 address.}
python3 - "$manager" "$worker" "$manager_ip" "$worker_ip" <<'PY'
import ipaddress, re, sys
for alias in sys.argv[1:3]:
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*", alias):
        raise SystemExit("Use a configured SSH alias.")
for address in sys.argv[3:]:
    ipaddress.IPv4Address(address)
PY
source_dir=$(cd "$(dirname "$0")" && pwd)
run_id=$(python3 -c 'import uuid; print(uuid.uuid4().hex)')
remote_root="/home/botstral/m02-acceptance/probe-$run_id"
probe_unit="botstral-m02-probe-$run_id"
agent_unit="botstral-m02-probe-agent-$run_id"
ssh_options=(-o BatchMode=yes -o StrictHostKeyChecking=yes -o ConnectTimeout=8)
mkdir -p "$output"
test -z "$(ls -A "$output")"
python3 - "$source_dir" "$output" <<'PY'
import hashlib, json, pathlib, sys
source, output = map(pathlib.Path, sys.argv[1:])
names = ("m02_daemon_probe.py", "m02_daemon_probe.sh")
manifest = {name: hashlib.sha256((source / name).read_bytes()).hexdigest() for name in names}
(output / "runner-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
PY

cleanup() {
    result=$?
    trap - EXIT
    ssh "${ssh_options[@]}" "$worker" "systemctl --user reset-failed '$agent_unit'" >> "$output/cleanup.log" 2>&1 || result=1
    ssh "${ssh_options[@]}" "$manager" "systemctl --user reset-failed '$probe_unit'" >> "$output/cleanup.log" 2>&1 || result=1
    ssh "${ssh_options[@]}" "$worker" "systemctl --user stop '$agent_unit'" >> "$output/cleanup.log" 2>&1 || result=1
    ssh "${ssh_options[@]}" "$manager" "systemctl --user stop '$probe_unit'" >> "$output/cleanup.log" 2>&1 || result=1
    exit "$result"
}
trap cleanup EXIT

ssh "${ssh_options[@]}" "$manager" "umask 077; mkdir -p '$remote_root'"
ssh "${ssh_options[@]}" "$worker" "umask 077; mkdir -p '$remote_root/state'"
scp -q "${ssh_options[@]}" "$source_dir/m02_daemon_probe.py" \
    "$certs/ca.pem" "$certs/manager.pem" "$certs/manager.key" "$certs/worker.pem" \
    "$manager:$remote_root/"
scp -q "${ssh_options[@]}" "$certs/ca.pem" "$certs/worker.pem" "$certs/worker.key" \
    "$worker:$remote_root/"
ssh "${ssh_options[@]}" "$manager" "chmod 0700 '$remote_root'; chmod 0600 '$remote_root/'*"
ssh "${ssh_options[@]}" "$worker" "chmod 0700 '$remote_root' '$remote_root/state'; chmod 0600 '$remote_root/'*.pem '$remote_root/'*.key"

python3 - "$remote_root" "$manager_ip" <<'PY' | ssh "${ssh_options[@]}" "$worker" "umask 077; cat > '$remote_root/state/config.json'"
import json, sys
root, address = sys.argv[1:]
print(json.dumps({
    "manager": f"wss://{address}:43123",
    "pin": "0" * 64,
    "client_cert": root + "/worker.pem",
    "client_key": root + "/worker.key",
}))
PY

ssh "${ssh_options[@]}" "$manager" \
    "systemd-run --user --quiet --unit '$probe_unit' --remain-after-exit --property=UMask=0077 --property=WorkingDirectory='$remote_root' --property=StandardOutput=append:'$remote_root/probe.log' --property=StandardError=append:'$remote_root/probe.log' /usr/bin/python3 '$remote_root/m02_daemon_probe.py' --listen '$manager_ip' --worker-address '$worker_ip' --server-cert '$remote_root/manager.pem' --server-key '$remote_root/manager.key' --ca '$remote_root/ca.pem' --client-cert '$remote_root/worker.pem' --output-dir '$remote_root/result'"

ssh "${ssh_options[@]}" "$manager" "python3 - '$remote_root/result/ready'" <<'PY'
import pathlib, sys, time
path = pathlib.Path(sys.argv[1])
deadline = time.monotonic() + 10
while not path.exists():
    if time.monotonic() > deadline:
        raise SystemExit("Probe listener did not become ready.")
    time.sleep(0.1)
PY

ssh "${ssh_options[@]}" "$worker" \
    "systemd-run --user --quiet --unit '$agent_unit' --remain-after-exit --property=UMask=0077 --property=WorkingDirectory='$remote_root' --property=StandardOutput=append:'$remote_root/agent.log' --property=StandardError=append:'$remote_root/agent.log' /home/botstral/m02-acceptance/bin/test/botstral-test-agent run --state-dir '$remote_root/state'"

ssh "${ssh_options[@]}" "$manager" "python3 - '$remote_root/result/result.json'" <<'PY'
import json, pathlib, sys, time
path = pathlib.Path(sys.argv[1])
deadline = time.monotonic() + 35
while not path.exists():
    if time.monotonic() > deadline:
        raise SystemExit("Probe did not finish before its deadline.")
    time.sleep(0.1)
print(path.read_text())
PY
scp -q "${ssh_options[@]}" "$manager:$remote_root/result/result.json" "$output/result.json"
scp -q "${ssh_options[@]}" "$manager:$remote_root/probe.log" "$output/probe.log"
ssh "${ssh_options[@]}" "$worker" "systemctl --user kill --signal=SIGTERM --kill-whom=main '$agent_unit'"
ssh "${ssh_options[@]}" "$worker" "python3 - '$agent_unit'" > "$output/agent-shutdown.json" <<'PY'
import json, subprocess, sys, time
deadline = time.monotonic() + 8
while True:
    text = subprocess.check_output([
        "systemctl", "--user", "show", sys.argv[1],
        "-p", "SubState", "-p", "ExecMainStatus", "-p", "ExecMainCode", "-p", "Result",
    ], text=True)
    state = dict(line.split("=", 1) for line in text.splitlines())
    if state.get("SubState") == "exited":
        assert state["Result"] == "success", state
        assert state["ExecMainCode"] == "1", state
        assert state["ExecMainStatus"] == "0", state
        print(json.dumps(state))
        break
    if time.monotonic() > deadline:
        raise SystemExit(f"Daemon did not exit cleanly: {state}")
    time.sleep(0.1)
PY
scp -q "${ssh_options[@]}" "$worker:$remote_root/agent.log" "$output/agent.log"
python3 - "$output/result.json" <<'PY'
import json, pathlib, sys
result = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert result["status"] == "passed", result
assert len(result["checks"]) == 7, result
print("Verified all seven daemon fence checks across the two VMs.")
PY
