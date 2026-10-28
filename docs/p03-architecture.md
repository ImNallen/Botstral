# Durable routine execution

The manager admits scheduled and event-triggered work while clients are closed. `RoutineDefinition` holds an owner-approved reusable `RunTemplate`. `RoutineOccurrence` binds a stable trigger key and definition revision to one ordinary `RunId`.

## Authority and storage

The existing SQLite State actor owns every write. The existing autonomous run loop reconciles bounded batches before claiming ordinary runs. Private shared run validation preserves regional inference verification, vault checks, current configuration, prices, and execution limits. Owner submission authenticates the owner. Routine submission checks the saved definition, pinned session and permission revisions, live bot, and source authority.

One transaction inserts the occurrence, ordinary run, aggregate reservation, and schedule advancement. A missing prerequisite produces a durable blocked occurrence. Templates use relative run durations and relative nested Vibe deadlines. No trigger grants exact-action approval. Existing connector and task reconciliation preserve uncertain effects.

Owner budgets count cumulative settled and reserved tokens, cost, and run count across routines. Completed runs release unused reservations. Uncertain runs retain their full reservation. Completed history does not consume the sixteen unresolved-run admission slots. Full run projections include unresolved evidence and sixteen recent resolved runs. Compact occurrence and replay records remain in SQLite.

## Trigger policies

Daily schedules use IANA zones. A spring gap selects the first valid local minute. An autumn fold selects the earlier instant once. Intervals use UTC anchors. Missed work coalesces to the latest occurrence within a finite catchup window. History records skipped counts and expired work. Resume computes the next future instant without backfill.

Overlap defaults to skip. Approval, uncertainty, and cancellation remain unresolved. Live tests use ordinary run admission, budgets, and approvals. They do not shift the saved schedule. Preview performs no work.

Publisher webhooks support named topics and bounded scalar equality subscriptions. The public TLS listener authenticates a bearer credential against its stored SHA-256 digest. It limits body size, request time, event freshness, source rate, topics, and matching fields. Receipt and matching admissions commit before acknowledgment. Source-scoped event identifiers survive restarts. Changed bytes for an existing identifier conflict. Revocation fences admission and dispatch. This is not a generic MCP subscription service.

## Controls and recovery

The app and typed owner conversation commands use one idempotent manager mutation boundary. Model text cannot invoke owner commands. A bot wakeup requires an explicit owner grant and shares cumulative owner budgets with its descendants. Its manager-local result must commit with the pending occurrence and replay identity.

Bot deletion uses a tombstone, disables routines and publishers, and requests cancellation. Dispatch checks the tombstone. Immutable connector audit remains intact. Duplication creates fresh paused definitions without sessions, grants, secrets, history, or reservations.

The durable inbox exposes outcomes and decision requests after reconnect. Transport notifications are refresh hints. Inbox delivery does not imply native push while the app is closed.

## Throughput checkpoint

Shared types, authority, and atomic storage precede clients. One implementation owner changes coupled schemas in its isolated worktree. Storage and admission, event and lifecycle behavior, and app integration form ordered verified commits. The parent supplies independent review.

The controlling design is Sol with the judge-approved Astra grafts for reusable templates, shared atomic admission, manager-local results, and concrete publisher topic subscriptions.
