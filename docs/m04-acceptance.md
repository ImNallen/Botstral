# M04 first-slice acceptance

The first slice implements durable manager-owned single-turn text inference. M04 remains open. No live credentials were supplied or requested.

## Reproducible checks

These commands passed on Rust 1.93.0 on macOS:

```sh
make verify
cargo test --locked -p botstral-manager --lib m04_ -- --nocapture
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
npm run check
npm test
git diff --check
```

`make verify` runs workspace formatting, clippy, build, and tests. It also runs the test-worker build and recovery checks, production release builds, and both nonproduction-feature release refusals. Existing Linux systemd acceptance tests remain explicitly ignored on this host. TypeScript checks preserve complete generated-schema drift checks. Desktop tests passed all 17 cases.

The focused suite passed all ten tests. Parent verification passed `make verify`, TypeScript checks, and all 17 desktop tests. After review corrections, focused acceptance and clippy passed again. Captured focused and full-verification output contained none of the three synthetic inference credentials. HTTP acceptance also scans client responses and the SQLite directory, including WAL files, for its fixture credential.

Bindings were generated through the existing tools:

```sh
cargo run --quiet --locked -p botstral-link --bin export-schema > link/schema/protocol.json
npm ci --ignore-scripts
npm run bindings:generate
npm run bindings:check
```

## Focused coverage

The eight HTTP tests live in `manager/src/manager/inference/tests.rs`. Recorded JSON fixtures live in `manager/tests/fixtures/m04/`. Two State tests live in `manager/src/manager/state/inference/tests.rs`.

| Test suffix after `m04_` | Proven behavior |
| --- | --- |
| `http_success_idempotency_replay_and_restart` | Every inference route requires authentication. Standard-tier success persists output, usage, request identity, reservation, and settlement. Duplicate intent returns the original record; changed intent conflicts. Snapshots and events include configuration and inference state. Results survive restart without another call. |
| `admission_configuration_vault_and_budget_refusals` | Locked vaults, stale or unverified configurations, model mismatch, wrong context, unavailable chat capability, archived models, invalid deadlines, and insufficient token or cost ceilings refuse inference before HTTP. Arbitrary endpoints, latest aliases, and unsupported billing configuration are rejected. Key replacement invalidates verification. |
| `unknown_responses_retain_reservations_without_retry` | Errors, redirects, missing or contradictory usage, priority or unknown service tiers, unsupported usage fields, model mismatch, tool output, malformed content, interrupted HTTP, and deadlines after sending retain full reservations. Output and fixed-length or chunked response bounds apply. Plain and JSON-escaped credential reflections cannot enter durable results. Safe request identifiers remain available. |
| `capacity_wait_expires_without_sending` | A deadline during the local provider-capacity wait produces `not_sent`, releases the reservation, and sends no chat request. Duplicate submission cannot replace the record. |
| `concurrency_cancellation_disconnect_and_shutdown` | Two active calls exhaust manager-wide admission. State operations remain responsive. Cancellation after sending preserves valid late usage and charges it once. Disconnection does not stop dispatch. Shutdown interrupts a stalled call and persists uncertainty within the fixture's three-second bound. |
| `before_sending_cancellation_and_authority_rechecks` | Authenticated State operations cancel unsent admissions and release their reservations. Claim rechecks vault lock, configuration authority, and deadline before HTTP. The tests serialize these transitions in one State job to control the race. Retrieval uses HTTP. |
| `completion_waits_for_state_queue_capacity` | A completed HTTP response survives all 32 pending queue slots being occupied and settles after capacity returns. |
| `crash_marks_sending_uncertain_and_never_replaces_call` | Abrupt supervisor abortion leaves a sending attempt held and an unsent admission intact on disk. Cold reopening marks sending uncertainty and releases the unsent admission without dispatch. Identical submissions cannot replace either record. |
| `ledger_transitions_and_events_commit_together_and_settle_once` | Injected event failures roll back admission, claim, cancellation, and settlement. Wrong or repeated attempt completions cannot charge twice. SQL rejects request and policy mutation. |
| `price_rounding_overflow_and_verification_authority` | Input and output costs round upward independently. Overflow is rejected. Key replacement and vault locking invalidate an in-flight verification's authority. |

## Remaining acceptance

Production accepts only official global, EU, and US HTTPS origins. Tests inject a private loopback HTTP endpoint into the real adapter and use the authenticated manager router. They do not establish live TLS connectivity, account access, actual model availability, privacy terms, invoice prices, regional processing, or provider-side cancellation.

Crash coverage aborts the supervisor and reopens the real SQLite database. A production manager process kill during live HTTP has not been exercised. New inference shutdown coverage uses the runtime and HTTP fixture, rather than a process signal during live inference. Existing binary shutdown tests still pass.

Tool loops, automatic worker operations, Vibe, vision, and live two-VM acceptance remain future work. Daily aggregate spending controls, invoice reconciliation, attention resolution, and ledger deletion are outside this first slice. Independent review identified two corrections tracked in [issue 17](https://github.com/ImNallen/Botstral/issues/17). Both regression tests failed before the fixes and passed afterward. The [final independent recheck](m04-review.md) found no remaining code findings.
