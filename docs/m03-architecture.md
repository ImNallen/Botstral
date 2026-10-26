# Durable tasks and recovery

The owner submits `SubmitTask` to `POST /v1/tasks`. Its identifier is the retry identity. An identical request returns the existing task. Changed intent conflicts. `GET /v1/tasks/{id}` returns immutable settings, progress, outcome and approval state. Tasks belong to the stable owner principal, independently of login sessions.

The manager actor owns task changes, command records, approvals, event retention and Markdown publication. Migration 0003 adds these records without replacing M01 or M02 tables. `CommandBody` contains semantic identity. `Delivery` supplies the current generation and session revision. The command row cannot change its body. Legacy task commands are explicitly unsupported. Owner synthetic input retains its existing fenced transport path and cannot start a task.

The worker owns `agent.sqlite3` under its exclusive private directory. Its incarnation is bound at the manager before execution delivery. Acceptance and outcomes commit with ordered report records before transmission. Recovery exchanges report pages of at most 16 entries and 48 kibibytes with a fixed high-water mark. Receipts follow manager commit. Semantic command tombstones and outcomes remain after report payload acknowledgment.

The owner grants an approval through `PUT /v1/tasks/{id}/approval`. The worker claims its exact binding immediately before adapter admission. The manager verifies owner, bot, task, operation, normalized typed work digest, permission revision, expiry, execution identifier, current fences and cancellation. Consumption and its event share one transaction. A permit belongs to the current connection and execution. Lost permits require attention. Revocation committed before the claim prevents admission. Revocation cannot undo an effect admitted by an earlier claim.

Cancellation creates durable intent and keeps the task cancelling until a worker stopped report. Completion during cancellation preserves effects without clearing cancellation. The synthetic adapter has no process group. Its `NoProcessGroup` stop evidence proves only that scoped protocol. Production process-group cancellation remains W01 work.

Markdown files use flat typed filenames. Their first line records kind, bot, revision and source references. Files are authoritative. Writes synchronize a temporary file, rename it and synchronize the directory. Before search or replay, source revisions reconcile into idempotent publication events and the separate derived SQLite index rebuilds. An index never recreates a source file. A retry compares expected revision and exact content.

Task admission is limited to 128 retained tasks. Conversations, messages, documents and explicit permissions each have a 256-record limit. Messages and Markdown bodies have a 4096-byte limit. Source references have count and length bounds. Snapshots therefore have a finite aggregate bound. Events retain 128 entries with an independent durable head. Future cursors fail. An expired cursor returns a consistent snapshot and replacement cursor from the single actor operation.

## Selected design and deviations

The selected Astra design and six independent judge gates are the implementation contract. The manager retains relational aggregates and a transactional command outbox. The worker journals execution evidence separately from manager authority. Full event sourcing and a shared cross-machine database were rejected by the parent design review.

The owner DTO command body has public serialized fields for protocol construction, while stored command bodies are immutable and every duplicate compares the whole body. This differs from a private-field Rust sketch without weakening stored identity.

Legacy task-start shapes are refused instead of translated. New callers use the durable task API. Protocol version two is incompatible with the historical M02 version-one fixtures. Historical evidence remains unchanged.

The debug synthetic provider has its own durable SQLite ledger. Its invocation count detects repeated effects independently of the worker journal. Test effects are synchronous and have no external process group. Synthetic input work is a bounded single adapter operation. It does not prove operating-system input, desktop independence, production provider reconciliation or real shell supervision.

Independent correctness and comment review belongs to the parent. This implementation run does not claim an independent verdict.
