# M04 tool-loop acceptance

The committed tests use recorded provider responses through the real private HTTP adapter and authenticated manager handlers. The default worker supplies synthetic process evidence and artifact bytes through the real gateway. A separate production lane passed on the Linux Computer VM with a real browser, file operations, systemd process execution, and durable artifact retrieval. Neither lane uses live provider credentials.

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

State tests cover atomic rollback across usage settlement, intent, task, command, child link, and event persistence. Repeated completion and result reconciliation preserve the original identities. Database reopen tests cover run, call, child-link, result, and final-response commit boundaries. The HTTP test aborts a supervisor after a provider send and checks restart uncertainty without a replacement call. They cover cumulative turn, token, cost, and operation stops; cancellation before claim and publication; revoked, expired, modified, and revised approval authority; configuration and key changes; locked vaults; deadlines; duplicate tool-call IDs; restart uncertainty; and cancellation stop evidence.

Adapter tests cover unsupported batches, unknown tools, malformed or oversized arguments, attempted authority injection, missing or invalid usage, and literal or encoded credential reflection. Valid usage settles without a child. Invalid usage stays reserved. Run tests also saturate shared transport capacity and the State queue, then verify settlement and bounded shutdown. A separate test holds both provider slots while an unrelated child deadline requests stopping. Existing standalone inference tests continue to exercise transport failures, response limits, standard-tier billing, cancellation, and shutdown through the shared transport.

Parent review also found a truncated-answer reporting defect. The added HTTP regression returned `completed` instead of `stopped` before correction. It now verifies `output_limit`, the retained `length` finish reason and partial text, settled usage, no executable child, and unchanged state after restart without a replacement call. The correction is tracked in [issue 20](https://github.com/ImNallen/Botstral/issues/20).

Result tests read real local files to check length, digest, UTF-8, symlink refusal, output ceilings, unavailable output metadata, text limits, non-text metadata, and secret reflection. Retention tests check admission refusal and summary replay bounds. The tests preserve process effect evidence after confirmed stopping.

## Integration checks

The native desktop lane uses the actual manager binary.

```sh
cargo build -p botstral-manager --locked
cargo test --manifest-path desktop/src-tauri/Cargo.toml --no-default-features --locked --test local -- --ignored --test-threads=1 --nocapture
```

Generated bindings and desktop checks use `npm run check`, `npm test`, and `npm run build`. Rust checks use `cargo fmt --all --check`, workspace clippy, and workspace tests. The production lane's exact observed results follow below. Live credentials and live two-VM acceptance are not part of these fixtures.

## Opt-in production worker lane

The ignored `m04_run_production_worker_browser_file_process_and_artifact` test starts the actual production worker with fixture TLS certificates. It uses the real manager worker listener and the recorded provider endpoint. It navigates a local fixture page, writes and reads a temporary shared file outside a coding workspace, runs `/usr/bin/cat` in a second approved child, checks the follow-up result, and downloads the durable file artifact.

Run this lane as a non-root account on Linux x86_64 with a working systemd user manager and a runtime prepared by `agent/runtime/setup.sh`.

```sh
BOTSTRAL_RUN_RUNTIME=/absolute/path/to/runtime BOTSTRAL_M04_PRODUCTION_WORKER=1 scripts/verify-m04-tool-loop.sh
```

The script builds the release worker. `BOTSTRAL_RUN_AGENT_BIN` can select another already built production worker. The test otherwise uses `target/release/botstral-agent`. It creates temporary certificates, local state, and guest files, which it removes after completion. It is a single-host production integration lane, not live two-VM milestone acceptance.

## Observed verification on macOS

The implementation and parent checkouts passed these commands on 2026-10-27. The counts below include the parent's truncation correction.

- `./scripts/verify.sh` passed workspace formatting, clippy, builds, default and test-worker suites, release builds, and both release feature refusals. The manager library suite passed 46 tests and skipped the opt-in production worker test on macOS.
- `./scripts/verify-m04-tool-loop.sh` passed seven recorded HTTP tests, 13 run state tests, generated-schema and TypeScript checks, 17 desktop TypeScript tests, and the actual-manager native desktop integration test.
- `npm run format:check` and `npm run build` passed.
- `cargo fmt --manifest-path desktop/src-tauri/Cargo.toml --check` passed.
- `cargo clippy --manifest-path desktop/src-tauri/Cargo.toml --no-default-features --all-targets --locked -- -D warnings` passed.
- `cargo test --manifest-path desktop/src-tauri/Cargo.toml --no-default-features --locked` passed five native tests. The separately invoked actual-manager test also passed.
- `cargo test -p botstral-agent --test m02_transport --locked w01_unsupported_host -- --ignored --nocapture` passed against the production worker. It verified that this host cannot consume process approval or accept a computer task without the required capabilities.

The final fixture changes were checked again with workspace clippy, the seven recorded HTTP tests, and the Linux production lane. They only change fixture enrollment, diagnostics, and release-worker selection. [Parent review](m04-tool-loop-review.md) records the findings and corrections.

## Observed production Linux acceptance

The parent ran the production lane on `botstral-computer`, the existing Linux x86_64 Computer VM, as its non-root guest account. The VM had Rust 1.93.0, a working systemd user manager, and the sealed runtime at `/home/botstral/w01-runtime-76b03bf1`. The manager and worker used isolated fixtures on that same VM.

The final source was `64dec675bfbc81569f96b472d30a478b5971cbb2`. SHA-256 comparison confirmed all 406 tracked workspace, manager, worker, and protocol files matched the parent checkout. The release worker and manager test binary were built from those files.

```sh
cargo build -p botstral-agent --release --locked
BOTSTRAL_RUN_RUNTIME=/home/botstral/w01-runtime-76b03bf1 \
  cargo test -p botstral-manager --lib --locked \
  m04_run_production_worker_browser_file_process_and_artifact -- --ignored --nocapture
```

The test passed with one test passed and none failed. It verifies no file effect before approval, real Playwright navigation and capture, a shared guest file outside a coding workspace, a second approved `/usr/bin/cat` operation, three model turns, returned process text, and an owner-authenticated artifact download matching the guest file.

Running the previously unobserved lane first exposed a plain-token enrollment fixture, which was corrected to protected JSON. The debug worker then missed helper readiness during runtime checks. The final fixture uses the release worker, matching production execution. Production timeouts were not changed. These corrections are tracked in [issue 21](https://github.com/ImNallen/Botstral/issues/21).

Temporary diagnostic fixture retention was removed before the final run, and the two retained diagnostic directories were cleaned up afterward. Live provider behavior and live two-VM milestone acceptance remain unobserved.
