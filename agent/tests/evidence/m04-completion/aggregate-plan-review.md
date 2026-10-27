# Independent acceptance-plan review

Verdict: The proposed focused lane can support aggregate M04 acceptance. Another full paid campaign is not inherently required. Acceptance remains conditional until the focused lane, final-source regressions, and gate mapping pass. This review approves an evidence strategy, not the unfinished implementation or milestone closure.

I read AGENTS.md, agent/tests/m04_two_vm.py, docs/m04-coding-vision-acceptance.md, docs/m04-acceptance.md, and the retained full-run result. I also inspected the production coding integration restart assertions. I made no provider calls or secret reads.

The retained report at final-live/68b2623d-c2d2-4243-9d61-62f19e847e7a/results.json correctly reports passed=false and live_acceptance=false. It independently records passed browser/shared-file/reasoning, vision, and Vibe edit/test/resume checks. Cancellation timed out; restart was missing; worker shutdown failed. Cleanup passed. These facts must remain visible and the original report immutable.

## Required scoped-lane evidence

1. Give the lane an explicit coding-lifecycle suite name and its own required-check set. A passing scoped report must not claim that the full campaign passed. Preserve source, installed binary, runtime, machine, and fixture identities.
2. Reserve exactly one call, 53,249 microdollars, through the existing persistent ledger before provider work. Keep immutable prior 432,544 and existing allocations 53,249 and 425,992. The resulting hold is 965,034 of 1,000,000. Keep the previous-reserved argument at its immutable prior value. Do not replace it with the accumulated ledger total. A new failed attempt leaves its reservation intact.
3. Observe the actual pinned Vibe CLI in the matching admitted execution scope before cancellation. Require durable cancelled/stopped evidence, empty/dead execution scope, dead observed descendants, and clean worker shutdown. A vanished PID alone is insufficient. Preserve execution identity and stop evidence.
4. Exercise pre-call refusal through the production API and require the expected budget error. Confirm no admitted coding attempt or process scope for the rejected task. The refused request must not silently require another paid allocation.
5. Restart the actual manager, reconnect the worker where necessary, and verify unchanged cancelled-task identity/accounting and no new coding attempts or native execution. Observe beyond initial restart response so delayed replay cannot escape the assertion.
6. Complete all cleanup, private-state, and redaction checks. Do not turn the previous shutdown failure into an allowed failure.

## Remaining coverage objection

The original final restart gate checks settled coding calls and downloads a completed coding artifact after manager restart. A tail that only restarts a cancelled task does not cover that behavior. The currently inspected production integration checks an unknown/held stalled call after restart and asserts its recorded provider request count stays at five. It does not presently repeat completed-coding artifact retrieval after that restart.

Before closure, extend the final-source recorded production integration to retain the completed edit task/call snapshot and verify identical settled accounting plus exact downloaded artifact bytes after manager restart, with no additional recorded upstream request. This costs no paid call and supplies the missing completed-result branch alongside the real two-VM cancelled-task restart branch. An existing equivalent executed test is also sufficient if cited precisely. Do not silently redefine the original gate as cancellation-only recovery.

## Source applicability and closure

Record an exact hash comparison between the failed full run and final production source. Unchanged Vibe/vision files alone are insufficient because the corrected Process lifecycle is shared by browser, coding, and ordinary processes. Review the actual diff and rerun final-source actual-worker coding integration, actual-browser recorded tool-loop integration, and Linux Process cancellation/shutdown regressions. Preserve their results and installed binary provenance. Broader behavior changes discovered in the diff require corresponding fresh coverage.

The closure document must map every milestone gate to an actual passed check and source applicability rationale, label recorded endpoints separately from live Mistral calls, and disclose that acceptance combines reports rather than a fresh passing full campaign. Update stale pending claims in current acceptance documentation while retaining historical failures. Independent source review of the Process correction remains required.

A full paid rerun becomes necessary only if final changes invalidate earlier live-provider observations in a way focused or recorded checks cannot cover. Lack of remaining budget is not evidence of acceptance; an uncovered gate must remain open.
