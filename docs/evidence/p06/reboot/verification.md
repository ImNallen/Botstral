# P06 host reboot verification

The real manager guest reboot passed on 2026-10-30. The production manager restarted through an enabled systemd unit with its vault locked. A protected credential-use request failed while locked and succeeded after explicit passphrase unlock.

The verified operation was `POST /worker/v1/protected-browser` through authenticated worker TLS. A synthetic protocol worker supplied the destination and execution claims. The fixture submitted the released values to its controlled HTTPS loopback site. This check proves manager secret release, encrypted persistence, and policy enforcement. It does not prove browser filling or production agent supervision. Those have separate P06 acceptance evidence.

## Recorded source and build

[build.json](build.json) records the source archive hash, Linux toolchain, build command, binary hash, and build cleanup. The source commit was `271f9cd72dabd2d0749027b84d355ca046de5f7e`. The command was `cargo build --locked --release -p botstral-manager`. It used the installed Rust toolchain on `botstral-computer` without production source changes or test features. [build.log](build.log) records the successful build.

The binary SHA-256 was `57a671a63e904189510ac7c4d1a7f0f13e3df500cbff005d873b242464872fa3`. The fixture compared the supplied binary with `/proc/<MainPID>/exe` before and after reboot.

## Observed behavior

[resources.json](resources.json) records the workload inspections, bounded SSH reconnection, and cleanup. [events.jsonl](events.jsonl) records the assertions. [before.json](before.json) and [after.json](after.json) record service, binary, and ciphertext identities.

- The manager guest had no unrelated user workloads or held package-manager locks before reboot.
- The paired fixture owner authenticated and saved one synthetic login through protected binary entry. Its policy required approval for every fill and restricted access to one bot and one loopback origin.
- Before reboot, the vault was unlocked. Release required explicit approval, the loopback site accepted the released login, and a repeated release failed.
- The kernel boot ID changed from `96625ae4-32cf-40b5-b7e1-355c7866229d` to `b4b87cad-8100-42e3-bfe1-f9e6d3597293`.
- The enabled systemd unit started automatically with a new invocation ID. The manager reported a locked vault. Its binary and encrypted login ciphertext were unchanged.
- A fresh, admitted execution attempted protected `prepare` while locked. The manager returned HTTP 403 and the site received no credential submission.
- The owner explicitly unlocked through protected passphrase entry. The same execution, authority, and destination then received a review. The saved metadata was unchanged, approval remained mandatory, and the released login authenticated to the loopback site.
- The manager log contained none of the synthetic owner password, vault passphrase, device credential, username, or login password.

The fixture did not configure a provider, call an external website, run inference, or spend a model budget. It provisioned no automatic unlock key. The private fixture passphrase file belonged to the test driver and was removed during cleanup.

## Rerunnable command

The script requires existing SSH access as UID 1000, passwordless sudo on `botstral-manager`, Python 3, OpenSSL, systemd, and a current-source production Linux x86_64 manager binary. It uses only the Python standard library and the existing `m05_acceptance.py` helper. The `run` command authorizes a real reboot of this named guest.

The recorded invocation was:

```sh
python3 manager/tests/p06_reboot.py run \
	--binary .audit/p06-reboot/botstral-manager \
	--evidence .audit/p06-reboot/run-5
```

Each invocation requires a new evidence directory and creates a unique private guest directory and boot-enabled unit. The default reconnection deadline is 180 seconds. The script exports value-free records and checks the service is stopped before removing its own unit and directory. A cleanup failure leaves the resource names in `resources.json`.

## Cleanup and earlier attempts

The final unit was `botstral-p06-reboot-b78326ddd1e9.service`. Its private directory was `/home/botstral/p06-reboot-b78326ddd1e9`. Both were removed after the service reached `inactive` with `MainPID=0`. The unit then reported `LoadState=not-found`.

The owned build directory `/home/botstral/p06-reboot-95a38271d648-build` was removed after checking that no user process held its files, executable, or working directory. The computer guest was not rebooted. No Mac, hypervisor, Docker, runtime, or unrelated service configuration changed.

[attempts.json](attempts.json) records four earlier fixture failures and their completed cleanup. The first exposed fixture log permissions. The second stopped at the workload guard. The third encountered the intentionally locked metadata read. The fourth waited for a redundant session-control message after reconnection. Attempts three and four included real guest reboots but did not pass the complete check. Their failure logs remain under [attempts](attempts/). The complete fifth attempt is the acceptance evidence above. No production defect was reproduced.
