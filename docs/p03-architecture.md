# Durable routine execution

The manager admits scheduled and event-triggered work while clients are closed. `RoutineDefinition` holds an owner-approved reusable `RunTemplate`. `RoutineOccurrence` binds a stable trigger key and definition revision to one ordinary `RunId`.

## Authority and storage

The existing SQLite State actor owns every write. The existing autonomous run loop reconciles bounded batches before claiming ordinary runs. Private shared run validation preserves regional inference verification, vault checks, current configuration, prices, and execution limits. Owner submission authenticates the owner. Routine submission checks the saved definition, pinned session and permission revisions, live bot, and source authority. Compact occurrence bindings pin the publisher identity and revision as well. Editing a definition cannot rebind an admitted run.

One transaction inserts the occurrence, ordinary run, aggregate reservation, and schedule advancement. A missing prerequisite produces a durable blocked occurrence. Templates use relative run durations and relative nested Vibe deadlines. No trigger grants exact-action approval. Existing connector and task reconciliation preserve uncertain effects.

Owner budgets count cumulative settled and reserved tokens, cost, and run count across routines. Admission and settlement update aggregate counters in their transactions. Completed runs release unused reservations. Uncertain runs retain their full reservation. Completed history does not consume the sixteen unresolved-run admission slots. Workspace projections include every unresolved run and eight recent resolved runs. Text previews are bounded and explicitly marked; an authenticated read loads full run text. Task projections retain active work, thirty-two recent terminal tasks, and tasks referenced by projected runs. Unreferenced old terminal transcripts can be compacted after sixteen completions. Compaction keeps compact turn receipts, provider request identifiers, accounting, hashes, and final text. Active, uncertain, referenced, occurrence, and replay evidence remains in SQLite.

## Trigger policies

Daily schedules use IANA zones. A spring gap selects the first valid local minute. An autumn fold selects the earlier instant once. Intervals use UTC anchors. Missed work coalesces to the latest occurrence within a finite catchup window. History records skipped counts and expired work. Resume computes the next future instant without backfill.

Overlap defaults to skip. Approval, uncertainty, and cancellation remain unresolved. Live tests use ordinary run admission, budgets, and approvals. They do not shift the saved schedule. Preview performs no work.

Publisher webhooks support named topics and bounded scalar equality subscriptions. The public TLS listener authenticates a bearer credential against its stored SHA-256 digest. It limits body size, request time, event freshness, source rate, topics, and matching fields. Receipt and matching admissions commit before acknowledgment. Source-scoped event identifiers survive restarts. Changed bytes for an existing identifier conflict. Revocation fences admission and dispatch. Publisher webhook topics are the supported subscription adapter. Event instructions are limited to 2048 bytes. Runs receive a bounded untrusted event envelope; omitted fields are counted explicitly.

## Controls and recovery

Owner protocol version three carries compact workspace runs. Manager and paired clients upgrade together. The native client reads existing version-two cached full runs and projects them locally without dropping credentials or journals. The published database upgrade is schema nine to ten. Migration 0010 is finalized before publication; development databases from intermediate commits are disposable fixtures.

The app and typed owner conversation commands use one idempotent manager mutation boundary. Model text cannot invoke owner commands. A bot wakeup requires an explicit owner grant and shares cumulative owner budgets with its descendants. Its manager-local result commits with the pending occurrence and replay identity.

Bot deletion uses a tombstone, disables routines and publishers, and requests cancellation. Dispatch checks the tombstone. Cancellation delivery retains the worker and session fence after tombstoning. Immutable connector audit remains intact. Duplication creates fresh paused definitions without sessions, grants, secrets, history, or reservations.

The durable inbox exposes outcomes and decision requests after reconnect. Decision identities include the task and exact approval, so acknowledging one request cannot hide a later request in the same run. Transport notifications are refresh hints. Inbox delivery does not imply native push while the app is closed.

## Throughput checkpoint

Shared types, authority, and atomic storage precede clients. One implementation owner changes coupled schemas in its isolated worktree. Storage and admission, event and lifecycle behavior, and app integration form ordered verified commits. The parent supplies independent review.

The controlling design is Sol with the judge-approved Astra grafts for reusable templates, shared atomic admission, manager-local results, and concrete publisher topic subscriptions.
