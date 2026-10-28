# P03 acceptance evidence

P03 runs scheduled and event-triggered work in the manager while owner clients are closed. The implementation uses ordinary runs, regional inference verification, current permissions, exact approvals, and existing uncertain-effect recovery.

## Behavioral checks

The focused suite contains fourteen `p03_` checks, plus the IANA calendar check. Parent review added the archived owner-run check.

| Requirement | Evidence |
| --- | --- |
| Manager-only execution and reconnect delivery | `p03_manager_loop_executes_without_owner_clients_and_recovers_inbox` signs out the owner, observes one provider call from the autonomous loop, restarts the manager, and recovers the same inbox and run identifiers. |
| Atomic admission and continued execution beyond sixteen completions | `p03_atomic_admission_restart_identity_and_more_than_sixteen_completions` injects a failed occurrence insert, verifies rollback, completes twenty runs, compares cumulative counters with the reservation ledger, checks compact provider receipts, and verifies clock rollback does not admit another run. |
| Restart boundaries and inbox recovery | `p03_sqlite_restart_before_and_after_admission_and_inbox_ack` reopens real SQLite before admission, after admission, after completion, and after completion but before inbox reconciliation. |
| Duplicate event with one external effect | `p03_duplicate_authenticated_event_restart_has_one_external_connector_effect` uses the real connector HTTP adapter, an exact approved task, and a counted MCP fixture write. Repeated authenticated events and SQLite reopen leave the count at one. |
| Publisher authentication and bounds | `p03_routine_controls_and_crash_journal_recovery` drives the real public TLS listener. It checks missing and incorrect credentials, duplicate receipt equality, changed-payload conflict, stale and future timestamps, oversized bodies, paused subscriptions, rate limiting, and revocation. |
| Daily calendar rules | `calendar_gap_fold_and_anchoring` checks the Stockholm spring gap, earlier autumn fold, anchored intervals, missed-date counts across both transitions, and invalid extreme timestamps. |
| Missed work, pause, resume, uncertainty, vault, and sessions | `p03_locked_vault_stale_bindings_budget_and_pause_missed_overlap` checks coalescing, no paused backfill, unresolved overlap, and durable blocked outcomes. |
| Configuration and aggregate budgets | `p03_current_configuration_and_shared_aggregate_reservations_block_admission` checks stale configuration and reservations shared across routines. |
| Reusable nested limits | `p03_reusable_template_refreshes_parent_and_nested_coding_deadlines` admits the same definition at two times and verifies fresh parent and Vibe deadlines. |
| Bounded wakeups and durable local results | `p03_wakeup_atomic_result_replay_and_chain_share_one_budget` injects a failed local-result transaction, replays settlement, and checks that descendants cannot replenish the original grant. |
| Conversation authority and controls | `p03_authenticated_conversation_crud_and_model_text_cannot_manage_routines` drives create, edit, live test, history, pause, resume, and delete. Assistant text and ungranted wakeups cannot obtain owner authority. |
| Deletion and duplication | `p03_owner_commands_replay_tombstone_and_paused_copies` checks paused fresh copies, deletion during active work, blocked new permits, and delivery of the cancellation under the existing worker and session fence. |
| Decisions after acknowledgment | `p03_each_exact_approval_has_a_new_durable_inbox_decision` verifies that acknowledging one task approval does not hide the next approval in the same run. |
| Workspace bounds | `p03_compact_workspace_preview_keeps_full_text_available_and_fits_bound` and `m05_bounds` check marked previews, full-text projection, the existing 80 MiB transfer limit, and the one-GiB recovery budget. |
| Archived result reads | `p03_archived_owner_run_preserves_ordered_text_and_preview` checks ordinary owner results after compaction, retained provider receipts, ordered projection, and Unicode previews. The twenty-routine check also reads the oldest saved response through the owner interface. |
| Publisher binding after edits | `p03_authenticated_event_receipt_conflict_and_owner_conversation` verifies that revoking the original publisher still fences its admitted run after the definition changes to another trigger. |
| Native recovery and existing caches | The native P03 journal case checks replay before send, replay after commit, changed-body refusal, protected legacy full-run cache reads, and restored workspace reconciliation. |

## Application evidence

[The interaction video](evidence/p03/routine-controls.webm) drives the actual `PairedWorkspace` and `Routines` components through budget setup, create, pause, resume, preview, live-test results, history, edit, conversation help, paused duplication, deletion, and inbox acknowledgment.

The renderer consumes [recorded actual-manager native snapshots](evidence/p03/native-controls.json). The video clearly labels this response replay and the synthetic model and worker. It performs no external effects. Local renderer generation and revision counters are normalized across recorded native process restarts. Durable manager state is unchanged.

[The proof report](evidence/p03/component-proof.json) records the browser, actions, limitations, errors, and video checksum. The baseline capture before visible changes is [the existing approval flow](evidence/p03/baseline-before-ui-change.png). `component-before.png` shows the new components before exercising controls. `component-after.png` shows the resulting routine workspace.

The baseline manager library run passed 85 tests with five ignored. The baseline desktop run passed 26 tests with one skipped. Logs remain under `.audit/p03/` in the implementation worktree.

## Recorded verification

Both `scripts/verify.sh` and `scripts/verify-desktop.sh` exited zero against the initial implementation tree recorded in `verification.json`, before independent review. The host used Darwin 27.0.0 arm64, Cargo 1.93.0, Node 26.12.1, and npm 11.20.0.

The workspace script passed formatting, workspace Clippy, builds, tests, packaging checks, connector contracts, nonproduction worker checks, production release compilation, and both required release-feature refusals. The manager library passed 99 tests with five opt-in cases ignored.

The desktop script passed formatting, generated bindings, TypeScript checks, 27 frontend tests with one skipped, frontend build, native Clippy and tests, actual-manager local controls, all seven journal cases, browser cleanup, and the native application build. The OS keychain acceptance case remained opt-in.

Before review, thirteen focused P03 checks and the calendar check passed. The final native recording, fixture TypeScript check, recorded-response contract test, and component driver also exited zero. The video lasts 15.2 seconds and reports no browser errors.

One extra native recording attempt timed out during sign-in while other checks ran concurrently. The complete desktop script and the later standalone recording passed. No unresolved check failure remains.

[The verification record](evidence/p03/verification.json) contains command results, log hashes, environment details, and the video checksum. Full logs remain under `.audit/p03/` for the parent review.

[Parent review](p03-review.md) records the independently reproduced archive-read defect, its correction, and the review verdict. [Parent verification](evidence/p03/parent-verification.json) identifies the corrected code commit and subsequent checks. The original video remains a component interaction proof. It does not demonstrate archived-result rendering.

## Reproduce the checks

Run the focused manager checks and the complete repository scripts.

```sh
cargo test -p botstral-manager --lib p03_
cargo test -p botstral-manager --lib calendar_gap_fold_and_anchoring
./scripts/verify.sh
./scripts/verify-desktop.sh
```

Generate fresh actual-manager native snapshots in a fresh fixture directory.

```sh
BOTSTRAL_P03_VIEWS="$PWD/docs/evidence/p03/native-controls.json" \
  python3 desktop/fixtures/paired_manager.py --verify-journal \
  --journal-case p03_routine_controls_and_crash_journal_recovery \
  --root .audit/p03/fresh-native-fixture
```

Record the component interaction using an installed Playwright module.

```sh
npx tsc --noEmit -p desktop/fixtures/tsconfig.p03.json
node desktop/fixtures/p03-proof.mjs /absolute/path/to/playwright/index.mjs
```

## Verification limits

The manager-loop proof uses a recorded provider and synthetic worker. The counted effect proof uses the real connector adapter with a local HTTP MCP fixture. Production connector and publisher routes retain their TLS requirements; publisher ingress is separately exercised over the real public TLS listener.

State restart fixtures restore a synthetic unlocked identity to isolate admission recovery. Real manager restart locks the vault. Separate checks verify that locked readiness blocks durably and completed inbox entries remain readable after restart.

Linux systemd, pinned Vibe runtime, and isolated multi-VM production acceptance cases remain opt-in and were not available on this macOS host. The native P03 journal fixture uses a memory-backed secret store. It does not claim an OS keychain round trip or OS push delivery while the app is closed.

The guarantee is one admitted ordinary run per stable trigger identity. Existing uncertain-effect reconciliation remains authoritative for providers without idempotency support.
