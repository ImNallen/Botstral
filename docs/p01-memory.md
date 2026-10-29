# Memory snapshots for later runs

A newly admitted run receives its bot's current Memory document. Correcting that document and restarting the manager delivers the corrected preference to a new run. The original `SubmitRun` and its prompt remain unchanged.

This is one bounded P01 outcome. The full P01 checklist remains open.

## Admission and durability

New owner and routine admissions reconcile the authoritative Markdown files through the existing document validator. Admission selects only the run's bot and `DocumentKind::Memory`. Instructions and other bots' Memory do not enter the run. Existing validation still rejects malformed documents anywhere in the authoritative directory. Source ownership, the 4096-byte Markdown bound, and the eight-source bound remain unchanged.

A private `PreparedRun` pairs the public `RunView` with an optional full `Document`. Migration 11 stores that document in nullable `runs.memory_snapshot`, in the same row and transaction as admission. The immutable-run trigger protects it. Recovery tooling supports manager schema 11. Worker schema support remains 2.

An admitted run retains its document through correction, clearing, tool results, retries, and restart. An identical owner submission returns the original run before reconciliation. SQL NULL means no context, including migrated legacy runs. Dispatch never loads current Memory for such a run. A selected document with blank Markdown remains in the snapshot as history and emits no context.

Routine occurrences insert the same prepared package with their reservations. Routine test commands and event batches prepare before their outer transaction because document reconciliation owns a transaction. Event insertion rechecks shared budget and capacity as earlier runs in the batch consume them. Occurrences, run snapshots, reservations, and event receipts still commit atomically. Wakeup planning discards its provisional package. The due occurrence captures the Memory current at its actual admission.

## Provider input and limits

Each turn starts with a separate User context message containing a fixed untrusted-data preface and the serialized full Document. It includes the bot, revision, sources, and Markdown. The unchanged original User or UserMultimodal prompt follows it. Existing `RunTurn.messages` and stored provider requests expose that provenance after a turn is admitted. A ready run with no turns does not expose the private snapshot through `RunView`.

The preface says that the current request takes precedence and memory grants no permissions. It does not prevent model prompt injection. Manager execution policy, exact approvals, and existing dispatch checks enforce permissions. Memory cannot grant a tool or alter a budget. The manager does not fetch source URIs or recheck frozen source references against later-deleted conversation history during dispatch.

Admission bounds the complete serialized initial transcript to 32 KiB and the prepared provider request to 64 KiB. Provider-secret reflection checks include the document before JSON rendering and the complete prepared request. This also catches keys containing quotes or backslashes. The shared transcript renderer repeats the document check at claim, including after key rotation and re-verification. Existing image, token, output, operation, time, cost, parent, and task limits remain in force. The existing dispatch checks still apply to subsequent turns.

## Verification

`manager/src/manager/inference/memory_tests.rs` drives the real manager HTTP API and a local captured HTTP provider. It writes message-sourced preference A, corrects it to B, restarts, creates a new conversation and run, and compares the received provider request with the persisted request and turn messages. The fixture derives its reply from the document it actually receives. Assertions exclude A, same-bot Instructions, and other-bot Memory and preserve the original prompt. Other tests reject reflected provider secrets and oversized context before any provider attempt.

State tests cover frozen admitted runs, same-ID retry, clearing, migration from schema 10, the immutable database guard, external file corrections, malformed and oversized authority, removed source history, tool-result followup after restart, multimodal prompts, complete provider-request bounds, routine rollback, event-batch budgets, and wakeup admission timing. Packaging tests restore schema 11 snapshots and their immutable guard.

Run the focused checks with:

```sh
CARGO_TARGET_DIR="$PWD/.audit/p01-memory/target" cargo test -p botstral-manager --locked --lib p01_
python3 -m unittest discover -s manager/packaging -p 'test_recovery.py' -v
```

The local provider proves delivery and recorded response behavior. It does not establish stochastic real-model adherence. No paid inference is required. `SubmitRun` has no conversation identifier, so the new conversation is owner history and the run is a separate existing API operation.

The [independent review](evidence/p01-memory/independent-review.md) approved this slice after replaying 15 memory tests, 15 routine tests, and nine recovery tests. Root separately replayed the [15 focused tests](evidence/p01-memory/root-focused.log) and [nine recovery checks](evidence/p01-memory/root-recovery.log). The owner's [full repository verification](evidence/p01-memory/repository-verification.log) passed on the identical implementation tree. The [handoff](evidence/p01-memory/handoff.txt) and [proof commands](evidence/p01-memory/proof-commands.tsv) distinguish broad verification from focused replay.

Both escaping regressions failed before correction. Their [raw-document](evidence/p01-memory/escaped-secret-before.log) and [rotated-key](evidence/p01-memory/rotated-secret-before.log) failures remain preserved. The [decision trail](p01-memory-decisions.tsv) records the transaction and reflection choices. [Artifact hashes](evidence/p01-memory/manifest.json) preserve these records.

The [final trail audit](evidence/p01-memory/final-audit.md) found no false success or material omitted pivot. Its pointer-resolution finding is corrected by an appended trail row and the [public artifact index](evidence/p01-memory/artifact-index.json). Historical pointer text stays intact; each alias names a byte-identical published copy.

This slice retrieves the single current bounded document. It adds no conversation-history retrieval, ranking, FTS5, semantic search, bot management, templates, UI, helper model, or release work. The prior helper acceptance evidence remains unchanged.
