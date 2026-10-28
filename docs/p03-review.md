# P03 independent review

The owner needs routines that execute without an open client and preserve authority, budgets, results, and replay identity. Independent review inspected the initial implementation tree `d54ddd47c37c3c8c9e3d799d4d58ffc5d4c35130` and the subsequent correction `aeecf7a`.

## Correctness

The initial Astra review found one verified defect. Transcript compaction retained final response receipts but authenticated reads loaded only live turns. An independent seventeen-completion probe returned zero turns for the oldest run. The parent's routine regression reproduced the mismatch with `left: []` and `right: ["Recorded response"]`.

The correction exposes saved receipts as `RunView.archived_turns`, loads them through the authenticated read, and projects their responses in turn order. The client marks compacted execution details. Active inference still reads only live turns. Separate checks cover routine and ordinary owner runs, provider receipts, ordinal ordering, full text, and Unicode previews.

A fresh Astra recheck passed at `aeecf7a` and independently executed both archive regressions. [Issue 37](https://github.com/ImNallen/Botstral/issues/37) tracks the verified defect until the P03 change lands.

The initial review found no further verified failure in routine authority, admission transactions, shared reservation accounting, wakeup chains, publisher authentication and revocation, pinned bindings, bot lifecycle, or dispatch fences.

## Protocol and controls

The Luna review found no concrete authorization, mutation replay, journal-bound, or version-two cache migration gap. It objected that the creation form offers one string equality condition while the protocol allows eight typed conditions. The parent treats this as a documented form limit. Authenticated `/routine create` accepts the complete definition before activation, and the app definition editor accepts every supported condition. The Astra recheck agreed that this reduced form does not create a concrete unsafe behavior.

The Sol comment review identified two narration comments. Both were removed. It found no correctness suppressions or other required code changes.

## Evidence

The evidence reviewer matched all seven original verification log hashes and the video hash against the implementation checkout. The original verification record now identifies its tested Git tree. Its additional unlogged checks remain implementer-reported outcomes. The parent does not treat those entries as independently hashed evidence.

The video uses actual app components with recorded native manager responses. Its visible fixture label and acceptance documentation distinguish component rendering from real manager, provider, worker, and keychain execution. The parent also inspected a frame extracted from the video.

The decision trail now preserves implementation integration, verification, review findings, the regression correction, and the durable design verdict. [Parent verification](evidence/p03/parent-verification.json) records checks against the corrected code. Production Linux, pinned Vibe, multi-VM, OS keychain, and closed-app push limits remain those documented in [acceptance evidence](p03-acceptance.md#verification-limits).

## Pull request recovery follow-up

[The automated review](https://github.com/ImNallen/Botstral/pull/38#discussion_r4404948114) identified a second recovery defect. An unresolved native submission or cancellation relied on a bounded recent-history snapshot. Once the committed run disappeared from that snapshot, its saved intent stayed unknown and blocked new mutations.

The real-manager regression reproduced two unknown run intents after ten completions. Extending it to thirty-four completed tasks exposed the same problem for task cancellation and approval revocation. Native recovery now reads outstanding run and task identifiers through authenticated operations. Submission and approval confirmation retain exact matching. Cancellation also checks child tasks omitted from the snapshot. No mutation is replayed to establish its outcome.

The regression verifies recovery without expanding the displayed history, permits a new mutation afterward, and keeps mismatched submissions and missing records unresolved. [Follow-up verification](evidence/p03/babysit-verification.json) records the checks for this correction.
