#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --locked
python3 -m unittest discover -s manager/packaging -p 'test_*.py' -v
python3 agent/tests/m06_connector_contract.py
cargo test --workspace --locked -- --nocapture

features=botstral-agent/nonproduction-test-worker,botstral-manager/nonproduction-test-worker
cargo clippy --workspace --all-targets --features "$features" --locked -- -D warnings
cargo build --workspace --features "$features" --locked
cargo test -p botstral-agent --features nonproduction-test-worker --locked --lib -- --nocapture
cargo test -p botstral-agent --features nonproduction-test-worker --locked --test m02_transport -- --nocapture --skip enrollment_tls_negotiation_and_lifecycle_use_actual_binaries

release_dir=$(mktemp -d "${TMPDIR:-/tmp}/botstral-production.XXXXXX")
trap 'rm -rf "$release_dir"' EXIT HUP INT TERM
cargo build --workspace --release --locked --target-dir "$release_dir"
test ! -e "$release_dir/release/botstral-test-agent"
"$release_dir/release/botstral-manager" --help
"$release_dir/release/botstral-agent" --help
for package in botstral-agent botstral-manager; do
    if cargo build -p "$package" --release --features nonproduction-test-worker --locked --target-dir "$release_dir" > "$release_dir/$package-refusal.log" 2>&1; then
        echo "$package must reject nonproduction-test-worker in release compilation." >&2
        exit 1
    fi
    grep -q 'nonproduction-test-worker is forbidden in release builds' "$release_dir/$package-refusal.log"
done
printf '%s\n' 'Verified default production binaries and both nonproduction release refusals.'
