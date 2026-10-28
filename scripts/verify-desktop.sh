#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
npm run format:check
npm run check
npm test
npm run build
cargo fmt --manifest-path desktop/src-tauri/Cargo.toml --check
cargo clippy --manifest-path desktop/src-tauri/Cargo.toml --all-targets --locked -- -D warnings
cargo test --manifest-path desktop/src-tauri/Cargo.toml --locked
cargo build -p botstral-manager --locked
cargo test --manifest-path desktop/src-tauri/Cargo.toml --locked --test local -- --ignored --test-threads=1 --nocapture
python3 desktop/fixtures/paired_manager.py --verify-journal
python3 manager/tests/c02_browser_cleanup.py
npm run desktop -- build --debug --no-bundle
