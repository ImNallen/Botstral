# M05 independent implementation review

The panel reviewed the implementation through `3de495e` against the complete M05 intent. The reviewers read actual authorization, TLS, State, recovery, client, worker, and acceptance paths. They did not treat the implementation agent's successful tests as an independent verdict.

| Reviewer | Findings |
| --- | --- |
| [gpt-6-astra](m05-reviews/astra.md) | Shared client transfer storage and premature lock release. Unknown mutation outcomes after response storage or validation failure. |
| [gpt-6.1-sol](m05-reviews/sol.md) | Unknown mutation outcomes after response failure. Transient TLS alerts misclassified as permanent trust failures. |
| [gpt-6-luna](m05-reviews/luna.md) | Anonymous attempts blocking valid authentication. Provider verification holding the global materialization permit during network I/O. |

All five distinct findings were accepted under [issue 29](https://github.com/ImNallen/Botstral/issues/29). No finding was dismissed as a preference. The two independent mutation reproductions provide the strongest agreement. The single-reviewer findings each have a concrete reachable path.

## Corrections

One client now admits one live session. That session retains the protected directory lock, including after its client is dropped. Recovery rejects a session belonging to another client. The real native contract checks exclusive admission, lock lifetime, and ownership through the actual Manager.

After mutation transmission, every failure before a validated result or explicit correlated refusal becomes `UnknownOutcome`. The contract deliberately prevents response-stage creation, observes the unknown outcome, and confirms that the message committed through a fresh authoritative snapshot.

Certificate and pin validation failures remain terminal. Other TLS transport failures retry with the same trust. The native regression receives an unauthenticated fatal internal-error alert, reconnects through the same opaque proxy to the healthy pinned Manager, and installs recovered state.

Valid credentials are checked independently of the failed-attempt response budget. The focused regression first failed with `Valid owner authentication failed with Some(RateLimited)`. It now admits that owner and a properly invited worker while retaining refusal limits for invalid credentials.

Provider verification releases the materialization permit during network I/O. It reacquires the permit before finishing its bounded configuration reply. A recorded provider gate holds verification while a second owner successfully recovers state. The new regression passes. This regression was added after the correction, so it has no claimed before-fix execution.

## Comment review and remaining gates

Independent comment review found three narrative Python module docstrings. They were removed in `3de495e`. It found no workaround comments or suppression annotations requiring structural changes.

The coordinator ran the native client regressions, the authentication regression, the provider-concurrency regression, the Manager library tests, formatting, and all-target Clippy. [Fresh independent correction review](m05-correction-review.md) approved the five production corrections. Additional pinned-TLS fixtures confirmed that malformed responses and uncorrelated refusals produce unknown mutation outcomes, while a correlated refusal remains explicit.

The evidence audit required exact named acceptance gates, explicit endpoint keys, and complete production build-input coverage. The evidence validator now encodes those checks and their negative controls. The canonical trail maps inherited audit pointers to packaged initial logs. The audit inspected 177 exported parent activity items, including 23 truncated items, and does not claim an exhaustive transcript audit.

Final-source repository verification, all 15 local gates, both native correction checks, all 7 two-VM public-relay gates, both Linux production compatibility lanes, and desktop checks passed. [Final acceptance](m05-acceptance.md) links the source and binary manifests, complete sanitized logs, cleanup observations, and five passing evidence controls. Initial evidence remains historical.

The parent independently checked all 12 source objects against the reviewed correction, the exact 111-file evidence inventory, the retained Linux binary hashes, and the actual suite and cleanup records. It integrated the evidence and reran all five evidence controls successfully. Production review and final evidence inspection are complete. The transcript audit remains limited by its 23 truncated items.
