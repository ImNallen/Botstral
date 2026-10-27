# W01 completion review

W01 acceptance passed on the corrected production source at `418d5610f203cddcc095a07632bf07f2f4198148`, with the final fixture correction at `62954fb`. Independent source review verified the corrections. [The evidence manifest](../agent/tests/evidence/w01-review/manifest.json) identifies the exact source, runtime, binaries, and retained results.

## Independent findings and corrections

The initial panel reviewed the same full change using gpt-6-astra, gpt-6.1-sol, and gpt-6-luna. The first two independently found a corrupt-transfer reconnect loop and permanent capability loss after interrupted Computer probes. All findings below were accepted.

| Finding | Correction | Evidence |
| --- | --- | --- |
| Corrupt transfers could repeatedly disconnect the worker and block later outputs. | Return durable unavailable receipts. Confirm the immutable offer on every connection before resuming chunks. Verify recovered final storage before acknowledging it. | The actual-binary regression first received `Error { code: InvalidMessage }` instead of terminal progress. The corrected protocol regression passed. |
| Missing or stopped disposable probe state could permanently disable Computer capability. | Reuse stage-aware disposable-probe cleanup. Released cleanup requires a Computer manifest marked as a probe. Admitted execution recovery remains separate. | A production run against the initial binaries retained `computer-probe/incomplete`, logged a missing manifest, and refused Computer submission with HTTP 409. The corrected two-VM run recovered both incomplete and failed/stopped probes, then executed guest work. |
| Recovery could publish renamed files without synchronizing their directory. | Synchronize the manager artifact directory before promotion and the worker spool directory before every immutable offer. | Independent source review traced the ordering. No real power-cut test was run. |
| Unavailable offers could close active output slots too early. | Require durable stopped evidence, or an unavailable uncertainty result paired with an uncertain execution outcome. | The actual-binary regression first received unavailable progress instead of rejecting the premature offer. The corrected regression passed. |

The corruption and probe gaps are tracked in [issue 14](https://github.com/ImNallen/Botstral/issues/14) and [issue 15](https://github.com/ImNallen/Botstral/issues/15). The completed change closes them through its pull request.

A fresh gpt-6-astra source reviewer verified the corrected tree with no remaining concrete blockers. It checked transfer identity, current leases, connection confirmation, publication ordering, offer terminality, disposable-probe callers, and separation from admitted execution recovery. This verdict is independent source review. Runtime acceptance is reported separately below.

## Verification

Full `make verify` passed on [macOS](../agent/tests/evidence/w01-review/verify-macos.log) and [Linux](../agent/tests/evidence/w01-review/verify-linux.log) after the corrections. Both release builds refused the nonproduction feature. [The Computer verifier](../agent/tests/evidence/w01-review/computer-checks.log) passed artifact protocol, storage reservation, spool, wire bounds, and generated-binding checks. TypeScript checks and all seventeen desktop unit tests passed. The initial native desktop verification remains applicable because its source and bindings did not change during recovery corrections.

The refreshed [browser two-VM run](../agent/tests/evidence/w01-review/computer-two-vm/results.json) passed all 35 checks. It proved actual navigation, file work outside coding workspaces, shared cookies/local storage/IndexedDB, cancellation, deadlines, approval revocation, crash non-replay, missing outputs, and retrieval after restart. It also corrupted an 8 MiB transfer during reception and verified that the real worker finished that output as unavailable and delivered the next output. Both probe-recovery regressions passed. The [process two-VM run](../agent/tests/evidence/w01-review/process-two-vm/results.json) passed all 21 checks.

The Linux production process suite passed five tests and the debug crash-boundary test. The [release-manager artifact regression](../agent/tests/evidence/w01-review/production-artifacts.log) passed after the corrections. The [source comparison](../agent/tests/evidence/w01-review/source-runtime-binaries.json) matched 133 files, including migrations, against the Linux build tree. Both hosts had no remaining fixture units after acceptance.

The raw [early-offer failure](../agent/tests/evidence/w01-review/early-offer-before.log), [corrupt-transfer failure](../agent/tests/evidence/w01-review/corrupt-transfer-before.log), and [production probe failure](../agent/tests/evidence/w01-review/probe-before/results.json) preserve the observed failures before correction. The first attempt at the new failed-probe fixture also caught an exclusive-write setup error. The fixture now replaces only its private configuration atomically. The final run passed with that correction.

Rerun the commands in [initial acceptance](w01-completion-acceptance.md), using fresh fixture roots. The corrected run used `/home/botstral/w01-review-0583cf15` and reused the sealed runtime at `/home/botstral/w01-runtime-76b03bf1`. The measured runtime and resource ceilings did not change. The supervisor and artifact synchronization corrections were source-reviewed without destructive host power-cut testing.

## Evidence audit

A separate gpt-6-astra reviewer parsed all 190 implementation activity items and compared the decision trail with retained verifier logs. It found no fabricated run or important unlogged pivot.

The audit identified two provenance clarifications. The initial resource-selection row pointed to an interim 1.5 GiB run. The final five-run 1 GiB evidence is [the maximum-viewport measurement record](../agent/tests/evidence/w01-completion/runtime-max-measurements.json). The initial collector truthfully hashed 106 selected source files but omitted SQL migrations. The refreshed collector includes SQL, frontend, and workflow files.

The implementation activity export did not retain returned web-page contents for its runtime-documentation lookup. Actual package requirements, installation, and browser probes were retained. The coordinator separately read the [supported operating systems and Node requirements](https://playwright.dev/docs/intro#system-requirements) and [matching-browser installation contract](https://playwright.dev/docs/browsers). This documentation limitation does not replace the actual runtime checks.

Comment review found three redundant fixture comments and no code-shape or correctness-suppression findings. The coordinator removed the comments. Raw evidence logs retain their original whitespace so their recorded hashes remain verifiable.

A final gpt-6-astra audit checked all 61 refreshed evidence hashes and 133 source hashes against the coordinator activity. It found no acceptance blocker or fabricated run. Its minor request to record the failed fixture replacement attempt is now an appended row in [the coordinator trail](../agent/tests/evidence/w01-review/coordinator-decisions.tsv). The manifest hash was refreshed after that addition.

## Final-offset crash recovery

The initial push check failed when the manager stopped after committing the final artifact offset but before verification and rename. Startup retained the complete `.part` file as transferring. A normal worker could finish publication by retrying, but startup alone did not restore availability.

Recovery now verifies complete partial files, renames valid content, and uses the existing synchronized promotion. Invalid content becomes unavailable. Incomplete transfers remain resumable.

The protocol test now uses the existing debug-only fault injector to stop at that exact boundary. It verifies the complete partial file and committed offset, then requires stored progress after restart before sending more bytes. Before the fix, the 16,401-byte transfer returned receiving instead of stored. The same test passed after the fix. A fresh gpt-6-astra source review found no blockers.

Run `cargo build -p botstral-manager --features nonproduction-test-worker`, then `cargo test -p botstral-agent --features nonproduction-test-worker --test m02_transport artifact_acceptance::w01_artifact_protocol_uses_durable_admitted_scope_and_idempotent_bytes -- --exact --nocapture`.

## Scope limits

Execution remains serial. Artifact bytes and records have finite retention budgets without automatic pruning. Supported browser state requires clean context closure. Open tabs, session storage, and unflushed crash-time state remain unsupported. Hostile same-account isolation, real power-cut durability testing, and W02 independent screens are not claimed.
