# C02 review record

Independent lifecycle review by `gpt-6-luna` covered source `e81cdbd51e2e852d2e6a78b5505eccf3a43ff8ed` against base `6c2b70b03295cdc8a5f69411f0bb0628f383c7fd`. Security review and production acceptance are separate runs.

## Missing-profile sign-out

The reviewer reproduced one P2 defect. When the profile directory was absent, [native sign-out](../desktop/src-tauri/src/remote.rs) returned success before deleting the keychain credential. The reviewer did not prove automatic reconnection. Restoration without the profile failed with a storage error.

The parent added a regression in [the native lifecycle tests](../desktop/src-tauri/tests/remote.rs). Before the fix, it failed with `Successful sign-out must delete the native credential after profile removal`. [The failure log](evidence/c02/signout-before.log) preserves that result.

Commit `6aee65db3e7b887484ca4a823f56bf45f1144652` recreates the private sign-out marker directory and attempts credential deletion even if marker persistence fails. The regression covers successful deletion and failed deletion after profile removal. The failed-deletion case restores a signed-out view without workspace authority. [All six deterministic lifecycle tests passed](evidence/c02/signout-after.log). The actual-manager contract with real macOS Keychain and the renderer parser passed again after the fix.

An independent recheck by `gpt-6-luna` at `7dc6935` confirmed this correction. It found a separate concurrent sign-out defect. A second call could return success while the first credential deletion remained blocked. The parent reproduced this with a blocked native store and moved sign-out serialization before lifecycle changes. Both callers now wait for deletion. [The failure log](evidence/c02/concurrent-signout-before.log) records the defect. [All seven deterministic lifecycle tests passed](evidence/c02/lifecycle-after.log). The fresh `gpt-6-luna` recheck at `d5ecb8c` found no lifecycle defects. It also tested storage-directory creation failure and confirmed native deletion was attempted before returning a storage error. Its native actual-manager and real Keychain test passed. The wrapper then exited 1 because that isolated checkout lacked `vitest`, so its renderer contract did not run. The parent renderer contract passed separately. Packaged production acceptance remains open.

## Evidence corrections

The reviewer found stale source hashes and outdated statements that the packaged synthetic walkthrough remained pending. The parent preserved the original implementation manifest and verification record, generated a current source manifest, and added [a verifier with negative controls](../desktop/fixtures/c02_source_record.py). It rejects changed hashes, missing files, and extra files.

The acceptance record and README now distinguish the completed packaged synthetic workflow from production-worker and remote-route checks. The earlier apparent live-update failure was rejected after a fresh rendered capture showed completion without Refresh. The native contract now observes activity without refresh commands and passes.

## Journal recovery and admission

Independent security review by `gpt-6-astra` at `e81cdbd` reproduced two defects against the actual native service and manager. A journal validation failure installed a client before restoration completed. Recovery then overwrote unknown operations and admitted new mutations. Separately, admitted requests could produce a journal larger than its reader's 1 MiB limit.

The parent [reproduced both failures](evidence/c02/journal-before.log) before changing production code. Restoration now installs the client only after journal and workspace validation. Pairing also refuses a missing or invalid journal when saved authority exists. Failed validation leaves the original bytes intact and blocks mutations. Repairing protected permissions permits restoration of the original unknown operation.

Every journal write now shares the reader's byte and record bounds. Admission reserves space for status growth and compacts only confirmed or refused records. Unknown, pending, and attention records survive compaction. The actual-manager contracts verify failed validation, missing journals, bounded admission across restart, retention of attention records, and refusal to replay unknown work. Each case receives a fresh manager fixture to preserve the production authentication limit.

The [actual-manager and macOS Keychain contract](evidence/c02/native-security-final.log) also passed after correction. The fresh `gpt-6-astra` review at `d5ecb8c` reran both original reproductions and tested malformed and missing journals, permissions, links, record counts, exact byte-budget growth, staging failures and pairing bypasses. It found no surviving findings and released the security hold for the two journal defects. This verdict excludes the later browser-policy correction and production test support. Those deltas await a separate independent review.

The complete frozen-source [security report](evidence/c02/security-independent-recheck.txt) and [lifecycle report](evidence/c02/lifecycle-independent-recheck.txt) are retained verbatim. Their relative local-artifact references describe the original isolated review checkouts.

## Browser verification helper cleanup

The final `gpt-6.1-sol` delta review at `d2aa248` reproduced one P2 issue in `manager/tests/c02_browser.py`. A native shutdown timeout escaped its finalizer before private invitation removal and result persistence. No production defect was reproduced in the browser-policy correction. Eighteen manager tests, seven native tests, the local production fixture restart, build isolation and evidence consistency passed in that review. [The full report](evidence/c02/final-delta-independent-review.txt) retains its exact commands and limits.

The parent [reproduced the failure](evidence/c02/browser-cleanup-before.log), then separated shutdown, private removal and evidence persistence. A failed shutdown now records its error and stops the scoped subprocess. Removal failure is also recorded without skipping the result file. The shared subprocess constructor closes its parent log handle after launch. [The original reproduction passed](evidence/c02/browser-cleanup-after.log), as did [three local regressions](evidence/c02/browser-cleanup-regressions.log) with resource warnings treated as errors. The regressions run in `make verify-desktop`. They use synthetic local protocol responses and do not claim OS credential cleanup after an actual shutdown failure. The fresh `gpt-6.1-sol` [helper recheck](evidence/c02/helper-independent-recheck.txt) at `f8846da` reran the original reproduction and three regressions, exercised eight neighboring failure cases, and confirmed the old helper still fails. No findings survived. The specific helper hold is released.

## Hosted review follow-up

Hosted code review at `ccf0998` found two additional defects. [The configuration finding](https://github.com/ImNallen/Botstral/pull/31#discussion_r4399655618) reproduced when an actual manager retained two verified revisions. Native submission returned `inference_unverified` because it selected revision 1. [The worker finding](https://github.com/ImNallen/Botstral/pull/31#discussion_r4399655625) reproduced when the retained session referenced a revoked worker. The rendered chooser was absent despite a ready replacement.

The native actor and renderer projection now use the same maximum-revision selection for each bot. The actual-manager Keychain contract submits with revision 2 and checks that exactly one current configuration is projected. Execution settings now offer replacement only when the assigned worker is revoked or no session exists. Sending remains blocked until the manager confirms the replacement session. The frontend regression selects the replacement and observes sending become available.

The packaged app also completed this recovery with an actual manager and a separate enrolled transport worker. It preserved the existing session identity, reached confirmed status on the replacement worker, and completed a new recorded-assistant run using revision 2. This does not claim new process or browser execution. [The 24-second recovery video](assets/c02-worker-recovery.mp4) uses actual native state captures with pauses condensed. All isolated state and processes were removed after graphical sign-out and exact Keychain absence verification.

[Before and after verification](evidence/c02-review/verification.json) retains the failures, passing actual-manager contract and renderer parser, all 26 frontend tests, full desktop verification, the manager session-rebinding test, graphical identities and cleanup. Hosted follow-up review continues on the new head.

## Hosted lifecycle follow-up

The next hosted review at `d28f902` found two P2 defects. [Observed revocation](https://github.com/ImNallen/Botstral/pull/31#discussion_r4399815908) cleared the view without persisting the decision. An actual-manager reproduction restarted against an unreachable route and observed `cached_activity=true`. [Reapproval reconciliation](https://github.com/ImNallen/Botstral/pull/31#discussion_r4399815916) accepted a retained historical binding. An unknown, uncommitted regrant became `confirmed` while the actual approval remained revoked.

The actor now persists a protected revocation marker before clearing activity, latches the decision in memory, releases the old client, and checks the marker before restoring credentials or cache. If marker persistence fails, cleanup attempts credential deletion and cache removal. If both fail, a distinct storage failure remains visible. Only an explicit new pairing clears the marker after preparing the fresh identity. The actual-manager regression now blocks offline cache restoration and then completes an explicitly authorized new pairing.

Approval recovery now requires the requested approval identity, permission revision, expiration, complete immutable command match and an unrevoked approval. The actual-manager regression retains uncertainty for an old revoked binding and an unrevoked binding with the wrong expiration. It confirms only the matching granted binding. The injected unknown journal represents a persisted request that never reached the manager. The fixture pauses its synthetic worker to keep the earlier grant unconsumed; this is a recovery test, not new process execution proof.

Both reproductions failed before correction and passed afterward. Full desktop verification passed with all six isolated journal contracts, and the actual-manager macOS Keychain and renderer contract passed. [The lifecycle verification record](evidence/c02-review/lifecycle-verification.json) retains commands, source and exact limits. Hosted re-review of the current head continues.
