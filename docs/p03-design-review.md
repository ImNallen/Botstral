# P03 design verdict

Choose Sol as the base. It gives the clearest owner authority boundary and a scoped implementation inside the existing manager writer. Adopt two focused grafts from Astra.

| Criterion (0–4) | Astra | Sol | Luna |
| --- | ---: | ---: | ---: |
| Durable occurrence-to-run admission | 4 | 4 | 3 |
| Preserved authority and execution checks | 3 | 4 | 3 |
| Explicit schedule semantics | 4 | 4 | 2 |
| Complete control, events, wakeups, delivery, lifecycle | 4 | 3 | 2 |
| Recurrence beyond sixteen runs with bounded storage | 4 | 4 | 1 |
| Single domain owner without fabricated authority | 4 | 4 | 3 |
| Total | 23 | 23 | 14 |

Sol wins the tie because its pinned execution bindings and typed owner conversation actions avoid silent configuration changes and model authority escalation. Its event adapter remains undecided. Astra makes that deliverable concrete, but its proposed general manager-local routine tool is broader than needed. Luna misses the existing historical-run cap and leaves pause/resume and bounded wakeup admission underspecified.

## Graft 1: Explicit run template and atomic manager operations

Use a dedicated reusable RunTemplate with relative durations and execution ceilings. Do not persist SubmitRun, absolute nested deadlines, approvals, image references, or transient run identifiers. Preserve Sol's owner-approved configuration revision and session binding; stale bindings produce a durable blocked occurrence. Do not automatically select another session, model, configuration revision, or regional endpoint.

Extract private validation and transactional insertion from owner run submission. Owner submission retains authentication. Routine submission requires an enabled, current owner-approved routine. One transaction commits occurrence, ordinary run, budget reservation, and schedule advancement. Refresh every generated deadline, including Vibe deadlines. No fabricated system owner or unchecked insertion API.

Use Astra's distinction between worker-task intents and manager-operation results for the narrow RequestWakeup tool. Commit its durable result and wakeup reservation atomically. The tool can request only a bounded future one-shot under an explicit owner grant. Its replay key includes parent run and tool call identity. Descendants inherit the same grant and aggregate budget; they cannot replenish authority or budget. General routine CRUD remains authenticated typed owner commands or owner-confirmed conversation cards.

## Graft 2: Ship one concrete subscription source

Implement authenticated publisher webhooks with named topics and durable topic subscriptions. Use bounded typed equality matching, source-scoped event identifiers, payload conflict detection, timestamp/replay limits, and protected source credentials. Receipt insertion and matching occurrence creation are atomic before successful acknowledgement. Paused or deleted routines cannot admit queued deliveries. Revocation prevents new ingestion and admission. Expose source health and failures in durable notifications.

Do not add generic connector polling, provider registration, arbitrary MCP subscriptions, or an event framework. Describe the shipped capability precisely as publisher webhook topic subscriptions. If PLAN.md requires a particular provider-managed subscription, this graft alone does not satisfy it; that adapter needs an explicit implementation contract.

## Decisive contracts

- Test means a real, bounded ordinary run through current permissions, approvals, overlap, and budgets. Label it as a live test. Preview is a separate non-executing calculation. Luna's preview-only Test does not prove routine execution.
- Preserve uncertain runs and deduplication evidence. Change the sixteen-run limit to unresolved active work and bound terminal transcript retention, history pages, and workspace projections together.
- Keep Sol's explicit daylight-saving, catch-up, pause/resume, single-flight, tombstone deletion, paused duplication, and durable inbox semantics.
- Do not claim offline push delivery from an inbox that appears on reconnection, or exactly-once external effects from exactly-once run admission.

Implementation can proceed with these contracts. The critical proof is duplicate event delivery plus manager restart yielding one ordinary run, with an owner-visible result after reconnect, while stale authority and exhausted budgets block admission.
