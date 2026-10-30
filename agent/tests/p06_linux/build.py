#!/usr/bin/env python3
"""Cross-build the unmodified production binaries using the installed Rust target and Zig."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(mode=0o700)
    zig = shutil.which("zig")
    if not zig:
        raise SystemExit("Install Zig and the x86_64-unknown-linux-gnu Rust target first.")
    for name, mode in (("zigcc", "cc"), ("zigar", "ar")):
        wrapper = output / name
        extra = ["-target", "x86_64-linux-gnu.2.31"] if mode == "cc" else []
        wrapper.write_text("#!/usr/bin/env python3\nimport os,sys\nos.execv(" + repr(zig)
                           + ", [" + repr(zig) + ", " + repr(mode) + "] + " + repr(extra)
                           + " + [a for a in sys.argv[1:] if not a.startswith('--target=')])\n")
        wrapper.chmod(0o700)
    env = {**os.environ, "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER": str(output / "zigcc"),
           "CC_x86_64_unknown_linux_gnu": str(output / "zigcc"),
           "AR_x86_64_unknown_linux_gnu": str(output / "zigar"),
           "CARGO_TARGET_DIR": str(output / "target"), "CARGO_BUILD_JOBS": "4"}
    argv = ["cargo", "build", "--locked", "--release", "--target", "x86_64-unknown-linux-gnu",
            "-p", "botstral-agent", "--bin", "botstral-agent", "--bin", "botstral-agent-supervisor-check",
            "-p", "botstral-manager", "--bin", "botstral-manager"]
    with (output / "build.log").open("wb") as log:
        result = subprocess.run(argv, cwd=Path(__file__).resolve().parents[3], env=env, stdout=log, stderr=log)
    (output / "command.json").write_text(json.dumps({"argv": argv, "exit": result.returncode,
                                                     "target_directory": env["CARGO_TARGET_DIR"]}, indent=2) + "\n")
    print(output / "target/x86_64-unknown-linux-gnu/release")
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
