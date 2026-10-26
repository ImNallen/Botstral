.PHONY: dev build verify desktop verify-desktop

dev:
	cargo run --locked -p botstral-manager -- --state-dir .state

build:
	cargo build --workspace --release --locked

verify:
	./scripts/verify.sh

desktop:
	npm run desktop -- dev

verify-desktop:
	./scripts/verify-desktop.sh
