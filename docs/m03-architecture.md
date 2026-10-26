# Durable tasks and recovery

The owner submits `SubmitTask` to `POST /v1/tasks`. Its identifier is the retry identity. An identical request returns the existing task. Changed intent conflicts. `GET /v1/tasks/{id}` returns immutable settings, progress, outcome and approval state. Tasks belong to the stable owner principal, independently of login sessions.

The manager actor owns task changes, command records, approvals, event retention and Markdown publication. Migration 0003 adds these records without replacing M01 or M02 tables. `CommandBody` contains semantic identity. `Delivery` supplies the current generation and session revision. The command row cannot change its body. Legacy task commands are explicitly unsupported. Owner synthetic input retains its existing fenced transport path and cannot start a task.

The worker owns `agent.sqlite3` under its exclusive private directory. Its incarnation is bound at the manager before execution delivery. Acceptance and outcomes commit with ordered report records before transmission. Recovery exchanges report pages of at most 16 entries and 48 kibibytes with a fixed high-water mark. Receipts follow manager commit. Semantic command tombstones and outcomes remain after report payload acknowledgment.

The owner grants an approval through `PUT /v1/tasks/{id}/approval`. The worker claims its exact binding immediately before adapter admission. The manager verifies owner, bot, task, operation, normalized typed work digest, permission revision, expiry, execution identifier, current fences and cancellation. Consumption and its event share one transaction. A permit belongs to the current connection and execution. Lost permits require attention. Revocation committed before the claim prevents admission. Revocation cannot undo an effect admitted by an earlier claim.

Cancellation creates durable intent and keeps the task cancelling until a worker stopped report. Completion during cancellation preserves effects without clearing cancellation. The synthetic adapter has no process group. Its `NoProcessGroup` stop evidence proves only that scoped protocol. Production process-group cancellation remains W01 work.

Markdown files use flat typed filenames. Their first line records kind, bot, revision and source references. Files are authoritative. Writes synchronize a temporary file, rename it and synchronize the directory. Before search or replay, source revisions reconcile into idempotent publication events and the separate derived SQLite index rebuilds. An index never recreates a source file. A retry compares expected revision and exact content.

Task admission is limited to 128 retained tasks. Conversations, messages, documents and explicit permissions each have a 256-record limit. Messages and Markdown bodies have a 4096-byte limit. Source references have count and length bounds. The aggregate limits keep serialized snapshots below 32 mebibytes, including JSON escaping. Worker uncertainty reasons are bounded to 256 bytes and each task retains at most four effect evidence entries. Events retain 128 entries with an independent durable head. Future cursors fail. An expired cursor returns a consistent snapshot and replacement cursor from the single actor operation.

## Selected design and deviations

The selected Astra design and six independent judge gates are the implementation contract. The [selected contract](m03-design-contract.md) and [independent design verdict](m03-design-verdict.md) record the six gates. The manager retains relational aggregates and a transactional command outbox. The worker journals execution evidence separately from manager authority. Full event sourcing and a shared cross-machine database were rejected by the parent design review.

The owner DTO command body has public serialized fields for protocol construction, while stored command bodies are immutable and every duplicate compares the whole body. This differs from a private-field Rust sketch without weakening stored identity.

Legacy task-start shapes are refused instead of translated. New callers use the durable task API. Protocol version two is incompatible with the historical M02 version-one fixtures. Historical evidence remains unchanged.

The debug synthetic provider has its own durable SQLite ledger. Its invocation count detects repeated effects independently of the worker journal. Test effects are synchronous and have no external process group. Synthetic input work is a bounded single adapter operation. It does not prove operating-system input, desktop independence, production provider reconciliation or real shell supervision.

Independent correctness and comment review belongs to the parent. This implementation run does not claim an independent verdict.

Parent review accepted JSON task and approval aggregates because the actor commits consumption, state and events together. It also accepted immutable cancellation delivery for the synchronous synthetic adapter. These choices preserve the selected transaction and identity rules.

Parent review found that rejecting one permit reconciled unrelated pending claims. Reconciliation now targets that execution. A pending claim records its authority separately from the latest delivery, so denial of an older claim preserves a renewed grant.

Index rebuilding discards the validated derived database and its sidecars before creating a fresh index from Markdown. It does not depend on successfully reading damaged cache pages. Malformed source files still fail validation and remain untouched.

Recovery includes a complete journal inventory after report replay. Inventory pages are bounded by the same entry and byte limits, and fresh execution waits for all pages. A consumed execution missing from that inventory requires attention. Terminal outcome reports remain in a separate journal table after transport payload pruning so repeated delivery can return the original report identity.

The lifetime report sequence is capped at 16384 per worker journal. Storage exhaustion fails closed and retains semantic tombstones. This milestone provides no journal compaction or task deletion API. Both machines retain evidence rather than freeing duplicate protection for new work.

Task views preserve prior effect and uncertainty evidence when cancellation later records stopped confirmation. A stopped confirmation does not assert that earlier uncertain effects did not occur.

The manager stores each typed task and approval aggregate as JSON in its task row. It indexes task, bot and worker relationally and stores immutable command rows, cancellation intent, receipts and events separately. This is a smaller storage shape than the proposed separate operation and approval tables. Actor methods retain the same transaction boundary. Independent parent review must assess this implementation choice.

Messages have a durable SQLite append sequence. Snapshots use that sequence rather than UUID order. Identical append retries return the existing message without changing its position.

Cancellation delivery carries the immutable original execution identity plus sticky cancellation intent, instead of a second executable command body. The cancellation identifier is stored transactionally in `cancellations`. A stopped journal tombstone prevents later delivery from restarting that execution.

A claim that receives no permit within five seconds becomes uncertain. A late permit cannot invoke the adapter after that transition. Queued manager deliveries recheck current control before socket publication. External source references must be bounded HTTPS URIs with a nonempty authority and no embedded credentials.

Model the Domain selects one task reducer and explicit journal phases. Type System Discipline separates argument digests, execution identifiers and current delivery fences. Prove It Works selects actual manager and worker processes plus a separate effect ledger for recovery proof.
