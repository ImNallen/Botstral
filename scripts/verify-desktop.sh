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
export BOTSTRAL_SIGNIN_TIMING=1
if [ "$(uname)" = Darwin ]; then
  n=1
  while [ "$n" -le 8 ]; do
    echo "SIGNIN_TIMING {\"role\":\"ci\",\"phase\":\"loop\",\"n\":$n}"
    python3 desktop/fixtures/paired_manager.py --verify-journal --journal-case missing_journal_does_not_become_an_empty_paired_profile
    n=$((n + 1))
  done
fi
python3 desktop/fixtures/paired_manager.py --verify-journal
python3 manager/tests/c02_browser_cleanup.py
npm run desktop -- build --debug --no-bundle
