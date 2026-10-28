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
