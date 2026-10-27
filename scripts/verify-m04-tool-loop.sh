#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo test -p botstral-manager --lib --locked m04_run -- --nocapture
cargo test -p botstral-manager --lib --locked state::runs -- --nocapture
npm run check
npm test
cargo build -p botstral-manager --locked
cargo test --manifest-path desktop/src-tauri/Cargo.toml --no-default-features --locked --test local -- --ignored --test-threads=1 --nocapture
if [ "${BOTSTRAL_M04_PRODUCTION_WORKER:-0}" = 1 ]; then
    cargo build -p botstral-agent --locked
    cargo test -p botstral-manager --lib --locked m04_run_production_worker -- --ignored --nocapture
fi
