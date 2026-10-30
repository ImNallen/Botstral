#!/usr/bin/env python3
"""Run the Linux integration fixture in a new, private Docker container."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import time
import uuid


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary-directory", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--image", help="Use an existing fixture image instead of building one.")
    parser.add_argument("--headless-only", action="store_true")
    parser.add_argument("--native-port", type=int, help="Publish only the paired TLS endpoint on host loopback for packaged native acceptance.")
    args = parser.parse_args()
    if args.native_port is not None and not 1024 <= args.native_port <= 65535:
        parser.error("--native-port must be between 1024 and 65535")
    repository = Path(__file__).resolve().parents[3]
    evidence = args.evidence.resolve()
    evidence.mkdir(mode=0o700)
    commands = []
    def run(argv, name, check=True):
        result = subprocess.run(argv, cwd=repository, capture_output=True)
        (evidence / (name + ".log")).write_bytes(result.stdout + result.stderr)
        commands.append({"argv": argv, "exit": result.returncode, "log": name + ".log"})
        (evidence / "commands.json").write_text(json.dumps(commands, indent=2) + "\n")
        print(json.dumps({"command": name, "exit": result.returncode}), flush=True)
        if check and result.returncode:
            raise RuntimeError(name + " exited " + str(result.returncode))
        return result
    slug = uuid.uuid4().hex[:12]
    name = "botstral-p06-" + slug
    image = args.image
    binary_directory = args.binary_directory.resolve()
    binaries = {name: hashlib.sha256((binary_directory / name).read_bytes()).hexdigest()
                for name in ("botstral-agent", "botstral-agent-supervisor-check", "botstral-manager")}
    info = run(["docker", "info", "--format", "{{json .Architecture}}"], "docker-architecture")
    architecture = json.loads(info.stdout)
    (evidence / "source.json").write_text(json.dumps({
        "reviewed_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository).decode().strip(),
        "diff_sha256": hashlib.sha256(subprocess.check_output(["git", "diff", "--binary"], cwd=repository)).hexdigest(),
        "fixture_sha256": hashlib.sha256((repository / "agent/tests/p06_linux/fixture.py").read_bytes()).hexdigest(),
        "binaries": binaries, "headless_only": args.headless_only, "host_architecture": architecture}, indent=2) + "\n")
    if not image:
        sealed = name + "-runtime"
        run(["docker", "build", "--platform", "linux/amd64", "-t", sealed,
             "-f", "agent/tests/p06_linux/Dockerfile", "."], "build-runtime")
        image = sealed
        if architecture == "aarch64":
            image = name + "-mixed"
            run(["docker", "build", "--platform", "linux/arm64", "--build-arg", "SEALED_IMAGE=" + sealed,
                 "-t", image, "-f", "agent/tests/p06_linux/Dockerfile.mixed", "."], "build-mixed")
    started = False
    snapshot_script = (
        "test -e /proc/sys/fs/binfmt_misc/register || mount -t binfmt_misc binfmt_misc /proc/sys/fs/binfmt_misc; "
        "for f in /proc/sys/fs/binfmt_misc/*; do "
        "case $f in */register|*/status) ;; *) printf '%s\\n' \"$f\"; cat \"$f\";; esac; done"
    )
    def binfmt_snapshot(label):
        result = run(["docker", "run", "--rm", "--name", name + "-" + label,
                      "--privileged", "node:26", "sh", "-ec", snapshot_script], label)
        return result.stdout
    before = binfmt_snapshot("binfmt-before")
    try:
        argv = ["docker", "run", "-d", "--name", name, "--privileged", "--cgroupns=private",
                "--tmpfs", "/run", "--tmpfs", "/run/lock"]
        if args.native_port:
            argv += ["--publish", "127.0.0.1:" + str(args.native_port) + ":44319"]
        for unit in ("systemd-binfmt.service", "systemd-sysctl.service", "systemd-modules-load.service"):
            argv += ["--mount", "type=bind,src=/dev/null,dst=/etc/systemd/system/" + unit + ",readonly"]
        run([*argv, image], "start-container")
        started = True
        for binary in binaries:
            run(["docker", "cp", str(binary_directory / binary), name + ":/usr/local/bin/"], "copy-" + binary)
        run(["docker", "cp", "agent/tests/p06_linux/fixture.py", name + ":/home/p06/fixture.py"], "copy-fixture")
        base = ["docker", "exec", "--user", "10001:10001", "-e", "XDG_RUNTIME_DIR=/run/user/10001",
                "-e", "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/10001/bus", name]
        deadline = time.monotonic() + 20
        while True:
            ready = run([*base, "systemctl", "--user", "is-system-running"], "user-systemd", check=False)
            if ready.returncode == 0:
                break
            if time.monotonic() >= deadline:
                raise RuntimeError("user systemd did not start")
            time.sleep(0.2)
        runtime = "/opt/p06-runtime"
        if args.headless_only:
            runtime = "/opt/p06-headless"
            prune = (
                "cp -a /opt/p06-runtime /opt/p06-headless; "
                "rm -rf /opt/p06-headless/browsers/chromium-1248 /opt/p06-headless/browsers/ffmpeg-1013; "
                "python3 /opt/p06-headless/seal.py; chmod -R a+rX /opt/p06-headless"
            )
            run(["docker", "exec", name, "sh", "-ec", prune], "headless-reseal")
        fixture_args = [*base, "python3", "/home/p06/fixture.py", "--root", "/home/p06/fixture", "--runtime", runtime]
        if args.native_port:
            fixture_args += ["--native-port", str(args.native_port)]
        result = run(fixture_args, "integration", check=False)
        run(["docker", "exec", name, "sh", "-c",
             "cat /home/p06/fixture/worker.log /home/p06/fixture/manager.log"], "service-logs", check=False)
        return result.returncode
    finally:
        if started:
            run(["docker", "rm", "-f", name], "remove-owned-container", check=False)
        after = binfmt_snapshot("binfmt-after")
        if before != after:
            raise RuntimeError("Docker emulation registrations changed")


if __name__ == "__main__":
    raise SystemExit(main())
