# Conversational memory design

Owners manage individual and shared memories through the bot. The Manager confirms actual persistence in the conversation. Secrets go through a supported protected entry path. No memory screen remains.

## Current flow

`link/src/durable.rs` defines a single `Document` for each `(BotId, DocumentKind)`. `state/memory.rs` owns Markdown files, validates same-bot task and message provenance, checks revisions, performs synced temporary-file replacement, and rebuilds `memory-index.sqlite3`. `write_document` recognizes one immediately repeated identical replacement, but has no durable operation identity. It cannot safely become an ordinary call inside the settlement SQLite transaction. A crash after rename can leave changed memory without a settled run result.

`state/runs.rs` selects one individual document at admission and freezes its JSON in `runs.memory_snapshot`. Later calls load that column. A null snapshot means no memory, including for legacy runs. The transcript explicitly treats memory as untrusted data. Its current secret check covers the configured provider key.

`runs/tools.rs` owns provider definitions, decoding and reverse serialization. `provider.rs::PreparedChat` derives tool availability from definitions. `state/runs/settlement.rs::finish_run_call` checks cancellation, deadlines, duplicate call identifiers and operation budgets before executing `request_wakeup`. The wakeup and `ManagerToolResult` commit in one SQLite transaction. Memory must preserve those gates while accounting for filesystem persistence.

`PairedWorkspace.tsx` mounts `Memory.tsx`. It also gates conversation submission on a ready worker and confirmed session. `SubmitRun.session` is mandatory, admission loads it, and `validate_policy` rejects an empty execution policy. These are real blockers for memory-only conversations.

## Alternatives

| Design | Benefits | Costs |
| --- | --- | --- |
| Extend the existing single Markdown document with an individual/shared scope. List returns one document; writes replace its complete body under a revision. | Lowest storage and migration cost. Retains the established file format and size limits. | Each update rewrites unrelated memories. Creating or deleting one fact requires model-generated document editing. Shared writers contend over every fact. Mixing shared scope into `Document` also affects owner instructions. |
| Separate typed memory entries, each backed by a Markdown file, with individual/shared ownership. | CRUD identifies one fact, preserves unrelated facts and permits independent revisions. Instructions stay bot-owned. Scope and provenance are explicit. | Introduces entry identifiers, import, bounded listing and a filesystem settlement journal. |

Recommend separate entries. It adds one domain rather than making instructions optionally global or delegating storage surgery to the model. The cross-store journal is required in either alternative while Markdown remains authoritative. Making SQLite authoritative would simplify transactions but contradict `DESIGN.md:124` and `PLAN.md:30`; do not silently make that trade.

## Public types and caller usage

Place domain types in `link/src/memory.rs`; keep storage implementation in Manager `state/memory.rs`. Illustrative signatures follow.

```rust
enum MemoryScope { Individual, Shared }
enum MemoryOwner { Individual { bot: BotId }, Shared }
struct MemoryEntry {
    id: MemoryId,
    owner: MemoryOwner,
    revision: MemoryRevision,
    markdown: MemoryText,
    source: MemorySource,
}
enum MemorySource {
    Run { bot: BotId, run: RunId, turn: RunTurnId },
    Imported { bot: BotId, sources: Vec<SourceReference> },
}
enum MemoryRequest {
    List { scope: MemoryScope },
    Create { scope: MemoryScope, markdown: MemoryText },
    Update { scope: MemoryScope, id: MemoryId,
             expected_revision: MemoryRevision, markdown: MemoryText },
    Delete { scope: MemoryScope, id: MemoryId,
             expected_revision: MemoryRevision },
}
enum MemoryOutcome {
    Listed { scope: MemoryScope, entries: Vec<MemoryEntry> },
    Committed { change: MemoryChange, entry: MemoryEntry },
    Rejected { reason: MemoryRejection },
}
enum VaultEntryPurpose { MistralProviderKey, Login }
enum VaultHandoffOutcome { LocalProviderSetupRequired, LoginItemsUnsupported }
```

Use checked newtypes for nonempty bounded text and positive revisions. Reject unknown fields. Delete reports the prior entry and a deletion outcome, never an invented live revision. Entry identifiers are Manager-generated. The provider cannot choose an individual bot identifier. Resolve `Individual` from the admitted run's bot; match both scope and identifier on every read and mutation. Shared provenance still records the originating bot and run.

Expose `memory` and `request_vault_entry` as real tools, present in provider schemas and decoding. The Vault request contains a purpose only, never a value field. One request counts as one operation. Tool definitions explain that shared writes require an explicit sharing request and memory cannot confer authority.

The model calls `List`, receives identifiers and revisions, then calls `Update` or `Delete` with a selected revision. On conflict it lists again and explains the conflict; it cannot silently overwrite. Create produces one entry. List is bounded by a fixed per-scope count and aggregate byte limit so it never silently omits entries. The same limits bound snapshots and provider context.

The settlement caller uses one domain operation, conceptually `settle_memory_call(run, turn, attempt, call) -> Result<(), OperationError>`. Memory owns validation, scope resolution, durable publication and receipts. Do not expose separate public validate/write/index methods that callers must coordinate. Private helpers may compose with settlement's transaction.

## Durable publication and replay

Markdown remains authoritative. Each entry has a private file with typed metadata, scope, revision, provenance and text. Use a namespaced directory so the old `memory-*.md` reconciler cannot misread entries. Deleted entries become small tombstones retaining identifier, scope, final revision and operation identity. A tombstone cannot enter model context or list output.

A new SQLite `memory_operations` journal records a unique `(run, turn, attempt)` operation, canonical proposal digest, expected prior digest and complete intended publication. Its record includes the already-billed response and accounting needed to finish settlement. The journal is recovery intent, not the source of live memory.

1. Under the Manager's serialized state access, validate cancellation, deadline, duplicate provider call identifier, limits, scope, revisions and secret checks. Prepare the deterministic entry bytes or tombstone. Atomically persist the journal intent and billed turn accounting. Mark the turn as awaiting memory publication, so provider recovery cannot resend it.
2. Publish with a private temporary file, `sync_all`, rename and directory sync. Include the operation identity in metadata. Do not serve memory reads or admit new runs across an unfinished publication.
3. In one SQLite transaction, record the publication digest/event, committed typed outcome, final turn and run state, and completed journal status. Only then expose the success notice. Rebuild the index as a cache; an index failure cannot convert an already durable write into a second mutation.

Startup drains incomplete journal operations before accepting requests. If the file matches the intended digest, finish settlement without rewriting. If it matches the expected prior state, finish publishing and settle. Otherwise stop that operation with an explicit uncertain/conflict outcome; never overwrite unexpected content. Repeated settlement of the same identity and digest returns its recorded outcome. Different arguments under the same identity are rejected. Same call identifiers in distinct turns still hit the existing duplicate check.

The intent transaction is the mutation's admission boundary. Cancellation or deadline expiry before it prevents the write. A cancellation arriving after it does not undo a committed mutation; recover publication, expose its committed notice, then stop subsequent work. This rule avoids lying about a write merely because the response was cancelled.

## Migration and snapshots

On startup, validate existing documents using the old reader. Import each individual Memory document as one entry, preserving its body, revision and source references. Derive the entry identifier deterministically from the legacy bot/document identity. Commit a migration marker only after the new file and directory are synced. Archive the legacy source outside active scan paths. Replay verifies identical content instead of importing again. Instructions stay in their existing format. Empty legacy Memory documents import as no active entry. Never promote an imported individual entry to shared.

Store new snapshots as a versioned object containing both applicable scopes. Add an explicit decoder for the previous single `Document` shape. A legacy stored object becomes an individual frozen entry in memory only; do not rewrite the database snapshot. SQL null remains no context. New empty snapshots are an explicit empty versioned object. Never resolve missing or old snapshots from current memory.

Admission captures a stable snapshot of both scopes after journal recovery. Later model calls use exactly that snapshot plus committed tool results. A same-run list may deliberately inspect current memory, but does not mutate the frozen snapshot. Deleting an entry removes it from future snapshots; prior admitted snapshots and conversation history remain. Historical erasure is separate scope.

## Secret handling and desktop

Reject known stored secret values at ingress, mutation preparation, legacy import and provider-transcript construction. Audit the actual stored secret inventory rather than calling the provider-key checker sufficient for every credential. Never echo rejected candidate text or put it into a failure receipt. The Vault tool cannot accept arbitrary text fields in which a value could be smuggled.

A secret-storage request produces a structured handoff outcome. Mistral provider keys route to the existing protected local administration flow. Paired clients explain that local setup is required if they cannot open that surface. Login-item storage explicitly reports unsupported until P06 exists. It must never report that a password was saved or solicit one in ordinary chat. No general detector can guarantee arbitrary unknown strings are secrets; test known values and explicit secret requests, and state this implementation boundary honestly.

Remove the Memory mount, component and active create/write-memory desktop commands after conversational tools pass. Preserve historical proof documents. Render committed memory outcomes directly in the conversation with saved/deleted text and scope, independent of a final model response. Render failures and Vault handoffs from typed outcomes. These are conversation notices, not a new memory viewer. Reject stale pending native memory mutations explicitly during journal recovery instead of replaying an obsolete write path.

Permit an execution policy with no worker tools. Make `SubmitRun.session` optional, validating it when worker tools require it. Update desktop admission so manager-only conversations need verified model configuration and budgets but no worker. Never supply computer/process permissions implicitly. Existing worker tool execution retains its exact-action approvals.

## Verification

Exercise provider schemas, strict parsing, actual Manager settlement and serialized desktop results. Cover both scopes and each CRUD operation; same-bot isolation; shared reads from two bots; revision conflicts; malformed and duplicate calls; limits and cancellation; replay with changed arguments; crashes before intent, before rename, after rename and before receipt commit; deleted entries after restart; cache deletion and rebuild; idempotent migration; unchanged old and null snapshots; and manager-only runs without a worker.

Use synthetic stored secrets in ingress, file import, list responses and mutations. Inspect captured provider bodies, durable files, notices and failure records. Verify honest local provider setup and unsupported login handoffs. Run schema/binding checks, Rust checks, desktop/native tests and recovery scripts. Record the real conversation path and notices on video. If packaged execution is unavailable, label a recording of real components driven by recorded-provider Manager flows accurately.

The module boundaries avoid the design red flags. There is one authoritative writer, one tool mutation path, one typed outcome, and one snapshot decoder. Filesystem recovery remains hidden behind the memory domain instead of leaking into desktop or provider code.
