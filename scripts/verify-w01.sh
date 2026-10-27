#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
umask 077
python3 -m unittest discover -s agent/tests -p 'test_w01_*.py'
if [[ $(uname -s) != Linux ]]; then
    echo 'W01 process acceptance requires Linux and a systemd 255 user manager.' >&2
    exit 1
fi
process_evidence=${1:?Provide an evidence directory.}
mkdir -p "$process_evidence"
process_evidence=$(cd "$process_evidence" && pwd)
process_target=$(pwd)/target
cargo build --workspace --release --locked 2>&1 | tee "$process_evidence/release-build.log"
BOTSTRAL_MANAGER_BIN="$process_target/release/botstral-manager" \
BOTSTRAL_PROCESS_AGENT_BIN="$process_target/release/botstral-agent" \
BOTSTRAL_PROCESS_EVIDENCE="$process_evidence" \
cargo test -p botstral-agent --locked --test m02_transport w01_ -- \
    --ignored --nocapture --test-threads=1 2>&1 | tee "$process_evidence/production.log"
cargo build --workspace --features botstral-agent/nonproduction-test-worker,botstral-manager/nonproduction-test-worker --locked \
    2>&1 | tee "$process_evidence/debug-build.log"
BOTSTRAL_PROCESS_EVIDENCE="$process_evidence" \
cargo test -p botstral-agent --features nonproduction-test-worker --locked --test m02_transport \
    w01_crash_boundaries -- --ignored --nocapture 2>&1 | tee "$process_evidence/crashes.log"
