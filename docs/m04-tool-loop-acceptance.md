# M04 recorded tool-loop acceptance

The committed tests use synthetic provider responses through the real private HTTP adapter and authenticated manager handlers. The default worker in the recorded acceptance test supplies synthetic process evidence and artifact bytes through the real worker gateway. This proves manager orchestration and persistence. It does not prove a browser ran or a Linux systemd scope executed on this macOS host.

## Recorded acceptance

Run `scripts/verify-m04-tool-loop.sh` for the recorded acceptance and consumer checks.

Run `cargo test -p botstral-manager --lib --locked m04_run -- --nocapture` for the HTTP tests. Run `cargo test -p botstral-manager --lib --locked state::runs -- --nocapture` for the state and result-collection tests.

The recorded browser and file chain checks all of these behaviors.

1. A function-capable configuration admits the recorded browser and shared-file proposal.
2. The child is durable and has no approval or execution permit.
3. The owner's exact child approval permits dispatch.
4. A terminal worker report does not advance the model while declared outputs remain pending.
5. Verified artifacts produce a durable untrusted tool result associated with the original provider call ID.
6. The next provider request includes that result and reaches a final response.
7. The owner downloads the artifact, and the completed run survives manager restart without another provider request.

The fixture requests `/srv/shared/m04-note.txt`, outside a coding workspace. The fixture does not execute that guest path. Process translation separately checks the executable, arguments, working directory, and immutable owner environment.

## Failure coverage

State tests cover atomic rollback across usage settlement, intent, task, command, child link, and event persistence. Repeated completion and result reconciliation preserve the original identities. They cover cumulative turn, token, cost, and operation stops; cancellation before claim and publication; revoked, expired, modified, and revised approval authority; configuration and key changes; locked vaults; deadlines; duplicate tool-call IDs; restart uncertainty; and cancellation stop evidence.

Adapter tests cover unsupported batches, unknown tools, malformed or oversized arguments, attempted authority injection, missing or invalid usage, and literal or encoded credential reflection. Valid usage settles without a child. Invalid usage stays reserved. Run tests also saturate shared transport capacity and the State queue, then verify settlement and bounded shutdown. Existing standalone inference tests continue to exercise transport failures, response limits, standard-tier billing, cancellation, and shutdown through the shared transport.

Result tests read real local files to check length, digest, UTF-8, symlink refusal, output ceilings, unavailable output metadata, text limits, non-text metadata, and secret reflection. Retention tests check admission refusal and summary replay bounds. The tests preserve process effect evidence after confirmed stopping.

## Integration checks

The native desktop lane uses the actual manager binary.

```sh
cargo build -p botstral-manager --locked
cargo test --manifest-path desktop/src-tauri/Cargo.toml --no-default-features --locked --test local -- --ignored --test-threads=1 --nocapture
```

Generated bindings and desktop checks use `npm run check`, `npm test`, and `npm run build`. Rust checks use `cargo fmt --all --check`, workspace clippy, and workspace tests. The production worker lane and exact observed results are recorded with the implementation delivery. Live credentials and live two-VM acceptance are not part of this fixture result.

## Opt-in production worker lane

The ignored `m04_run_production_worker_browser_file_process_and_artifact` test starts the actual production worker with fixture TLS certificates. It uses the real manager worker listener and the recorded provider endpoint. It navigates a local fixture page, writes and reads a temporary shared file outside a coding workspace, runs `/usr/bin/cat` in a second approved child, checks the follow-up result, and downloads the durable file artifact.

Run this lane as a non-root account on Linux x86_64 with a working systemd user manager and a runtime prepared by `agent/runtime/setup.sh`.

```sh
BOTSTRAL_RUN_RUNTIME=/absolute/path/to/runtime BOTSTRAL_M04_PRODUCTION_WORKER=1 scripts/verify-m04-tool-loop.sh
```

`BOTSTRAL_RUN_AGENT_BIN` can select an already built production worker. The test otherwise uses `target/debug/botstral-agent`. The test creates temporary certificates, local state, and guest files. It does not use live provider credentials. This lane compiles on the macOS implementation host but is not observed there. It is a single-host production integration lane, not live two-VM milestone acceptance.
