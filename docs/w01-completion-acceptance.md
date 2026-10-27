# W01 completion acceptance

Computer implementation and its two-VM browser acceptance have passed. Final full verification and independent coordinator reviews are being reconciled. W01 checkboxes remain open until the required evidence is recorded.

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
