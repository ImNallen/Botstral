# Candidate design: conversational memory

## Current flow

`Document` and `WriteDocument` in `link/src/durable.rs` require one `BotId` and carry a kind, revision, source references, and Markdown. `manager/state/memory.rs` treats private Markdown files as authoritative. `document_publications` records revision and digest; SQLite search is rebuilt from files. A write validates sources and expected revision, writes and syncs a temporary file, renames it, syncs the directory, then rebuilds the index.

At run admission, `validate_run_admission` currently requires a live bot, a computer session for that bot, and a nonempty execution policy. It snapshots only that bot's Memory document into `runs.memory_snapshot`. `load_run_memory` checks bot and kind. Existing snapshots are immutable context; a legacy null snapshot remains null. Provider tools are generated from the run policy. `finish_run_call` settles manager actions such as `request_wakeup`; worker actions become separate tasks and retain their approval boundary. Provider secret reflection checks guard admission and provider requests/results.

This means memory tools can be manager capabilities with no worker grants, but current admission still blocks a memory-only run: all-empty execution policy is rejected and a computer session is mandatory. The design must explicitly remove those two coupling points. Do not synthesize computer/process policy to pass validation.

## Storage options

| Design | Shape | Advantages | Costs |
|---|---|---|---|
| Scoped documents | Extend `Document` with `MemoryScope::{Bot(BotId), Shared}` and keep Instructions bot-only. Preserve Markdown/revision/source model. | Smallest change to authoritative file store, revision checks, validation, recovery, and publication. Existing memory can migrate directly. | Scope-aware filenames and uniqueness; shared source references need installation-wide validation. Replace the current per-bot snapshot with a bundle. |
| Separate entries | `MemoryEntry { id, scope, text, revision, sources }` in a new SQLite authoritative table, optionally with FTS. | Individual create/update/delete semantics and search are natural; shared scope maps cleanly to one row set. | New authority, transactional migrations, backup/recovery and indexing contract; current file publication pattern no longer applies. Existing Markdown needs parsing/identity decisions. |

Recommend scoped documents. User asks for memory management through conversation, but does not require independent entries. A document scoped to bot or installation preserves the existing durable contract and offers explicit list/create/update/delete through revisioned document writes. It is the smallest safe design. Do not add a parallel entries store or retain desktop mutation APIs after cutover.

## Types and API

Put public wire types and schema in `link`, with generated TypeScript bindings. Keep scope impossible to omit:

```rust
enum MemoryScope { Bot { bot: BotId }, Shared }
struct MemoryDocument { scope: MemoryScope, revision: u64, sources: Vec<SourceReference>, markdown: String }
struct MemoryRead { scope: MemoryScope }
struct MemoryWrite { scope: MemoryScope, expected_revision: u64, operation: MemoryMutation, source: SourceReference }
enum MemoryMutation { Create { markdown: String }, Replace { markdown: String }, Delete }
struct MemoryReceipt { scope: MemoryScope, revision: Option<u64>, digest: Digest, outcome: MemoryWriteOutcome }
enum MemoryWriteOutcome { Created, Updated, Deleted, AlreadyApplied }
```

Use bounded, strict serde schemas (`deny_unknown_fields`), stable operation IDs, maximum body/source sizes, and a capped document count. Keep mutation IDs scoped to the owner installation and persist `(operation_id, request_digest, receipt)` with the mutation. Reuse an ID with the same digest returns the original receipt; reuse with different arguments conflicts. A create is `expected_revision = 0`; replace/delete require the current revision. List is scope-specific and returns one current document or an explicit empty result. Expose manager methods such as:

```rust
fn read_memory(&mut self, authority: OwnerAuthority, scope: MemoryScope) -> Result<Option<MemoryDocument>, OperationError>;
fn write_memory(&mut self, authority: OwnerAuthority, request: MemoryWrite) -> Result<MemoryReceipt, OperationError>;
fn list_memory(&mut self, authority: OwnerAuthority, scope: MemoryScope) -> Result<Vec<MemoryDocument>, OperationError>;
```

Bot-scoped operations validate that the bot belongs to the authenticated installation. Shared scope is deliberate and installation-wide. It is never inferred from wording such as “remember this”; the tool argument must carry the scope. Never accept a bot ID from an untrusted provider tool without checking it against the run's admitted bot and owner.

Register `memory_list`, `memory_create`, `memory_update`, and `memory_delete` in provider tool definitions for every conversational run. Decode them into typed `ToolProposal` variants. The manager reads and applies these calls directly during run settlement, producing a `ManagerToolResult`; they do not create worker tasks, computer grants, process grants, or worker approvals. Keep per-run operation/turn budgets. Reject malformed, oversized, unknown, or duplicate tool calls as a stopped/unsupported proposal with no write. Each write receipt and user-facing reply reports the committed scope and revision only after persistence succeeds. On conflict, return current revision and require the bot to read and ask/resolve; never silently overwrite.

## Atomicity, replay, and cancellation

Store scoped files atomically using the existing private temp-file, fsync, rename, and directory-fsync flow. Serialize mutations under SQLite `BEGIN IMMEDIATE`, checking expected revision and operation receipt before publication. Write the file, sync it, then commit receipt and publication record; reconcile file-to-index state on startup. A crash before rename leaves old state; after rename but before receipt commit, retry detects the exact requested next revision/content and records the same receipt rather than incrementing again. A digest mismatch at that revision is a conflict. Make delete a durable tombstone/receipt transition before removing the file so a retry cannot resurrect it. Index data remains disposable and rebuildable.

Run settlement already checks duplicate tool-call IDs and persists manager result with run state. Extend that transaction to associate each memory operation ID with the run and tool-call ID. Cancellation before a provider call settles no mutation. Once manager settlement commits a memory write, cancellation cannot roll it back; the committed receipt is returned when that settlement is observed/replayed. Restart recovery replays neither the filesystem write nor a manager action with a new identity. Test crashes at each file/DB boundary and tool response replay.

## Snapshot and migration

Introduce `MemorySnapshot { bot: Option<MemoryDocument>, shared: Option<MemoryDocument> }` for newly admitted runs. Prefer adding a new nullable snapshot column to the runs row through the next ordered migration, rather than changing the meaning/shape of legacy `memory_snapshot` in place. Admission reads both scopes and commits the snapshot with the run. Transcript construction and follow-up load only the stored bundle, never current memory. Old snapshots continue to load through the legacy path. Legacy null snapshots stay null and never gain either current scope. Validate bot scope against run bot and shared scope as installation-wide. Bound the combined serialized snapshot and provider transcript.

For storage, retain existing `memory-{bot}.md` as the bot-scoped record. Add one stable `memory-shared.md`. On restart reconciliation accepts only exact known names, validates metadata/scope and sources, and rebuilds the disposable index. Migration does not reinterpret existing bot files as shared. Preserve publication journal conflict checks. If adopting explicit schema tags, use a versioned envelope and a one-time conversion that keeps revision, Markdown, and sources byte-for-byte; migration failure must leave old files authoritative.

## Secret boundary and Vault handoff

Run existing `reflects_secret` checks against memory mutations before persistence and against the resulting provider request. Also reject known stored provider-secret values found in candidate content. Tool descriptions and bot responses should instruct users to record nonsecret facts only. Secret-storage requests route to the supported protected local administration flow; never ask the user to paste a password or key into ordinary chat. Reusable Vault login items are not implemented, so say that a general login item cannot be saved yet. For supported provider-key updates, direct the owner to the protected provider-key entry flow and report that chat did not store the value. Do not claim broader Vault support.

## Desktop and caller behavior

Remove `Memory.tsx` and its mount from `PairedWorkspace.tsx`; remove desktop `create_memory`/`write_memory` contract and native journal handling after conversational tools are wired. Keep memory contents available only through run context and explicit bot replies. A bot reports “saved to this bot” or “saved for all bots” with a committed revision after receipt. Conflict, policy rejection, capacity limit, and Vault handoff produce distinct truthful replies. No desktop memory viewer or editor remains.

## Test plan

- Schema round trips, generated binding parity, malformed/unknown fields, body/source/count bounds, and explicit scopes.
- Both scopes list/create/update/delete; independent revisions; installation boundary and bot isolation; shared memory visible to each bot and persistent after restart.
- Concurrent expected-revision writes yield one commit and one conflict. Same operation ID plus same digest returns one receipt; changed digest conflicts. Duplicate tool-call IDs, provider retries, cancellation before/after settlement, and crashes around rename/receipt/index never double-write or silently overwrite.
- Provider definitions and decode reach manager settlement with no execution tools. Memory-only run admission works without worker execution policy or connected worker session; no computer/process tools are added. Existing worker-call approval tests remain valid.
- New runs include both applicable snapshots. Follow-up uses snapshots only. Existing admitted and legacy null snapshots remain unchanged across mutation and restart.
- Secret reflection rejects candidate values and provider output; secret-storage prompts never solicit values and report the genuine protected handoff or unsupported general Vault capability.
- Remove desktop viewer/editor and mutation journal surface. Verify actual conversation through recorded-provider manager flows and capture an interaction video if packaged execution is unavailable, labeling that limitation.
- Run manager, link schema/binding generation, desktop/native, formatting, and recovery checks. Preserve historical `docs/p01-*.md` evidence and add the accurate feature scope.
