#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
umask 077
computer_evidence=${1:?Provide an evidence directory.}
mkdir -p "$computer_evidence"
computer_evidence=$(cd "$computer_evidence" && pwd)
cargo build --workspace --locked
BOTSTRAL_PROCESS_EVIDENCE="$computer_evidence" \
cargo test -p botstral-agent --test m02_transport --locked w01_artifact_ -- --nocapture
BOTSTRAL_PROCESS_EVIDENCE="$computer_evidence" \
cargo test -p botstral-agent --test m02_transport --locked w01_manager_store_ -- --nocapture
cargo test -p botstral-agent --lib --locked spool_capacity -- --nocapture
cargo test -p botstral-link --test computer --locked -- --nocapture
npm run bindings:check
printf '%s\n' 'Run agent/tests/w01_computer_two_vm.py with fresh fixture roots and certificates for actual Linux browser acceptance.'
