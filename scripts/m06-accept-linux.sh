#!/bin/sh
set -eu
component=${1:?Provide manager or agent.}
evidence_dir=${2:?Provide a fresh evidence directory.}
case "$component" in manager|agent) ;; *) exit 1;; esac
case "$evidence_dir" in /*) ;; *) echo 'Absolute evidence directory required.' >&2; exit 1;; esac
[ ! -e "$evidence_dir" ] || { echo 'Evidence directory must be new.' >&2; exit 1; }
umask 077
mkdir -m 700 "$evidence_dir"
source_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$source_root"
git rev-parse HEAD > "$evidence_dir/source.txt"
uname -a > "$evidence_dir/kernel.txt"
if [ "$component" = manager ]; then
    cargo build --locked -p botstral-manager --bin botstral-manager > "$evidence_dir/build.log" 2>&1
    cargo test --locked -p botstral-manager offline_restore_reopens -- --nocapture > "$evidence_dir/restore.log" 2>&1
    python3 -m unittest discover -s manager/packaging -p 'test_*.py' -v > "$evidence_dir/package.log" 2>&1
    python3 manager/packaging/install.py --binary "$source_root/target/debug/botstral-manager" --root "$evidence_dir/installed" > "$evidence_dir/install.json"
else
    [ "$(id -u)" != 0 ] || { echo 'Run as the nonroot worker account.' >&2; exit 1; }
    cargo build --locked -p botstral-agent --bins > "$evidence_dir/build.log" 2>&1
    python3 agent/packaging/install.py --binary "$source_root/target/debug/botstral-agent" --root "$evidence_dir/installed" > "$evidence_dir/install.json"
    target/debug/botstral-agent-supervisor-check --scratch "$evidence_dir/supervisor-probe" > "$evidence_dir/supervisor.log" 2>&1
fi
printf 'Acceptance completed. Evidence is in %s.\n' "$evidence_dir"
