#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
repo="$PWD"
base=31024003a18ffe6bb166c2ea4d62fdc6ac67f5a6
mkdir -p .audit
fixture=$(mktemp -d "$repo/.audit/m03-base.XXXXXX")
trap 'rm -rf "$fixture"' EXIT
export CARGO_TARGET_DIR="$repo/.audit/m03-base-target"
git archive "$base" | tar -x -C "$fixture"
python3 - "$fixture" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1]) / 'agent/tests/m02_transport/sessions.rs'
source = path.read_text()
old = '''    f.receipt(worker, missing.id, |v| {
        v["acknowledgment"] == json!({"type":"rejected","code":"unknown_task"})
    })
    .await;'''
new = '''    let recovered = f.receipt(worker, missing.id, |v| {
        v["acknowledgment"] == json!({"type":"rejected","code":"unknown_task"})
            || v["last_event"] == json!({"type":"completed","applied":3})
    }).await;
    assert_eq!(recovered["last_event"], json!({"type":"completed","applied":3}), "durable recovery after actual daemon restart: {recovered}");'''
assert source.count(old) == 1
path.write_text(source.replace(old, new))
PY
cargo build --manifest-path "$fixture/Cargo.toml" --workspace --locked --features botstral-agent/nonproduction-test-worker,botstral-manager/nonproduction-test-worker > .audit/m03-base-build.log 2>&1
log="$repo/agent/tests/evidence/m03/base-restart-failure.log"
set +e
cargo test --manifest-path "$fixture/Cargo.toml" -p botstral-agent --features nonproduction-test-worker --locked --test m02_transport sessions::confirmed_takeover_two_sessions_cancellation_and_results -- --nocapture > "$log" 2>&1
result=$?
set -e
test "$result" -ne 0
rg -q 'durable recovery after actual daemon restart.*unknown_task' "$log"
printf 'Captured expected recovery failure at base %s. Cargo exited %s.\n' "$base" "$result"
