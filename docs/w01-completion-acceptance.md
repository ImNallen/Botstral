# W01 completion acceptance

W01 acceptance and independent review are complete. [The final review record](w01-completion-review.md) covers the corrected production source at `418d561` and the final fixture correction at `62954fb`. Its refreshed evidence supersedes the initial implementation run below.

The initial implementation checks passed on `5316dad327af10ee164eb177c63ef43153dd0f23`. Independent review then found and corrected recovery gaps. No external acceptance prerequisite remains missing.

The [evidence manifest](../agent/tests/evidence/w01-completion/manifest.json) hashes every retained artifact. [Source and binary evidence](../agent/tests/evidence/w01-completion/source-runtime-binaries.json) confirms 106 source files match the Linux build tree. The tested agent SHA-256 is `a2bebbfde469c265979aee851c64e75cc52490f2c299d276640252c4b654be17`. The tested manager SHA-256 is `5769960e95ee83816cccbd6a29d5bc68a31a68d251f0f6c0bea766942682eda9`. The sealed runtime identity is `1e3b96245240d969ab380e4ce843f6ac68d27444d425cf211f75e4d2b6859061`.

## Requirement evidence

| Requirement | Actual result and evidence |
| --- | --- |
| Supported runtime and real browser | Pinned Node 24.22.1, Playwright 1.64.1, and Chromium 156.0.8078.4 revision 1248. Runtime integrity and browser probes passed. Navigation, fill, click, text capture, and PNG download passed in [32 two-VM checks](../agent/tests/evidence/w01-completion/computer-two-vm/results.json). |
| Guest browser, file, and shell work | The configured guest account wrote and read an approved absolute file outside coding workspaces. An existing shell command created the exported file. Exact downloaded bytes passed. [Process acceptance](../agent/tests/evidence/w01-completion/process-two-vm/results.json) also passed literal shell arguments, environment, output capture, and duplicate refusal. |
| Serial admission and resource ceilings | Global and per-bot occupied capacity left further approvals unconsumed. The 64 MiB spool reservation test passed. The [production manager test](../agent/tests/evidence/w01-completion/production-artifacts/artifact-admission.json) reserved exactly 256 MiB and refused the next claim without consuming approval. Five maximum-viewport workloads under the final 1 GiB and 128-task ceiling recorded 196,243,456 peak cgroup bytes and 87 tasks, with all outputs checked. [Raw measurements](../agent/tests/evidence/w01-completion/runtime-max-measurements.json) include samples and tool identity. These fixtures establish no general throughput promise. |
| Cancellation, deadlines, and non-replay | Real Chromium descendants were observed live, then absent after cancellation. Runtime and absolute deadlines failed with stopping proof. Active browser crash recovery retained one effect and an uncertain execution. The [Linux production suite](../agent/tests/evidence/w01-completion/linux-production-final.log) passed five process tests and the separate debug crash-boundary test. Journal replacement and unknown-scope refusal remain intact. |
| Separate execution and delivery facts | Completed work retained explicit pending delivery during transfer. Missing outputs, lost sealed snapshots, and corrupt manager blobs became durably unavailable without replacement execution. |
| Durable transfer and private owner download | Exact 8 MiB output bytes survived manager restart during transfer and another restart before download. Gap, altered offer, mismatched duplicate, unrelated task, execution, journal, worker, and unauthenticated access were rejected. [Production manager protocol evidence](../agent/tests/evidence/w01-completion/production-artifacts/artifact-protocol.json) proves lost chunk acknowledgment, lost final receipt, immutable offer identity, and duplicate delivery after launch approval expiry. Partial downloads were refused. |
| Preserved guest and browser state | Cookies, local storage, tested IndexedDB, and guest files survived another bot's task and an idle worker restart. A corrupt cookie database disabled the capability without erasing that database or guest files. |
| Existing compatibility | Full `make verify` passed on [macOS](../agent/tests/evidence/w01-completion/verify-macos-serial.log) and [Linux](../agent/tests/evidence/w01-completion/verify-linux-final.log). `make verify-desktop` passed on [macOS](../agent/tests/evidence/w01-completion/verify-desktop-final.log) and [Linux](../agent/tests/evidence/w01-completion/linux-compatibility-final.log). [Generated bindings](../agent/tests/evidence/w01-completion/bindings-final.log) and [unsupported-host refusal](../agent/tests/evidence/w01-completion/unsupported-host.log) passed. Both production builds refused the synthetic feature. |
| Fixture cleanup | [Final cleanup](../agent/tests/evidence/w01-completion/final-cleanup.json) found no fixture units or processes on either VM. Individual shutdown records and positive cgroup cleanup remain in the two-VM evidence directories. Fresh fixture state and the sealed runtime are retained for review. |

## Fixture scope and limitations

The fresh base is `/home/botstral/w01-76b03bf1`. The runtime is `/home/botstral/w01-runtime-76b03bf1`. Prior evidence under `/home/botstral/w01-570e76c4` and permanent owner state were not overwritten. The fixtures used synthetic owner data and fresh certificates. No model or production credentials were required.

The initial executor is serial globally and per bot. The artifact store retains at most 256 MiB and 512 records, with no automatic pruning. Open tabs, session storage, and unflushed crash-time browser changes remain unsupported. A corrupt shared profile requires operator repair. Guest setup privileges remain unchanged. Same-account hostile isolation is not claimed.

The only lifecycle adjustment beyond the accepted design recognizes the original inactive systemd invocation after automatic timeout, with positive proof that its recorded cgroup is empty. [The observed systemd state](../agent/tests/evidence/w01-completion/timeout-scope-proof.json) and updated independent-runtime crash acceptance prove why this correction was needed. Execution remains uncertain after a daemon crash. No substantive architecture deviation was required.

## Verification commands

The local contract is rerunnable with:

```sh
scripts/verify-w01-computer.sh .audit/w01-completion/evidence
make verify
make verify-desktop
```

Run Linux process acceptance on a systemd 255 guest:

```sh
scripts/verify-w01.sh /home/botstral/w01-fixture/process-evidence
```

Use fresh synthetic certificates and a fresh fixture base on each pair of SSH hosts. Install the pinned runtime separately. The browser fixture drives installed default release binaries, not a simulated browser:

```sh
python3 agent/tests/w01_computer_two_vm.py \
  --base /home/botstral/w01-fixture \
  --runtime /home/botstral/w01-runtime \
  --runtime-manifest .audit/runtime.json \
  --cert-dir .audit/certificates \
  --output-dir .audit/computer-evidence
```

The expected binaries live at `BASE/bin/production/botstral-manager` and `BASE/bin/production/botstral-agent`. The separate process fixture is `agent/tests/w01_two_vm.py`. The resource fixture is `agent/tests/w01_runtime_measure.py`. `agent/tests/w01_evidence.py` checks exact source equality with the Linux build tree and records runtime and binary hashes.

## Review boundary

The implementation agent did not create a pull request, push, merge, deploy, or start a watcher. The coordinator owns independent correctness, comment, and decision-trail review. The canonical append-only implementation log is `.audit/w01-completion/decisions.tsv` in the implementation checkout.
