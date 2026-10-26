import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
import shutil
import tempfile

from m02_two_vm import REQUIRED_CHECKS


def load(root, name):
    return json.loads((root / name).read_text())


def assemble(directory, application_commit):
    raw = directory / "raw"
    run = load(raw, "two-vm-results.json")
    assert run["summary"]["status"] == "passed" and run["summary"]["failed"] == 0, run["summary"]
    passed = {check["name"] for check in run["checks"] if check["status"] == "passed"}
    assert not REQUIRED_CHECKS - passed, sorted(REQUIRED_CHECKS - passed)
    manifests = {}
    for name, expected in (
        ("two-vm-runner-manifest.json", {"m02_two_vm.py", "m02_remote.py"}),
        ("probe-runner-manifest.json", {"m02_daemon_probe.py", "m02_daemon_probe.sh"}),
    ):
        manifest = load(raw, name)
        assert set(manifest) == expected, manifest
        for source, digest in manifest.items():
            actual = hashlib.sha256(Path(__file__).with_name(source).read_bytes()).hexdigest()
            assert actual == digest, (source, actual, digest)
        manifests.update(manifest)
    probe = load(raw, "daemon-probe-result.json")
    expected_probe = {
        "actual_worker_identity_across_vms", "current_receipt_applies_before_takeover",
        "delayed_receipt_after_confirmed_takeover", "daemon_rejects_stale_revision",
        "daemon_rejects_stale_generation", "current_owner_input_applied",
        "return_to_bot_does_not_replay_old_producer",
    }
    assert probe["status"] == "passed", probe
    assert {check["name"] for check in probe["checks"] if check["status"] == "passed"} == expected_probe, probe
    shutdown = load(raw, "probe-agent-shutdown.json")
    assert shutdown["Result"] == "success" and shutdown["ExecMainCode"] == "1" and shutdown["ExecMainStatus"] == "0", shutdown
    cleanup, hosts = {}, {}
    for role in ("manager", "worker"):
        observed = load(raw, role + "-observation.json")
        assert observed["fixture_units"] == [], observed["fixture_units"]
        assert not any(address.rsplit(":", 1)[-1] in {"43122", "43123"} for address in observed["listening_tcp"]), observed
        hosts[role] = observed
        cleanup[role] = {"fixture_listener_absent": True, "fixture_units_absent": True}
    configurations = load(raw, "proxmox-configurations.json")
    assert set(configurations) == {"manager", "worker"}, configurations
    for role, config in configurations.items():
        assert config["name"] == hosts[role]["hostname"], config
        assert config["cores"] == hosts[role]["cpus"], config
        assert int(config["memory"]) * 1024 * 0.9 < hosts[role]["memory_kib"] < int(config["memory"]) * 1024, config
        size = re.search(r"(?:^|,)size=(\d+)G(?:,|$)", config["scsi0"])
        assert size and int(size[1]) * 1024**3 == hosts[role]["root_disk_bytes"], config
        assert "bridge=vmbr0" in config["net0"], config
    hashes = {}
    for role in ("manager", "worker"):
        for line in (raw / (role + "-binaries.sha256")).read_text().splitlines():
            digest, path = line.split(maxsplit=1)
            assert re.fullmatch(r"[0-9a-f]{64}", digest), digest
            hashes[path] = digest
    for phase in ("production", "test"):
        identity = next(check["details"] for check in run["checks"]
                        if check["name"] == phase + ".separate_hosts_and_binary_hashes")
        for role in ("manager", "worker"):
            binaries = identity[role]["binaries"]
            assert binaries and all(hashes.get(path) == digest for path, digest in binaries.items()), binaries
    linux = (raw / "linux-verify.log").read_text()
    marker = "Verified default production binaries and both nonproduction release refusals."
    assert marker in linux, "Linux production verification did not finish."
    return {
        "recorded_at_utc": datetime.now(timezone.utc).isoformat(),
        "application_commit": application_commit,
        "runner_sha256": manifests,
        "hosts": hosts,
        "proxmox_resources": load(raw, "proxmox-resources.json"),
        "proxmox_configurations": configurations,
        "two_vm": {"summary": {key: value for key, value in run["summary"].items()
                               if key not in {"output_dir", "retained_roots"}}, "checks": run["checks"]},
        "daemon_probe": probe,
        "daemon_probe_shutdown": shutdown,
        "linux_verification": {"command": "make verify", "rust": "1.93.0", "result": "passed",
                               "production_separation": marker,
                               "test_results": [line for line in linux.splitlines() if line.startswith("test result:")]},
        "cleanup": cleanup,
    }


def check_controls(directory, application_commit):
    assemble(directory, application_commit)

    def change_binary(value):
        identity = next(check["details"] for check in value["checks"]
                        if check["name"] == "production.separate_hosts_and_binary_hashes")
        path = next(iter(identity["manager"]["binaries"]))
        identity["manager"]["binaries"][path] = "0" * 64

    controls = {
        "missing required check": ("two-vm-results.json", lambda value: value["checks"].pop(0)),
        "changed runner hash": ("two-vm-runner-manifest.json", lambda value: value.update({"m02_two_vm.py": "0" * 64})),
        "remaining fixture unit": ("manager-observation.json", lambda value: value.update({"fixture_units": [{"unit": "m02-bad.service"}]})),
        "remaining fixture listener": ("worker-observation.json", lambda value: value["listening_tcp"].append("192.168.1.235:43122")),
        "failed probe shutdown": ("probe-agent-shutdown.json", lambda value: value.update({"ExecMainStatus": "9"})),
        "missing probe check": ("daemon-probe-result.json", lambda value: value["checks"].pop(0)),
        "wrong virtual CPU allocation": ("proxmox-configurations.json", lambda value: value["manager"].update({"cores": 999})),
        "changed application binary hash": ("two-vm-results.json", change_binary),
        "unfinished Linux verification": ("linux-verify.log", lambda value: value.replace("Verified default production binaries and both nonproduction release refusals.", "")),
    }
    for name, (filename, mutate) in controls.items():
        with tempfile.TemporaryDirectory() as directory_name:
            copied = Path(directory_name)
            shutil.copytree(directory / "raw", copied / "raw")
            path = copied / "raw" / filename
            value = json.loads(path.read_text()) if path.suffix == ".json" else path.read_text()
            changed = mutate(value)
            path.write_text(json.dumps(value) if path.suffix == ".json" else changed)
            try:
                assemble(copied, application_commit)
            except AssertionError:
                print("Rejected " + name + ".")
            else:
                raise RuntimeError("Accepted " + name + ".")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Verify saved M02 artifacts and rebuild their acceptance record.")
    parser.add_argument("--evidence-dir", type=Path, required=True)
    parser.add_argument("--application-commit", required=True)
    parser.add_argument("--check-controls", action="store_true")
    settings = parser.parse_args()
    if not re.fullmatch(r"[0-9a-f]{40}", settings.application_commit):
        parser.error("Provide the full application commit SHA.")
    if settings.check_controls:
        check_controls(settings.evidence_dir, settings.application_commit)
    else:
        result = assemble(settings.evidence_dir, settings.application_commit)
        (settings.evidence_dir / "m02-two-vm.json").write_text(json.dumps(result, indent=2) + "\n")
    print("Verified source hashes, required checks, binary hashes, shutdown, and host cleanup.")
