# Conversational memory acceptance

The feature is integrated against accepted base `41aec24ae8b62b7cb35874cdd4feb3760425e198`. Verified runtime source is `bace80bed72fe5aca55e9f258ab68827178bb1d2`. The parent corrected independent review findings, reran all local gates, and regenerated the captured fixtures and recording. [Independent source review](../../../.audit/conversational-memory/release-review.md) approved that source. Its pending-verification condition is satisfied by the completed results below.

The original implementation used one isolated writer. The parent integrated it into `t3/bot-managed-memory` and owns the subsequent corrections and evidence. No deployment, merge, paid inference, or request for user credentials was used.

## Results

| Command | Actual result | Evidence |
| --- | --- | --- |
| `./scripts/verify.sh` | Exit 0. Workspace formatting, strict Clippy, builds, Python recovery and acceptance unit tests, Rust tests, test-feature checks, production release builds, and both test-feature release refusals passed. Manager library: 137 passed, 5 existing environment-specific tests ignored. Worker transport: 8 passed and 6 ignored by default; 21 passed, 7 ignored, 1 intentionally filtered with test features. | [Full repository log](logs/repository.log) |
| `./scripts/verify-desktop.sh` | Exit 0. Generated bindings, TypeScript, formatting, frontend tests and build, native Clippy/tests, real local-manager integration, all nine native journal scenarios, browser-cleanup checks, and debug Tauri build passed. Frontend: 32 passed, 1 existing opt-in test skipped. | [Full desktop log](logs/desktop.log) |
| `python3 scripts/verify-conversational-memory.py --full --playwright-module /tmp/botstral-conversational-proof/node_modules/playwright/index.mjs` with `BOTSTRAL_CHROME` set | Exit 0. Memory suite: 26 passed. Connector secret inventory: 1 passed. Native memory scenario: 1 passed. Fixture types, frontend tests, and video generation passed. | [Exact commands and exit codes](commands.json), [memory log](logs/memory.log), [native log](logs/native-memory.log) |
| `npx tsc -p desktop/fixtures/tsconfig.p03.json` and `tsconfig.m06.json` | Both exit 0. Existing active proof fixtures compile after removing obsolete memory methods. | [Additional command results](repository-commands.json) |

Logs retain command output with trailing blank lines removed for repository whitespace checks. [Verification boundaries](verification-boundaries.md) names every default ignored test, the separately invoked native scenarios, the frontend opt-in skip, and the transport filter.

The final full run includes the complete public-view guard, configuration and archived-call rotation regressions, repair after publication attention, and routine overlap protection. The manager library passes 137 tests with five environment-dependent cases ignored. The focused 26-test suite includes the process-crash cases. Native verification includes removal of refused literal and escaped secret-bearing submissions from the mutation journal.

## Coverage

| Contract | Passing proof |
| --- | --- |
| Both scopes and all CRUD; one-fact preservation; actual bot/run/turn provenance; shared access from two bots; guessed foreign entry IDs; revision conflicts | Real HTTP provider definitions and parsing through manager settlement in `manager/inference/memory_tests.rs`; captured [manager conversations and provider requests](manager-conversations.json). |
| Malformed tools, unknown fields, model-provided provenance, duplicate calls and callbacks, disabled process/computer tools | HTTP parsing and state settlement tests. Invalid proposals create no entry or worker task. |
| Manager-only conversation without worker/session/permission | HTTP fixture has no worker. Native Send uses no session and no worker policy. Component test removes workers, sessions, and permissions before sending. |
| Budget, operation, turn, token, cost, deadline, and cancellation limits | State tests refuse publication before intent admission. A separate restart test cancels after intent admission and retains the committed receipt. |
| Publication crashes | Actual child-process aborts before intent, after intent commit, before rename, after rename before directory sync, before final receipt, and after receipt. Update and delete recover at all publication boundaries with one revision and receipt. An unexpected digest requires attention without overwrite or provider resend. Restoring expected or intended bytes resumes the same operation without duplicate events or receipts. |
| Migration and snapshots | Migration 11 to 12 retains old frozen Documents and null context. New snapshots include explicit empty arrays and both scopes. Immutable triggers refuse changes. Imported sources survive history removal. Import crashes cover intent, publication, and archival. Blank documents create no live entry. |
| Backup and restore | Private offline backup restores live entries, tombstones, archives, and an unfinished publication. Schema 11 compatibility and schema 12 immutable snapshots/journal payloads pass recovery tests. |
| Secrets | Literal, nested JSON, quoted, and Unicode-escaped provider values; remote bearer and local environment connector values; retained rotated connector epochs; newly sensitive frozen context, dispatch, complete public run views, configuration and archived call identifiers; rejected response identifiers; import refusal; native rejected-submit journal cleanup. |
| Vault | Closed value-free enum, unknown/value fields rejected, actual protected provider-key path named, reusable login items truthfully unsupported. Typed handoff survives transcript compaction. |
| Visible durable truth | Saved text and scope, lists after failed final inference, restart, and no success notice from proposed arguments. Full typed outcomes survive transcript compaction. |
| Editor cutover | Memory component, mount, application/native mutations, obsolete styles/tests/fixtures removed. Stale native memory records remain readable solely for uncertainty and are never replayed. Existing capability files had no memory command entries to remove. |

## Visual and runtime evidence

[Before](before.png) shows actual baseline components. [After](after.png) shows committed notices. [The interaction video](conversation.webm) sends six requests and refreshes after the recorded manager restart. [Native snapshots](native-conversations.json) come from the real paired native service and manager with local recorded HTTP inference. [Recording metadata](component-proof.json) includes its video digest and browser version.

The video is a component replay through the real `PairedWorkspace` and `MemoryNotice`, not packaged Tauri execution. Renderer generation and revision counters are normalized; durable outcomes, scope, and text are unchanged. Native integration uses its synthetic credential store and worker fixture; memory creates no worker task. The captured native workspace contains a fixture worker and session. Separate manager HTTP admission and renderer tests prove conversation with no worker, session, or computer permission. The debug Tauri application was built, but no packaged application session, real OS keychain interaction, Linux/systemd worker execution, or paid provider behavior is claimed. Existing environment-specific production tests remain ignored.

## Boundaries and corrections

Known-secret exclusion uses the current Mistral key and all retained connector credential epochs. It does not classify arbitrary unknown text, arbitrary encodings, or overwritten provider keys. Historical files and backups are not retroactively erased. Reusable login storage remains outside this feature under P06. Entries are bounded; the installation limit of 256 canonical files includes tombstones.

The [design reconciliation](../../../.audit/conversational-memory/reconciliation.md) records implementation choices and parent corrections. Old document-cache and capacity checks now use Instructions because owner Memory writes are obsolete; legacy memory compatibility and conversational backup have dedicated replacement checks. Historical evidence documents were preserved.

## Review corrections and provenance

The parent reproduced the review findings before changing production code. [Request protection](../../../.audit/conversational-memory/parent/protection-before.log) exposed protected content with `left: true`, `right: false`. [Repaired publication](../../../.audit/conversational-memory/parent/recovery-before.log) returned `Err(Conflict)` instead of `Ok(())`. [Configuration](../../../.audit/conversational-memory/parent/configuration-before.log) and [archived-call](../../../.audit/conversational-memory/parent/archived-before.log) checks returned no refusal instead of `Some(InvalidInput)`. The [routine overlap regression](../../../.audit/conversational-memory/parent/overlap-before.log) also failed before the active-phase correction. All pass in the final suites.

The [first parent repository run](../../../.audit/conversational-memory/parent/repository-first.log) exited 101 because four restart tests read full run content while the Vault was locked. Those tests now assert HTTP 409 `vault_locked`, explicitly unlock, and retain their original persisted-content assertions. The complete final repository run exits 0.

The [implementation trail](../../../.audit/conversational-memory/decisions.tsv) remains distinct from the [parent trail](../../../.audit/conversational-memory/parent-decisions.tsv). Integration replaced two earlier ignored parent planning rows with the implementation's tracked file at the same path. The parent records that loss and links the surviving [selected design](../../../.audit/conversational-memory/design/selected.md) and [independent design judgment](../../../.audit/conversational-memory/design/judge.md). The combined history is not claimed to be continuously append-only. [Evidence review](../../../.audit/conversational-memory/evidence-review.md) identifies the original provenance and scope concerns; this record resolves them explicitly.
