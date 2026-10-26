# M03 recovery acceptance

The actual manager and worker binaries recover stable task, command, operation and execution identities without replacement effects. The worker ledger is independent of its execution journal. Its invocation count detects repeated adapter calls. Production execution remains unavailable.

## Reproduce

Run these commands from the repository root with Rust 1.93 and dependencies from the lockfiles.

```sh
./scripts/check-m03-baseline.sh
make verify
cargo build --workspace --locked --features botstral-agent/nonproduction-test-worker,botstral-manager/nonproduction-test-worker
M03_EVIDENCE_DIR="$PWD/.audit/m03-evidence" cargo test -p botstral-agent --features nonproduction-test-worker --locked --test m02_transport -- --nocapture --skip enrollment_tls_negotiation_and_lifecycle_use_actual_binaries
npm run check
npm test
npm run build
cargo test --manifest-path desktop/src-tauri/Cargo.toml --no-default-features --locked
```

The recovery tests live in `agent/tests/m03_recovery/mod.rs` and reuse the actual TLS binary fixture. Every process has bounded waits and cleanup. Expected abrupt HTTP disconnects appear as caught helper-task panics in fault tests. They do not fail the tests.

The saved initial pre-change failure is [pre-change-failure.log](../agent/tests/evidence/m03/pre-change-failure.log). The historical restart reproduction is [base-restart-failure.log](../agent/tests/evidence/m03/base-restart-failure.log). `scripts/check-m03-baseline.sh` exports base `3102400` into a temporary source archive under this worktree, changes only the recovery assertion and verifies the expected `unknown_task` failure from actual binaries. At base `3102400`, the recovery request returned HTTP 404 instead of a durable task. The [committed results directory](../agent/tests/evidence/m03/results/) contains fresh synthetic task identities and exact effect counts. No provider result is injected into the manager as recovery proof.

## Requirement evidence

Each test below is selected with `cargo test -p botstral-agent --features nonproduction-test-worker --locked --test m02_transport NAME -- --nocapture` after building both feature binaries.

| M03 requirement | Reproducible evidence |
| --- | --- |
| Conversations and settings persist. | `recovery::durable_task_survives_manager_restart` reads the identical request and settings after manager restart. `recovery::markdown_sources_conversations_and_event_snapshots_recover` preserves 141 conversation messages. |
| Conversation order survives retries and restart. | `recovery::conversation_append_order_survives_restart_and_retries` appends messages with deliberately reversed UUIDs and verifies append order after an identical retry and manager restart. [The failing-before log](../agent/tests/evidence/m03/message-order-before.log) records reversed order. [The passing-after log](../agent/tests/evidence/m03/message-order-after.log) records the fix for [issue 8](https://github.com/ImNallen/Botstral/issues/8). |
| Progress, approvals and outcomes persist. | `recovery::independent_effect_ledger_recovers_every_worker_crash_boundary` reads the same command, consumed approval, progress and outcome after worker and manager restarts. |
| Owner instructions and memory remain authoritative Markdown with valid source references. | `recovery::markdown_sources_conversations_and_event_snapshots_recover` compares source bytes across restart, corrupts and removes only the index, and removes a source without restoring it from the index. |
| Generated memory cannot change owner authority. | The source test rejects the instruction kind on `/v1/generated-memory`. The endpoint has no permission or approval mutation. Policy and grants use separate authenticated typed methods. |
| Explicit task transitions remain monotonic. | `cargo test -p botstral-link --locked durable::tests` enumerates all state and report combinations for terminal and cancellation monotonicity. Real binary tests cover queued, running, approval, attention, completion, failure and cancellation. |
| Task, command and events commit atomically. | `cargo test -p botstral-manager --lib --locked task_outbox_and_event_roll_back_together` forces event insertion failure and checks that no task or command remains. `recovery::manager_claim_and_outbox_crashes_preserve_original_intent` crashes after outbox and claim commit. |
| Worker acceptance and outcomes precede publication. | The worker crash matrix stops after acceptance, admitted permit, independent effect and outcome commits. The original journal incarnation and command survive. |
| Duplicate delivery recovers existing execution. | `fencing::actual_daemon_rejects_stale_frames_and_queued_receipts` repeats an actual daemon delivery after completion, receives its original terminal report identity and observes one ledger invocation. Changed body identity closes the connection without another effect. |
| Manager restart and worker reconnect reconcile. | The worker crash matrix and `sessions::confirmed_takeover_two_sessions_cancellation_and_results` recover original task identifiers through new generations and revisions. |
| Invalid permits remain isolated and renewed approvals progress. | `recovery::permits::invalid_permit_preserves_another_sessions_pending_claim` completes the other session once after an invalid permit. `recovery::permits::renewed_approval_survives_denial_of_older_pending_claim` claims the renewed binding after denial of the old one. |
| Regranting identical authority republishes rejected work. | `recovery::regrants::identical_regrant_is_delivered_after_denial_before_delayed_report` revokes a grant, denies its pending claim and regrants its exact binding before a delayed approval-required report. It requires redelivery and a valid permit without reconnecting. The saved before and after logs reproduce and correct the cache suppression. |
| Unknown external effects require attention. | `effect_committed` with observation disabled leaves one invocation and attention. Reliable observation returns the original outcome with one invocation. `permit_admitted` has zero effects and attention. |
| Journal replacement is detected even without acceptance acknowledgment. | `recovery::journal_replacement_after_unacknowledged_acceptance_never_restarts_work` confirms the manager received no acceptance, replaces the worker journal and observes attention with zero effects. Worker unit tests refuse corrupt and linked journals. |
| Cancellation survives disconnection and preserves effects. | `recovery::offline_cancellation_preserves_effects_until_stopped_confirmation` restarts both peers after the effect, retains cancelling before worker return and records stopped evidence with the original effects. |
| Every approval binding field and one-use identity is checked. | `recovery::every_approval_binding_field_and_single_use_identity_are_checked` varies nine fields, receives no permit for mismatches and refuses reuse after one successful claim. |
| Authority is rechecked immediately before execution. | `recovery::delayed_claim_rejects_revocation_policy_and_expiry` pauses the daemon before its claim and invalidates authority. No invalid case increments the provider ledger. The fencing fixture delays a permit past takeover, withholds another permit until its five-second timeout and verifies zero extra effects. `cargo test -p botstral-link --locked binding_tests` checks that approval authority matches the actual adapter arguments. |
| Report pages, receipts and inventory remain bounded. | `recovery::recovery_pages_and_lost_report_receipts_are_bounded_and_idempotent` uses 24 tasks and 48 reports, loses a committed receipt, reconnects through multiple pages and replays an original report without another event. |
| Actual report pages obey count and byte limits. | `recovery::permits::actual_worker_report_pages_respect_count_and_byte_limits` observes every page from an actual daemon and checks all 48 report sequences. The saved `observed-report-pages.json` contains measured entry and byte counts. |
| Markdown publication repairs index and event crash windows. | `recovery::published_markdown_recovers_each_file_index_event_crash_window` crashes before rename, after rename and after index publication. Retry returns revision one and publication events are not duplicated. |
| Event retention yields consistent snapshots. | The source and replay test crosses the 128-event boundary and reads a snapshot containing all 141 messages. It appends 15 more messages between retained pages, verifies exact cursor continuity, compares all 156 messages after restart and rejects a future cursor. |
| Derived data-page corruption cannot prevent recovery. | `recovery::derived_index_data_corruption_recovers_without_touching_sources` corrupts a cache root page while preserving its schema. Restart rebuilds the index from unchanged Markdown. Malformed Markdown returns an error without rewriting its bytes. |
| Aggregate admission bounds keep snapshots complete. | `recovery::snapshots::aggregate_admission_limits_keep_complete_snapshots_bounded` admits 128 tasks and 256 records of each other bounded collection, rejects overflow and checks the complete encoded snapshot against 32 mebibytes. Message and document bodies use the 4096-byte limit with JSON escaping. |
| Prior migrations, security and production refusal remain intact. | `make verify` includes populated version-one and version-two migrations, future version-four refusal, M01 actual TCP acceptance, M02 actual TLS and private-state checks, default and feature clippy, release binaries and both feature release refusals. |
| Generated bindings remain compatible. | `npm run check` checks generated schema drift and both TypeScript workspaces. Desktop tests and production build pass. |

## Limits

The provider is a debug-only synchronous synthetic adapter with its own durable SQLite ledger. It proves the effect and result crash window, not real provider integration. Synthetic input is one bounded adapter operation. It has no operating-system process group. `NoProcessGroup` stopped evidence proves only that scope. Real process-group termination, browser and shell execution, inference, spending enforcement and operating-system desktop independence remain M04 and W01 work.

The run uses local separate processes and actual mutually authenticated TLS. It does not repeat the historical two-VM deployment acceptance. Historical M02 evidence and version-one remote reproduction scripts remain unchanged. Current Rust fixtures intentionally use protocol version two and the durable task API.

The historical M02 fixture paused one actively progressing synthetic task while another continued. The M03 adapter executes one synchronous operation instead. Current fixtures prove admission fencing and independent pending claims. They do not repeat active input pause and resume. The W02 manual-takeover acceptance remains open for that behavior on real execution.

There is no generic attention-to-retry API, task deletion or journal compaction. Admission stops at 128 retained tasks and 16384 lifetime reports per journal. Evidence and tombstones are retained. Revocation cannot undo a previously admitted operation. Cancellation can record stopped execution while preserving an earlier unknown effect.

The desktop compatibility checks cover TypeScript, frontend build, five headless native transport tests and the separate actual-manager native transport test. They do not exercise a desktop window or native packaging. Independent correctness, comment and decision-log review belong to the parent. No implementation sub-agent review is claimed.

## Recorded results

The implementation owner's `make verify` passed before parent review. It includes formatting, warnings-as-errors clippy with both feature sets, locked workspace tests, twelve feature-enabled actual-binary fixtures, default release builds and both nonproduction release refusals. [verify.log](../agent/tests/evidence/m03/verify.log) preserves that output. The parent fixed the review findings and passed sixteen focused recovery fixtures. [parent-recovery.log](../agent/tests/evidence/m03/parent-recovery.log) preserves those results. The corrected head then passed `make verify`, including eighteen feature-enabled actual-binary fixtures and both release-feature refusals. [parent-verify.log](../agent/tests/evidence/m03/parent-verify.log) preserves the complete corrected-head run.

`npm run check`, `npm test` and `npm run build` passed. The desktop frontend has 17 passing tests. Native transport has five passing ordinary tests and one passing actual-manager test run separately with `--ignored --test-threads=1`. The [desktop check log](../agent/tests/evidence/m03/desktop-check.log), [frontend test log](../agent/tests/evidence/m03/desktop-tests.log), [native test log](../agent/tests/evidence/m03/desktop-native-tests.log) and [actual-manager log](../agent/tests/evidence/m03/desktop-actual.log) preserve those results. `npm run bindings:check` passed without generated drift.

The baseline reproduction script passed by detecting the expected historical Cargo failure. It does not count that intentional failure as a passing recovery implementation.
