# W01 process review and final acceptance

The first W01 unit runs one explicitly approved guest command with bounded output, descendant cancellation, and conservative recovery. Full W01 remains open. The reviewed production source is `bd1576000c4a122abc795a6c42e8cb3ae496c93b`. Later documentation and evidence commits do not change that source.

## Independent review

The initial gpt-6-astra, gpt-6.1-sol, and gpt-6-luna reviewers each found one actionable issue. The parent reproduced and fixed all three under [issue 12](https://github.com/ImNallen/Botstral/issues/12).

| Finding | Before | Correction |
| --- | --- | --- |
| Interrupted capability probes permanently disabled execution. | The actual production daemon left approved work queued after an interrupted probe. | Clean partial never-released probes and stop any recorded probe service before probing again. Real command uncertainty remains unchanged. |
| Reconnect confirmed control before old processes stopped. | The actual manager reported confirmed control while a previous process remained live. | Defer affected snapshot confirmation until recovery and verified stopping. Missing-journal sessions remain pending. |
| Fixture cleanup accepted failed service commands. | An unavailable service manager did not fail the cleanup check. | Check command results, terminal service state, and execution-scope emptiness. Explicitly absent units are accepted. |

The first reconnect correction also emitted unsolicited confirmations for synthetic-only sessions. Four existing protocol tests rejected that ordering. The parent restricted explicit snapshot confirmations to sessions with journaled process work. Focused checks then passed, followed by the complete compatibility verifier.

Fresh gpt-6-astra source review returned VERIFIED at the source commit above, with no blocking correctness, security, or structural finding. It inspected the complete production change and correction path. A suspected stale pending-confirmation race was dismissed because the manager ignores older revisions without changing current control. Two comment reviews found no added comments or suppressions to remove.

## Execution evidence

The parent repeated the actual running interfaces after the corrections. [The parent evidence directory](../agent/tests/evidence/w01/parent/) retains the commands, results, source hashes, and artifact manifest.

| Check | Result | Evidence |
| --- | --- | --- |
| Interrupted probe and reconnect negative controls. | Both failed on the original release and passed after correction. | [Before](../agent/tests/evidence/w01/parent/review-before.log), [after](../agent/tests/evidence/w01/parent/review-after.log). |
| Failed cleanup negative control. | Failed before correction. Both cleanup unit checks passed afterward. | [Before](../agent/tests/evidence/w01/parent/cleanup-before.log), [after](../agent/tests/evidence/w01/parent/cleanup-after.log). |
| Existing protocol ordering. | All focused actual-daemon and permit checks passed. | [Actual daemon checks](../agent/tests/evidence/w01/parent/compatibility-actual.log), [permit checks](../agent/tests/evidence/w01/parent/compatibility-permits.log). |
| Full macOS verifier. | Passed, including both nonproduction release-feature refusals. | [macOS log](../agent/tests/evidence/w01/parent/verify-macos.log). |
| Full Linux verifier. | Passed, including both nonproduction release-feature refusals. | [Linux log](../agent/tests/evidence/w01/parent/verify-linux.log). |
| Linux production and crash acceptance. | Five default-release tests and the debug crash-boundary test passed. | [Linux process log](../agent/tests/evidence/w01/parent/verify-process.log). |
| Separate manager and Computer VMs. | All 21 checks passed with the corrected binaries and checked cleanup. | [Two-VM results](../agent/tests/evidence/w01/parent/two-vm/results.json). |
| Source identity. | All 24 changed code, schema, binding, and fixture files matched the Linux build tree. | [Source hashes](../agent/tests/evidence/w01/parent/source-files.json). |

The initial [desktop compatibility and generated-binding checks](w01-verification.md) passed. Their TypeScript and desktop sources have not changed since that verification. The final process tests also retain missing-journal control refusal, independent runtime termination, timeout behavior, output bounds, and ordinary descendant cleanup.

## Decisions and limits

Model the Domain separates effect knowledge from stop proof. Make Operations Idempotent forbids another launch after an uncertain start. Foundational Thinking and Type System Discipline put immutable bounded inputs before execution. Boundary Discipline validates network, configuration, and operating-system observations.

Exhaust the Design Space compared three candidates before selecting service supervision. Experience First prioritized useful guest execution. Separate Before Serializing Shared State isolated implementation while retaining one journal writer. Sequence Work into Verifiable Units ordered the baseline, implementation, corrections, and acceptance. Build the Lever and Prove It Works require rerunnable binary and cross-VM tests. Fix Root Causes corrected probe recovery and control confirmation at their original decision points.

Execution remains opt-in and requires Linux with systemd 255. One command runs at a time, with at most 1 KiB retained per output stream. Browser execution, model integration, Vibe, artifact transfer, desktop task controls, and full W01 acceptance remain later work. The configured guest account is not a hostile-code sandbox. Unknown effects still require attention and never authorize automatic relaunch.

## Audit coverage

The canonical implementation trail is [.audit/w01-implementation.tsv](../.audit/w01-implementation.tsv). The parent appended corrections for unavailable intermediate logs and inspection artifacts. Those earlier stages and their exact timing cannot be reconstructed from the retained final evidence.

The Sol trail reviewer sampled the implementation transcript and expanded selected tool actions. It verified the original 50 artifact hashes and 22 source hashes. It did not audit every transcript item. The final Astra source reviewer found no contradiction in the parent decisions or appended corrections. Neither source review is a substitute for execution acceptance or a complete transcript audit.
