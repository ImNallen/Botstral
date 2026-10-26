# Selected M03 design contract

## Problem and caller usage

Owners submit one durable task and retrieve the same task after reconnect or restart. Clients do not coordinate task creation and command delivery. Task settings are an immutable validated snapshot. Conversations and messages have stable retry identities. Authenticated owner methods submit, inspect, cancel, grant or revoke approvals, update typed permissions, write separate Markdown source kinds, search and resume events.

## Shape

Use the Astra candidate as the base and apply all six gates in m03-design-verdict.md. Its private CommandBody is immutable semantic identity and Delivery adds the current fence. The manager state actor owns one task reducer and atomically commits task changes, immutable outgoing commands and authoritative events. It also owns approvals and Markdown publication. The worker owns a private journal, semantic command tombstones, execution phases and durable reports. Report identity includes a journal incarnation persisted by both peers before execution delivery.

Graft Sol's single reducer, explicit storage admission bounds and retry identity for document writes. Delete duplicate task-start authority when integrating the existing worker-command route. Keep transport fences separate from command identity and old execution evidence. Lost authority, replaced journals and unknown external effects require attention rather than another start. Keep cancellation sticky until stopped evidence, preserving effects that arrived first.

## Synthesis decision

The independent Astra judge selected Astra at 22 of 24. Sol scored 18 and Luna 12. The parent also selected Astra, although its provisional scores were more generous. The judge identified concrete missing gates in all candidates and its stricter analysis is accepted. Full event sourcing lost because it adds projection/schema recovery rules without removing task ownership complexity. A shared cross-machine database violates worker disconnection and authority requirements. No Luna graft improves the selected shape.

The required gates are final execution-claim semantics, journal incarnation binding before any delivery, one reducer with ordered evidence and sticky cancellation, bounded durable report pagination and acknowledgments, Markdown publication recovery across source/index/events, and consistent bounded snapshots with future-cursor rejection. Read m03-design-verdict.md for each complete rule and its verification.

## Authority

Approval binds the stable owner, bot, task, operation, normalized arguments, permission revision, expiry and single-use execution identifier. Authenticate owner mutations. A manager restart deletes login sessions, not task identities or stable owner identity. Explicit revocation and policy revisions invalidate pending authority. Claims linearize admission. The worker rechecks local expiry, fence and cancellation and invokes the effect without an intervening execution queue. Lost or old permits never become fresh authority. Document the limit that revocation cannot undo an operation already admitted.

## Source and scope

Flat private Markdown sources remain authoritative and disjoint by typed document kind. The derived search index can be rebuilt. Published document revisions must reconcile into idempotent events after a crash. Production browser, shell, provider and model execution remain later work. Synthetic effect fixtures and crash barriers remain debug-only. Existing synthetic input cancellation has no external process group. Its stopped evidence must say so. A real controlled process-group fixture is required before making a process-group termination claim.

## Verification units

1. Capture failing actual-binary recovery acceptance, then add protocol identity and additive migration with prior-data preservation.
2. Implement the task reducer, transactional commands/events, conversations/settings, approval authority and memory APIs with focused behavior checks.
3. Implement worker journal, claims, bounded report replay and reconnect reconciliation. Check actual disconnect and restart boundaries with an independently durable synthetic effect ledger.
4. Complete Markdown and replay crash recovery, verify all acceptance rows, run repository verification and document scoped limits.

## Implementation reconciliation

No code deviations are accepted yet. The implementation owner updates this contract in the committed architecture document with evidence for accepted deviations before closing a unit. A test or implementation convenience cannot silently weaken the six gates.

## Principles that changed choices

Model the Domain selects a task reducer and typed immutable command body instead of scattered lifecycle booleans. Separate Before Serializing Shared State gives the code owner an isolated worktree while keeping a real single manager writer. Prove It Works requires actual binaries and independent effect evidence. Foundational Thinking puts protocol identity and migration before recovery logic. Sequence Work into Verifiable Units requires each unit's focused behavior checks before the next.
