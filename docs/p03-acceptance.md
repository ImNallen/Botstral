# P03 acceptance evidence

The owner needs durable scheduled and event-triggered work while clients are closed.

Implementation acceptance requires real SQLite restart checks, authenticated duplicate events with one counted external effect, changed-payload conflicts, IANA gap and fold behavior, anchored intervals, finite missed-work coalescing, pause and resume, unresolved overlap, pinned readiness, shared budgets, wakeup replay and chains, bot deletion, paused duplication, and more than sixteen completed runs.

App and typed conversation controls must share the authenticated owner boundary. Inbox results, failures, and decisions must survive reconnect. UI evidence must use the actual components and label fixture limitations. A successful build is not behavioral evidence.

## Verification record

Pending implementation and execution. PLAN.md remains unchecked until the corresponding behavior has evidence.

The baseline manager library run passed 85 tests with five ignored. The baseline desktop run passed 26 tests with one skipped. The existing paired workspace rendered through the committed M06 component driver before any visible changes. Logs are under `.audit/p03/` in the implementation worktree.

The first storage unit checks transaction rollback, stable trigger identity, twenty completed runs, bounded full run projections, durable inbox entries, locked vault, stale session revision, exhausted budget, missed intervals, pause and resume, unresolved overlap, owner command replay, bot tombstones, paused copies, authenticated event deduplication, changed-payload conflict, and typed owner conversation commands. The IANA calendar check covers the Stockholm spring gap and autumn fold.

The second unit passes nine focused P03 tests and strict manager clippy. `p03_manager_loop_executes_without_owner_clients_and_recovers_inbox` signs out the owner before the due instant, observes one inference call from the autonomous loop, restarts the manager, and recovers the same inbox and run identity. `p03_duplicate_authenticated_event_restart_has_one_external_connector_effect` uses the real connector HTTP adapter with a local MCP fixture, exact task approval, and a counted write. Repeated authenticated events and reopening SQLite leave the effect count at one. The fixture transport uses local HTTP; production ingress and connector transports retain TLS requirements.

Wakeup checks inject a failed SQLite write and prove that neither the pending wakeup nor its manager-local result commits alone. Replaying settlement creates no additional wakeup. Descendants share the original cumulative grant and owner budget. Reopen checks cover admission boundaries and inbox acknowledgment. Stale configuration and concurrent aggregate reservations block admission.
