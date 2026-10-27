#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p botstral-manager --lib --locked
python3 -B -m unittest discover -s agent/tests -p test_m04_acceptance.py -v
npm run check

if test -n "${BOTSTRAL_VIBE_BIN:-}"; then
    evidence_dir=${BOTSTRAL_M04_EVIDENCE:-$(mktemp -d "${TMPDIR:-/tmp}/botstral-m04.XXXXXX")}
    mkdir -p "$evidence_dir"
    python3 -B agent/tests/m04_vibe_contract.py --vibe "$BOTSTRAL_VIBE_BIN" \
        --report "$evidence_dir/vibe-contract.json"
fi
if test "${BOTSTRAL_M04_VIBE_PRODUCTION:-0}" = 1; then
    : "${BOTSTRAL_VIBE_RUNTIME:?Set the installed Vibe 2.24.2 directory.}"
    if test -z "${BOTSTRAL_RUN_AGENT_BIN:-}"; then
        cargo build -p botstral-agent --release --locked
        BOTSTRAL_RUN_AGENT_BIN="$(pwd)/target/release/botstral-agent"
        export BOTSTRAL_RUN_AGENT_BIN
    fi
    cargo test -p botstral-manager --lib --locked \
        m04_coding_production_worker_pinned_vibe -- --ignored --nocapture
fi
