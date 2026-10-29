# Durable memory at run admission

## Problem

A corrected per-bot preference must survive restart and reach a new run. An admitted run must retain the document it admitted, including its bot, revision, and sources. The original prompt stays unchanged. Memory supplies untrusted context and cannot grant tool permissions. This slice retrieves the one current bounded Memory document. It does not retrieve conversation history or rank search results.

Observed code supports a small internal solution. `state/runs.rs` has two transcript construction sites, admission and claim. `validate_run_admission` is shared by owner submission, routine occurrence admission, and wakeup preflight. `state/memory.rs::reconcile_documents` already owns filesystem authority, revision integrity, source ownership, and bounds. `link::RunView` rejects unknown fields, so adding a response field has compatibility consequences even with a deserialize default.

## Usage

The owner writes Memory revision 1 through the existing document API, corrects it to revision 2, restarts, and submits the existing SubmitRun request. The response preserves that request exactly. The first RunTurn.messages and captured provider request contain revision 2 and its sources. Revision 1 and other bots' memory do not appear.

Owner admission stays one operation.

```rust
let prepared = self.validate_run_admission(request)?;
let tx = self.connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
insert_run(&tx, owner, &prepared)?;
tx.commit()?;
Ok(prepared.view)
```

Routine occurrence preparation returns the same private package. Its existing transaction stores the occurrence, reservations, and package together.

```rust
let (occurrence, prepared) = self.prepare_occurrence(/* existing inputs */)?;
Self::commit_occurrence(&tx, owner, &routine, &occurrence, prepared.as_ref())?;
```

Claim reads frozen state without consulting current documents.

```rust
let view = self.load_run(id)?;
let snapshot = self.load_run_memory(id)?;
let messages = transcript(self, &view, snapshot.as_ref())?;
```

## Shape

Keep the public link schema unchanged. Add one nullable `memory_snapshot TEXT` column to `runs`. Store serialized `Document` or SQL NULL. NULL always means no memory for this run, including migrated legacy runs. Never interpret NULL as a request to retrieve current memory. Extend the existing immutable_run trigger to prohibit updating this column. Make insert_run name its columns explicitly.

This is snapshot ownership, not a second authoritative memory store. Markdown remains the source for new admissions. The immutable row records historical input for one admitted run. No snapshot map, cache, secondary table, or independent writer is needed.

```rust
// Private to manager state. Construction stays in runs.rs.
pub(super) struct PreparedRun {
    pub(super) view: RunView,
    memory: Option<Document>,
}

impl State {
    // Reconciles authority, captures current same-bot Memory, and checks the
    // complete initial transcript before returning an insertable package.
    pub(super) fn validate_run_admission(
        &mut self,
        request: SubmitRun,
    ) -> Result<PreparedRun, OperationError> {
        todo!()
    }

    // Reads only the admitted snapshot. NULL never triggers retrieval.
    fn load_run_memory(&self, id: RunId) -> Result<Option<Document>, OperationError> {
        todo!()
    }
}

fn transcript(
    state: &State,
    view: &RunView,
    memory: Option<&Document>,
) -> Result<Vec<RunMessage>, OperationError> {
    todo!()
}

pub(super) fn insert_run(
    db: &Connection,
    owner: OwnerId,
    prepared: &PreparedRun,
) -> Result<(), OperationError> {
    todo!()
}
```

The insertable package encodes the association between the admitted view and its memory. This follows Model the Domain. Reusing Document and one existing row follows Laziness Protocol. Both principle leaves were read for this candidate.

Do cheap existing request and bot checks before filesystem reconciliation. Call `reconcile_documents` within shared admission and select only `document.bot == request.bot && document.kind == Memory`. Keep existing whole-directory validation behavior, including rejection caused by malformed authoritative documents. Do not silently fall back to the index or an earlier document.

Serialize the selected Document once for its context message. Prefix that serialized data with fixed text saying it is saved, untrusted memory, that the current request takes precedence, and that it grants no permissions. Put this User message before the existing original User or UserMultimodal message. JSON escaping makes the data boundary inspectable; it is not a security boundary against model prompt injection. Existing manager authorization remains the security boundary.

Preserve a selected empty Document in storage but emit no memory message for an empty Markdown body. A later new run sees no cleared preference. Existing runs retain their prior snapshot. Bot, revision, sources, and Markdown are all present in the provider message when memory is used. Do not rewrite request.prompt or fabricate source conversation history.

Validate the full serialized initial transcript against MAX_RUN_TRANSCRIPT_BYTES and its PreparedChat encoding against MAX_PROVIDER_REQUEST before insertion. Check secret reflection against the complete prepared provider request, including memory. The current admission guard checks only SubmitRun and is insufficient for this new input. Keep the existing provider envelope reflection guard too. Retain document body/source bounds without adding secret APIs or masking rules. Do not enlarge any context, token, cost, operation, or parent limit. Existing claim accounting still governs actual inference. Reuse the existing formatter for all turns so provenance remains stable.

Snapshot load parses the immutable Document and checks its bot/kind against the run and structural bounds. Do not revalidate source existence against live mutable records on every turn. Admission already established provenance; a snapshot must not become different because later source retention changes. The only current-document read is new admission.

## Module map

`manager/migrations/0011.sql` adds the column and immutable trigger coverage. `manager/src/manager/state.rs` applies that migration and updates version tests.

`manager/src/manager/state/runs.rs` owns PreparedRun, capture during shared admission, atomic insertion, snapshot load, and context rendering. Existing load_run continues returning the wire RunView unchanged. There is no reason to thread snapshots through every cancellation, accounting, or tool-result function.

`manager/src/manager/state/routines/reconcile.rs` changes prepared occurrence payloads from RunView to PreparedRun and passes that package to insert_run. `prepare_occurrence` becomes mutable. All its observed callers already operate on mutable State.

`manager/src/manager/state/routines/wakeups.rs::prepare_wakeup` becomes mutable because its shared admission preflight reconciles documents. It discards the prepared package as it already discards the validated RunView. Recording a pending wakeup does not admit its future run. Actual due admission captures current memory then. Its observed settlement caller already uses mutable State.

## Compatibility and recovery

Schema migration leaves legacy rows NULL. Ready legacy runs never acquire new memory after upgrading. Existing idempotent submit_run returns the previously admitted run before reconciliation, so a retry cannot change its snapshot or fail because memory changed afterward. Different requests with the same ID retain the existing conflict behavior.

Snapshot and run insertion share one transaction, also inside routine occurrence transactions. There is no durable intermediate run without its intended snapshot. A restart before first dispatch reads the same snapshot. Existing durable turn messages and encoded provider requests retain their replay behavior. Do not reconstruct a retry request from current Markdown.

No link schema regeneration, TypeScript binding changes, SubmitRun field, System role, or RunView compatibility negotiation is required. Provenance is inspectable through existing turn messages after inference admission. The accepted limitation is that a Ready run with zero turns does not expose its snapshot through the public RunView.

## Alternatives considered

Adding `RunView.memory_snapshot: Option<Document>` makes snapshot inspection available immediately and keeps state together in memory. It also expands the public protocol, generated schemas, bindings, constructors, fixtures, and older-client compatibility surface. Defaults protect new readers of old records, but old deny_unknown_fields readers still reject newly populated responses. This is excessive for a slice whose existing turn messages provide the required inspectability.

Storing snapshots in a separate run_memory table avoids changing existing five-column positional INSERT statements, including fixtures. It creates a second row lifecycle, joins or extra queries, and missing-row semantics that can conceal forgotten insertion paths. An immutable column is simpler and makes the association physically atomic in one row. Inventory every positional runs INSERT and convert it to an explicit column list in the same change. This bounded migration cost is preferable to maintaining a permanent second table. Guard UPDATE of the new column explicitly. A separate DELETE guard is unnecessary for a column because deleting its snapshot means deleting the run itself; retain the existing run lifecycle policy rather than introducing unrelated retention changes.

Retrieving current memory in transcript avoids snapshot storage. It fails the requirement because edits and restart can change an admitted run, and legacy ready runs acquire new behavior. Storing only a revision or source identifier also fails because old document bodies are not retained by this repository's authoritative document design.

Prepending memory to SubmitRun.prompt avoids a new storage field but loses the original request, breaks idempotent comparison, and turns context provenance into caller-controlled text. Reject it.

## Meaningful tests

1. Existing local captured-provider test writes a message-sourced preference, corrects it, restarts State, creates a new conversation and run, and checks the actual captured provider body and durable turn messages for corrected text, exact source identifier and revision, unchanged user prompt, and absence of superseded text. The mock reply should depend on seeing the corrected preference. This proves delivery, not real-model adherence.
2. Admit a run before correction without claiming it. Correct memory and restart. Claim the old run and a fresh run. Their actual messages must show old and new snapshots respectively. Include a same-ID submission retry after correction.
3. Give another bot a unique preference. Its marker and sources must not enter the first bot's run. Include rejection of cross-bot message-source writes through the existing document validator.
4. Clear the document with a new revision. A fresh run gets no memory message; the previously admitted run keeps its prior preference.
5. Migrate a version-10 database containing a ready legacy run. Populate current memory afterward. Claiming that old run adds none. A new run gets current memory.
6. Exercise routine due admission and pending wakeup. Updating memory while a wakeup is pending affects its eventual newly admitted run, while an already admitted routine run remains frozen.
7. Reject oversized or malformed authoritative memory before run insertion or provider capture. Put the fixture's synthetic provider key in a valid memory document and verify admission rejects it without dispatch. Test near-bound serialized context within existing limits. Check that the immutable trigger refuses changing the admitted snapshot.
8. Put a request for an unauthorized tool in memory. Use a deterministic mock proposal and verify existing manager policy refuses it. Memory must not change execution_policy, scopes, or budgets.

## Synthesis decision

Candidate recommendation only. The parent chooses the synthesized design.

## Implementation reconciliation

No production changes were made. The sketch requires all insert_run callers to accept PreparedRun in the same change. No bypass accepting a bare RunView should remain.

## Open facts

The captured-provider fixture is identified in grounding but was not opened by this candidate. Migration test fixtures that assert schema version and positional runs INSERT statements need inventory. These are implementation facts to resolve locally, not requests for user decisions.

## Next implementation step

Add the immutable nullable snapshot column and PreparedRun package, then update both admission paths and the two transcript construction sites together.
