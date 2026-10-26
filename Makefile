.PHONY: dev build verify

dev:
	cargo run --locked -p botstral-manager -- --state-dir .state

build:
	cargo build --workspace --release --locked

verify:
	./scripts/verify.sh
