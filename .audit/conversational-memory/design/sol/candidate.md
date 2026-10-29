# Conversational memory design

The owner manages individual and shared memories through the bot. Each saved fact has its own identity. Updating one fact must preserve unrelated facts. The conversation reports the committed change even when the next inference fails.

## Current flow

`Document` and `WriteDocument` in `link/src/durable.rs` require a bot, a kind, a revision, sources, and Markdown. `manager/src/manager/state/memory.rs` writes one file per bot and kind with fsync and rename. It then reconciles document publications and rebuilds a disposable index. A crash after rename can leave a committed file without a publication event until reconciliation.

`manager/src/manager/state/runs.rs` captures one Memory document at admission in `runs.memory_snapshot`. Follow-up turns load that immutable value. SQL NULL never reloads current memory. `settlement.rs` already commits manager wakeup results with model-turn accounting, while worker tools use separate task admission and approvals.

`PairedWorkspace.tsx` mounts `Memory.tsx`. Its send blocker also requires a ready worker, a confirmed session, execution permission, and a process or browser tool. `SubmitRun.session` is required, and admission always looks it up. Merely adding a memory tool would therefore leave basic memory conversations dependent on worker execution.

## Alternatives

| Design | Benefit | Cost | Verdict |
| --- | --- | --- | --- |
| Add a shared document alongside each bot's existing Memory document. Tools replace or clear a scope document. | Small storage and snapshot change. Existing individual files stay unchanged. | Every update rewrites unrelated memories. Individual fact identity and deletion require parsing Markdown. A second writer would inherit existing filesystem and settlement crash windows. | Reject as the final product shape. It is a smaller implementation of the wrong operation. |
| Add scoped memory entries with identifiers and revisions. Import existing Memory as one legacy entry per bot. | Each update or deletion addresses one fact. Scope and provenance have one owner. The API matches conversation operations. | Requires an import, publication journal, and snapshot format extension. | Recommend. Keep instructions as existing documents and move all active Memory writes to one owner. |

Do not encode shared memory as a fake bot or as a document with a meaningless bot field. Do not create a second authoritative SQLite copy while claiming Markdown is authoritative.

## Caller usage and types

The model calls `memory` with a discriminated action. The current run determines the bot. The model never supplies a foreign bot identifier.

```rust
enum MemoryScopeSelection { Individual, Shared }
enum MemoryScope { Bot { bot: BotId }, Shared }
struct MemoryEntry {
    id: MemoryId,
    scope: MemoryScope,
    revision: u64,
    markdown: String,
    sources: Vec<SourceReference>,
}
enum MemoryProposal {
    List { scope: MemoryScopeSelection },
    Create { scope: MemoryScopeSelection, markdown: String },
    Update {
        scope: MemoryScopeSelection,
        id: MemoryId,
        expected_revision: u64,
        markdown: String,
    },
    Delete {
        scope: MemoryScopeSelection,
        id: MemoryId,
        expected_revision: u64,
    },
}
struct RequestVaultEntry { purpose: String }
enum MemoryOutcome {
    Listed { entries: Vec<MemoryEntry> },
    Created { entry: MemoryEntry },
    Updated { entry: MemoryEntry },
    Deleted { id: MemoryId, scope: MemoryScope, revision: u64 },
    Refused { reason: MemoryRefusal },
}
```

Expose both tools on manager runs. Derive schemas, decode, and encode from these types. Return current identifiers and revisions from List. Create and Update reject empty text and bounded-input violations. Delete addresses one entry. Model-supplied sources are forbidden. Add `SourceReference::Run { id }` and derive the source from the settling run, since `SubmitRun` has no durable conversation-message identifier.

Private manager functions own validation and publication.

```rust
fn prepare_memory_change(
    &mut self,
    actor: &RunView,
    turn: RunTurnId,
    call: &RunToolCall,
    proposal: &MemoryProposal,
) -> Result<PreparedMemoryOperation, OperationError>;
fn commit_memory_intent(tx: &Transaction, operation: PreparedMemoryOperation)
    -> Result<(), OperationError>;
fn reconcile_memory_operations(&mut self) -> Result<(), OperationError>;
fn memory_context(&mut self, bot: BotId) -> Result<Vec<MemoryEntry>, OperationError>;
```

`PreparedMemoryOperation` is private. It contains the stable operation identity, expected prior digest, exact new file bytes or deletion tombstone, and resulting receipt. Other modules cannot write entry files directly. The State actor remains the single writer.

## Storage, import, and snapshots

Use one private Markdown file per entry at the state root, such as `memory-entry-<id>.md`, with bounded structured metadata and Markdown content. Avoid a new subdirectory because `manager/packaging/recovery.py` permits only root files and the existing artifact layout. Parse and validate it at the boundary. Store the scope as a tagged enum. Keep the SQLite search index rebuildable from these files. Enforce a total bound per selected scope as well as the per-entry limit, so admission cannot accumulate unbounded context. Return explicit capacity refusal instead of silent truncation.

Import each existing per-bot Memory document as one legacy entry. Preserve its Markdown, revision, and sources. Assign its MemoryId once through a durable import record. Publish and verify the imported entry before archiving its old filename. Startup finishes an interrupted import idempotently before serving memory reads or writes. Keep instructions on the existing Document path. Reject obsolete owner `WriteDocument` Memory writes after import and remove the paired-memory mutation commands, so only one active API owns memory.

Add migration 12 with a memory-publication journal and nullable `runs.memory_entries_snapshot`. Extend immutable-run protection to the new column. New admissions always store a JSON vector, including `[]`, containing the applicable individual and shared entries. A null new column selects the old frozen `memory_snapshot` path. If both columns are null, there is no context. This distinguishes an old run from a newly admitted empty context without loading present-day memory into either.

Snapshot validation checks unique identifiers, legal revisions, total bounds, and scope applicability. It does not recheck source records that might later disappear. Individual entries cannot name another bot. Shared entries enter runs for every bot. Current requests still take precedence, and memory grants no permissions. Recovery, backup file filters, schema ceilings, generated bindings, and restore tests must include the new storage and migration.

## Atomic mutation and replay

Markdown rename and SQLite commit cannot be one atomic operation. Do not call the existing `write_document` before settling the model response and assume exactly-once behavior.

Use a durable two-stage publication protocol.

1. Validate the current canonical entry, scope, expected revision, size bounds, known-secret exclusions, deadline, cancellation, call identity, and operation budget.
2. In one SQLite transaction, persist the billed provider response, operation reservation, immutable memory intent keyed by run and turn, and a waiting-for-memory phase. Do not publish a success result yet.
3. Publish exact intent bytes through a private temporary file, fsync, rename, and directory fsync. For deletion, atomically move the file out of active memory with a durable tombstone rather than losing revision knowledge.
4. In another transaction, mark the intent applied, append the authoritative event, persist the typed receipt in the turn, and make the run Ready. The receipt now drives the visible success notice.

Recovery drains committed intents before normal run recovery or new memory operations. If the target digest equals the intended result, finalize the same operation. If it equals the expected prior digest, finish publication. If it matches neither, require attention rather than overwrite unrelated changes. A repeated settled provider callback returns the original receipt without another revision. A duplicate call identifier is rejected by existing run rules. Pending intent recovery completes the already admitted mutation; it does not invent a new model decision.

The journal is not an alternative memory source. It is an immutable instruction to finish publication. Retain durable operation receipts needed to reconcile retries. Never rebuild current memory from arbitrary old journal rows after a user has legitimately changed or deleted it.

## Manager-only conversation

Make `SubmitRun.session` optional. An absent session is valid only when no worker-backed tool or image policy is enabled. Memory and Vault handoff are manager tools and require no computer permission. A present session with worker tools retains all current bot, worker, policy, and approval checks. Do not enable a process tool merely to pass existing validation.

Allow a manager-only execution policy and advertise memory and handoff tools on that path. Keep inference verification, provider-key unlock, owner authentication, model budgets, deadlines, and cancellation. `RunTemplate` can keep a required session for existing computer routines in this slice. Update native `SendInput` and desktop send prerequisites so an ordinary conversation can send with no session. Execution settings continue to opt into worker capabilities explicitly.

## Secret handling and Vault handoff

`request_vault_entry` accepts a short purpose, never a secret value. Return an explicit supported outcome. Provider-key setup can point to the actual protected trusted local-administration API flow. There is no established desktop secret-entry form to open. General website login-item storage must say it is unavailable until P06 supplies it. Do not fabricate a reusable credential item or solicit values in chat.

Reject known provider and connector secret values before memory publication, including nested JSON escaping where the current reflection guard supports it. Check list results and frozen context again before provider dispatch after credential rotation. The system guidance says that credentials and other secret-storage requests use the handoff tool instead of memory.

A general free-text secret classifier cannot guarantee that an arbitrary unknown string is a password. Do not claim that a keyword blacklist proves that guarantee. The deterministic boundary is that protected tools cannot carry secret values to the model, known stored values cannot be written or reflected, and memory tools cannot grant execution authority. Test those boundaries and label the remaining model classification limit honestly.

## Desktop behavior

Delete `Memory.tsx`, its application methods, native create/write commands, capability entries, and obsolete editor tests. Replace them with conversation tests. Keep immutable historical operation journals readable where recovery requires them, but expose no new entry point that creates legacy memory writes.

Extend `ManagerToolResult` with a defaulted typed memory receipt, or a defaulted tagged manager outcome that includes memory and Vault handoff. Render a committed create/update/delete receipt inside the run's conversation card. Show the saved text and scope for creation. Render refusals and uncertain publication states honestly. Do not derive a success notice from proposed arguments or rely on the next assistant turn. List content returns as normal conversation content, without a browsable management screen.

## Verification

- Drive a local recorded provider through actual HTTP tool schemas, parse, settlement, and a follow-up assistant turn for list/create/update/delete in both scopes.
- Prove one-entry edits preserve another entry. Prove a bot cannot read or modify another bot's individual entry by guessing its MemoryId. Prove two bots see deliberate shared memory after restart.
- Prove no worker or execution permission is needed for memory-only conversation, while disabled computer and process proposals remain refused.
- Test migration from schema 11, interrupted import, empty imported content, old snapshots, legacy null snapshots, and new empty snapshots. Verify backup and restore.
- Fault after intent commit, after rename, and before receipt commit. Restart and prove one revision, one receipt, and one visible notice. Test conflicts against unexpected external file edits.
- Test duplicate callbacks, duplicate call identifiers, exhausted budgets, cancellation, expired deadlines, malformed tool arguments, known secrets, escaped secrets, credential rotation, and explicit unsupported Vault requests.
- Render the real PairedWorkspace with captured manager views and record the interaction. Assert no memory editor exists and a committed notice remains visible when the next inference fails. Label component replay separately from packaged Tauri execution.
- Run Rust checks for affected crates, generated schema checks, desktop and native tests, formatting checks, recovery checks, and the repository's required verification commands.

Model the Domain drove separate entry identities and scope variants. Separate Before Serializing Shared State kept one manager writer and isolated this design artifact. Prove It Works drove provider dispatch and crash-window evidence instead of a mocked method-call assertion.
