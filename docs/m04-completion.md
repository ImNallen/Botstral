# M04 completion acceptance

M04 is complete as of 2026-10-27. [Final independent review](../agent/tests/evidence/m04-completion/final-acceptance-review.md) approved aggregate closure with no unresolved acceptance blocker. The evidence combines observed live checks across separate Linux Manager and Computer VMs with final-source recorded-provider regressions. No complete fresh paid campaign passed. The original failed reports remain unchanged.

[The evidence manifest](../agent/tests/evidence/m04-completion/acceptance.json) records source, binary and runtime hashes, raw results, reviews and verification logs. Raw logs retain their original bytes; Git whitespace checking excludes those log artifacts. [The implementation guide](m04-coding-vision.md) describes ownership and limits. [The runners](m04-coding-vision-acceptance.md) reproduce the checks.

## Acceptance gates

| Gate | Observed proof |
| --- | --- |
| Real browser work and approved reasoning child | `production.browser_shared_file_and_owner_artifacts` passed in [the live full run](../agent/tests/evidence/m04-completion/live-full-failed.json). It fills and clicks the page, captures text and a screenshot, then observes a live model select an exactly approved browser child and receive its actual text. |
| Shared guest file and second bot | The same check creates a file outside the coding workspace and verifies its bytes through a second bot. |
| Durable owner artifacts with worker offline | `production.vibe_edit_separate_test_and_exact_resume` downloads the completed `answer.py` after stopping the worker. |
| Authorized real vision | `production.actual_vision` passes a retained PNG reference to EU `mistral-small-2603`. The model returns the random canvas digits `383599`. The [downloaded screenshot](../agent/tests/evidence/m04-completion/live-vision.png) was independently inspected and its digest matches the raw report. |
| Real Vibe change and relevant test | The live Vibe check changes `answer.py` from `return 0` to `return 42`. A separately approved Python task asserts the result. The coding ledger contains three settled edit calls. |
| Exact same-session resume | The live check resumes the successful task with native session `49d8c69e-57ee-6747-8756-01b71f260bd2` and retains that identity. The resume call settles. |
| Coding cancellation and containment | The [focused lifecycle run](../agent/tests/evidence/m04-completion/live-lifecycle.json) observes pinned native Vibe inside the exact execution scope, requests cancellation, receives a durable cancelled result, verifies the observed processes dead and the cgroup absent, and shuts the worker down cleanly. |
| Refusal before a provider call | That run submits a one-microdollar coding budget, receives `inference_budget`, and verifies neither a task nor coding-call record exists. Recorded tests also cover turn, token, context, deadline, concurrency and parent aggregate limits. |
| Restart without replacement | The focused run restarts Manager, waits for a newer ready worker generation and confirmed control, and verifies identical cancelled-task process evidence, calls and execution identities with empty scopes. [Final-source native integration](../agent/tests/evidence/m04-completion/linux-native-cancel.log) separately verifies completed coding calls, process evidence, outputs and exact artifact bytes after Manager restart without another recorded provider request. |
| Keys, private state and cleanup | Every focused-run permission, redaction, shutdown and cleanup check passes. [The exact provider-key scan](../agent/tests/evidence/m04-completion/final-secret-scan.json) finds no plaintext key in 571 parent and fixture files. The actual key was never sent to Computer VM. |

## Failures and corrections

The first new live campaign produced an invalid browser proposal. Manager refused it safely. [Its report](../agent/tests/evidence/m04-completion/live-rejected-proposal.json) remains failed. After successful fixture shutdown, the independently reviewed reconciliation retained its one full 53,249-microdollar attempt ceiling and released only unadmitted future calls.

The next full run passed reasoning, browser/file work, vision, Vibe edit, separate test, resume and offline download. Cancellation then left `scope_stop_unproved`, and worker shutdown failed. The original report remains `passed: false` and `live_acceptance: false`. Systemd reported the service stopped, but the worker discarded the underlying proof error. The exact original failing branch cannot be recovered.

[Issue 26](https://github.com/ImNallen/Botstral/issues/26) corrects two independently reproduced failure modes. Stop confirmation now authenticates and pins the scope, waits at most two seconds for kernel emptiness despite a lost stop response, and rechecks stopped service identity, boot and kernel emptiness. Populated, active, replaced or unreadable scopes fail closed. Diagnostics retain bounded safe error text. [Before](../agent/tests/evidence/m04-completion/stop-proof-before.log) and [after](../agent/tests/evidence/m04-completion/stop-proof-after.log) logs preserve the real-systemd regression results. This does not claim either reproduced branch caused the historical failure.

Independent review also corrected literal workspace glob boundaries in [issue 25](https://github.com/ImNallen/Botstral/issues/25). The actual pinned CLI test first failed a legitimate edit in a metacharacter path, then passed that edit and denied its sibling path after escaping. [Issue 24](https://github.com/ImNallen/Botstral/issues/24) corrected coding-job shutdown ownership. [Issue 23](https://github.com/ImNallen/Botstral/issues/23) added documented bounded token-detail decoding without weakening billing validation.

## Source applicability and verification

[The acceptance snapshot source comparison](../agent/tests/evidence/m04-completion/source-applicability.json) identifies two production files changed after the successful live Vibe/vision observations. They implement Process stop proof and its diagnostic. The additional manager change extends an ignored integration test. Mistral transport, coding authority, Vibe wrapper, model configuration, vision decoding and shared schemas are unchanged.

The accepted Linux snapshot matches its recorded component files. Installed Manager binaries match across the two VMs. [Provenance](../agent/tests/evidence/m04-completion/source-provenance.json) records the hashes and source archive digest. Existing installations remained untouched; corrected binaries used the unique base `/home/botstral/m04-stop-final.TIi8kJ`.

Final-source [Linux verification](../agent/tests/evidence/m04-completion/linux-production.log) passes eight real-systemd stop regressions, the native Vibe integration, the actual browser/file/process/artifact integration, and all five production Process acceptance tests. A ninth portable stop test rejects missing population data. The additional native integration passes cancellation after observing the real CLI, completed-artifact recovery, bounded manager shutdown, and retained unknown/held recovery after restart. Its provider is recorded.

[Repository verification](../agent/tests/evidence/m04-completion/verify.log) passes formatting, all-target Clippy, workspace/default and test-worker suites, release builds and both release refusals. The manager suite passes 56 tests with two Linux lanes ignored; those two lanes pass separately on Linux. Six acceptance-budget tests pass. The desktop's 17 tests, frontend build and six actual native desktop tests pass in their retained logs.

## PR review follow-up

[PR review](https://github.com/ImNallen/Botstral/pull/27#discussion_r4392589254) found that an earlier owner-selected Vibe deadline survived translation but differed from the parent deadline copied into child task settings. Valid coding proposals therefore stopped before approval. Child settings now use the translated Vibe deadline. Earlier, equal and later policy bounds produce the minimum of policy and parent deadlines, without changing approval or worker execution.

[The correction receipt](../agent/tests/evidence/m04-completion/pr-deadline-correction.json) records before/after source hashes and regression logs. The regression fails on the previous production code and passes after the fix. All 57 manager tests pass with the same two Linux lanes ignored, and workspace all-target Clippy passes. No additional paid call was made. The original independent review and Linux provenance describe the earlier acceptance snapshot; this receipt records the later State-only correction.

## Spending and scope

The authorized cap stays US$1. The persistent ledger retains immutable prior allowances of 432,544 microdollars, the rejected campaign's 53,249, the failed full campaign's entire 425,992, and the focused run's 53,249. Total reserved exposure is **965,034 microdollars**, or **US$0.965034**. This is a conservative ceiling, not an invoice or a subscription-credit balance. No allowance was reset or discounted to settled charges.

The focused cancellation had one sent call. Its execution outcome remains unknown, while valid usage settled at the configured ceiling charge of 798 microdollars. Restart preserves that record without replacement. The full 53,249 campaign allowance remains retained.

The focused report deliberately has `suite: coding-lifecycle`, `passed: true`, and `live_acceptance: false`. It proves only its named gates. The aggregate gate map above supplies the milestone result; it does not rewrite either failed full report.

Vibe has approved file-edit authority under the guest account. It is not a filesystem sandbox or a guarantee against external effects by unrestricted guest processes. Cancellation stops descendants and cannot undo prior effects. Helpers and retries count against task and parent limits. Daily account budgets, invoice reconciliation, connectors, owner-device pairing and remote access remain later work. Unsupported Vibe tools remain blocked.

## Review and decision trail

The [append-only decision trail](m04-decisions.tsv) records the choices and corrections. Independent review inspected retained test logs and recomputed evidence/source digests; it did not rerun the suites. Its transcript audit covers exported activity through position 409 with 88 truncated items, rather than every omitted output byte. Final closure and log relocation are appended after that audit.

Model the Domain kept authority and accounting in State and execution in Process. Build the Lever produced capped rerunnable acceptance tools. Separate Before Serializing Shared State required unique fixture roots and one locked persistent campaign ledger. Sequence Work into Verifiable Units kept implementation, runner and correction commits reviewable. Fix Root Causes replaced the failing stop proof instead of increasing the shutdown timeout. Prove It Works required actual native Vibe, kernel cgroups, downloaded image bytes and two separate VMs.
