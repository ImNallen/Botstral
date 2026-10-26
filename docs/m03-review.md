# M03 implementation review

The first independent correctness review used `gpt-6-astra` on `a17f72f` against `3102400`. It returned NOT VERIFIED. The [review thread](t3-thread://v1/thread:delegated-task:command%3Amcp%3A51847601-a509-4b04-adda-f8cc3c0fd61e%3Adelegate-task%3A7f0b9adb-a1fb-421b-b1b6-58a93ec0ac6a) records its findings and audit of the implementation evidence. A separate `gpt-6.1-sol` comment review proposed two comment removals and one no-op branch removal. The parent applied all three.

## Correctness findings and fixes

| Finding | Fix and evidence |
| --- | --- |
| One invalid permit reconciled unrelated pending claims. | Reconciliation now targets one execution. The actual-daemon regression failed with `valid execution became uncertain`, then completed the other session once after the fix. |
| Denial of an older claim discarded a renewed grant. | Pending claims retain their original binding separately from the latest delivery. The actual-daemon regression timed out waiting for the renewed claim before the fix and completed once afterward. |
| Corrupt derived data pages blocked startup despite valid Markdown. | Index rebuilding discards the validated cache and sidecars and recreates them from source files. The actual-manager regression proved schema creation succeeded while cache deletion reported `database disk image is malformed`. Restart failed before the fix and recovered afterward without changing the source. |
| Snapshot continuation and aggregate bounds lacked acceptance. | Tests now append events between multiple retained pages, check exact sequence continuity, compare all messages after restart, fill every aggregate admission limit, reject overflow and check complete encoded snapshot size. |
| Parent inspection found UUID sorting reversed conversation order. | Messages now have a durable append sequence. The actual-manager regression failed with `second` before `first`, then preserved append order across retry and restart. |

[Issue 8](https://github.com/ImNallen/Botstral/issues/8) tracks message order. [Issue 9](https://github.com/ImNallen/Botstral/issues/9) tracks the independent recovery findings. The [acceptance record](m03-acceptance.md) names every regression and the committed evidence files.

## Accepted deviations and scope

The independent reviewer accepted JSON task and approval aggregates. The single manager actor preserves their transaction boundary. It also accepted cancellation delivery using the original immutable execution identity and sticky intent for the synchronous synthetic adapter.

The synthetic adapter performs one synchronous operation. It proves admission fencing and independent pending claims. It does not repeat the historical M02 test that paused one actively progressing input stream while another continued. Active input pause and resume remains W02 acceptance. `NoProcessGroup` evidence does not prove operating-system process-group termination.

The initial audit found no fabricated final verification result. Earlier intermediate log pointers now have committed copies under [intermediate evidence](../agent/tests/evidence/m03/intermediate/). The decision log records these accepted deviations and the reduced active-input coverage.

## Final gate

The parent fixed the findings and passed sixteen focused recovery fixtures plus feature-enabled clippy. That corrected head passed full verification, including eighteen feature-enabled actual-binary fixtures and both release-feature refusals. [The complete log](../agent/tests/evidence/m03/parent-verify.log) records that run.

A fresh `gpt-6-astra` review at `ee7e58a` accepted those corrections and their evidence, but found one remaining progress defect. Revocation followed by an identical regrant recreated the cached delivery, so denial of the older claim could suppress further transmission. The parent tested the failed premise that renewed authority must change delivery content. The actual-manager regression timed out waiting for redelivery before the fix and obtained a delivery and valid permit afterward. Claim denial now invalidates that transport cache entry.

Full verification and a targeted independent recheck of this final cache correction remain pending. No merge is authorized.
