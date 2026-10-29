# Conversational memory acceptance

The feature is implemented against accepted base `41aec24ae8b62b7cb35874cdd4feb3760425e198`. The implementation head is `8eb43ba65d3360b0bf48e1593e6bb2be4848d96d`. Parent review and integration remain pending.

The assigned checkout is `/Users/nallen/.t3/worktrees/Botstral/t3-0da55b78`, on `t3/conversational-memory`. One writer used this checkout. No subagents, extra checkouts, pushes, pull requests, deployment, or paid inference were used. No user credentials were requested.

## Results

| Command | Actual result | Evidence |
| --- | --- | --- |
| `./scripts/verify.sh` | Exit 0. Workspace formatting, strict Clippy, builds, Python recovery and acceptance unit tests, Rust tests, test-feature checks, production release builds, and both test-feature release refusals passed. Manager library: 134 passed, 5 existing environment-specific tests ignored. Worker transport: 8 passed and 6 ignored by default; 21 passed, 7 ignored, 1 intentionally filtered with test features. | [Full repository log](logs/repository.log) |
| `./scripts/verify-desktop.sh` | Exit 0. Generated bindings, TypeScript, formatting, frontend tests and build, native Clippy/tests, real local-manager integration, all nine native journal scenarios, browser-cleanup checks, and debug Tauri build passed. Frontend: 32 passed, 1 existing opt-in test skipped. | [Full desktop log](logs/desktop.log) |
| `python3 scripts/verify-conversational-memory.py --playwright-module /tmp/botstral-conversational-proof/node_modules/playwright/index.mjs` with `BOTSTRAL_CHROME` set | Exit 0. Memory suite: 23 passed. Connector secret inventory: 1 passed. Native memory scenario: 1 passed. Fixture types, frontend tests, and video generation passed. | [Exact commands and exit codes](commands.json), [memory log](logs/memory.log), [native log](logs/native-memory.log) |
| `npx tsc -p desktop/fixtures/tsconfig.p03.json` and `tsconfig.m06.json` | Both exit 0. Existing active proof fixtures compile after removing obsolete memory methods. | [Additional command results](repository-commands.json) |

Logs retain command output with trailing blank lines removed for repository whitespace checks.

The final focused suite ran after moving the after-rename fault before directory sync. Recovery's intended-digest path completes that sync. Native verification includes removal of refused literal and escaped secret-bearing submissions from the mutation journal.

## Coverage

| Contract | Passing proof |
| --- | --- |
| Both scopes and all CRUD; one-fact preservation; actual bot/run/turn provenance; shared access from two bots; guessed foreign entry IDs; revision conflicts | Real HTTP provider definitions and parsing through manager settlement in `manager/inference/memory_tests.rs`; captured [manager conversations and provider requests](manager-conversations.json). |
| Malformed tools, unknown fields, model-provided provenance, duplicate calls and callbacks, disabled process/computer tools | HTTP parsing and state settlement tests. Invalid proposals create no entry or worker task. |
| Manager-only conversation without worker/session/permission | HTTP fixture has no worker. Native Send uses no session and no worker policy. Component test removes workers, sessions, and permissions before sending. |
| Budget, operation, turn, token, cost, deadline, and cancellation limits | State tests refuse publication before intent admission. A separate restart test cancels after intent admission and retains the committed receipt. |
| Publication crashes | Actual child-process aborts before intent, after intent commit, before rename, after rename before directory sync, before final receipt, and after receipt. Update and delete recover at all publication boundaries with one revision and receipt. An unexpected digest requires attention without overwrite or provider resend. |
| Migration and snapshots | Migration 11 to 12 retains old frozen Documents and null context. New snapshots include explicit empty arrays and both scopes. Immutable triggers refuse changes. Imported sources survive history removal. Import crashes cover intent, publication, and archival. Blank documents create no live entry. |
| Backup and restore | Private offline backup restores live entries, tombstones, archives, and an unfinished publication. Schema 11 compatibility and schema 12 immutable snapshots/journal payloads pass recovery tests. |
| Secrets | Literal, nested JSON, quoted, and Unicode-escaped provider values; remote bearer and local environment connector values; retained rotated connector epochs; newly sensitive frozen context, dispatch and public views; rejected response identifiers; import refusal; native rejected-submit journal cleanup. |
| Vault | Closed value-free enum, unknown/value fields rejected, actual protected provider-key path named, reusable login items truthfully unsupported. Typed handoff survives transcript compaction. |
| Visible durable truth | Saved text and scope, lists after failed final inference, restart, and no success notice from proposed arguments. Full typed outcomes survive transcript compaction. |
| Editor cutover | Memory component, mount, application/native mutations, obsolete styles/tests/fixtures removed. Stale native memory records remain readable solely for uncertainty and are never replayed. Existing capability files had no memory command entries to remove. |

## Visual and runtime evidence

[Before](before.png) shows actual baseline components. [After](after.png) shows committed notices. [The interaction video](conversation.webm) sends six requests and refreshes after the recorded manager restart. [Native snapshots](native-conversations.json) come from the real paired native service and manager with local recorded HTTP inference. [Recording metadata](component-proof.json) includes its video digest and browser version.

The video is a component replay through the real `PairedWorkspace` and `MemoryNotice`, not packaged Tauri execution. Renderer generation and revision counters are normalized; durable outcomes, scope, and text are unchanged. Native integration uses its synthetic credential store and worker fixture; memory creates no worker task. The debug Tauri application was built, but no packaged application session, real OS keychain interaction, Linux/systemd worker execution, or paid provider behavior is claimed. Existing environment-specific production tests remain ignored.

## Boundaries and corrections

Known-secret exclusion uses the current Mistral key and all retained connector credential epochs. It does not classify arbitrary unknown text, arbitrary encodings, or overwritten provider keys. Historical files and backups are not retroactively erased. Reusable login storage remains outside this feature under P06. Entries are bounded; the installation limit of 256 canonical files includes tombstones.

The [design reconciliation](../../../.audit/conversational-memory/reconciliation.md) records implementation choices. Old document-cache and capacity checks now use Instructions because owner Memory writes are obsolete; legacy memory compatibility and conversational backup have dedicated replacement checks. Historical evidence documents were preserved.

Earlier repository runs exited 101 because of obsolete editor fixture calls and one schema-11 assertion; both were fixed. The first evidence run exited 101 after supplying a filename where the capture expected a directory; that runner was corrected. An initial packaging-only run exited 1 before the required manager build. Type checking exited 2 for a nullable workspace capture in a test; it now narrows before the callback. The passing logs above are the final results. The append-only [decision trail](../../../.audit/conversational-memory/decisions.tsv) and local earlier logs retain the development record.
