# M06 independent review and corrections

M06 gives the owner reusable connections, separate bot grants, recoverable state, and separate Linux deployments. Three independent reviewers examined `4483b183671aec5bc36a65fa11c25308fb127423..48bf1bb2cc917499a6889f24519426f5f0666755`.

| Reviewer | Original verdict | Report |
| --- | --- | --- |
| Astra | ISSUES | [Reproductions and evidence audit](m06-reviews/astra.md) |
| Sol | ISSUES | [Reproductions and evidence audit](m06-reviews/sol.md) |
| Luna | PASS | [Focused checks and evidence audit](m06-reviews/luna.md) |

The two issue reports identify four distinct defects. The coordinator reproduced every defect against the integrated checkout before changing production code. [Issue 34](https://github.com/ImNallen/Botstral/issues/34) tracks their correction in `290421d`.

| Finding | Coordinator reproduction | Correction and regression |
| --- | --- | --- |
| Schema references block the State writer. | A 2090-byte schema reached task admission and did not return within the probe's ten-second limit. The simple control completed. [Before log](evidence/m06-parent/review-schema-before.log). | Discovery and argument validation bound local reference expansion before compilation. Shallow local pointers still work. Fan-out, cycles, scope changes, and dynamic references are refused. The actual discovery settlement records the invalid catalog as a protocol failure. |
| Expanded catalogs stop shared inference. | A valid admitted request expanded to 72325 bytes and stopped the production run pump. [Before log](evidence/m06-parent/review-catalog-before.log). | Admission checks the combined provider request against 65536 bytes. Existing oversized records and growing transcripts stop only their own run. A regression seeds an older oversized record before a healthy run and proves that the healthy run receives the next call permit. |
| Generated connector limits prevent worker startup. | The untouched generator output failed production worker loading with `InvalidMessage`. [Before log](evidence/m06-parent/review-generator-before.log). | The generator now emits the existing Process maximums of 1024 output bytes and 256 MiB memory. A test loads its actual output through production `Configuration::read`. |
| Maintenance omits the connector interpreter. | Diagnostics accepted modified bytes, and a verified backup restored configuration with a missing interpreter. [Diagnostic log](evidence/m06-parent/review-diagnostics-before.log) and [recovery log](evidence/m06-parent/review-recovery-before.log). | Recovery requires coverage of `connector.json`'s interpreter. Diagnostics check its pinned digest. Tests refuse omission and tampering, then restore and verify the original interpreter bytes. |

The [20 manager connector tests](evidence/m06-parent/review-authority-after.log) and [14 packaging tests](evidence/m06-parent/review-packaging-after.log) pass after correction. Their assertions cover the observable failures rather than the implementation of the new bounds. Both [rebuilt Docker images](evidence/m06-parent/review-images.json) pass. The Linux worker acceptance uses the actual generator, refuses tampering and omitted runtime roots, and restores the pinned interpreter before production startup verification. Its first extension attempt omitted recreation of the intentionally excluded service lock after restore. That [fixture failure](evidence/m06-parent/review-docker-worker-before-fixture.json) remains retained. Running the actual worker check recreates the lock and verifies restored startup configuration before diagnostics.

The original reviewer verdicts remain unchanged. They reviewed the stated source, not later coordinator corrections. No additional independent approval is claimed for those corrections. Earlier coordinator changes also fixed the deadline test clock, added desktop proof, and rejected combined installation roots. [Issue 32](https://github.com/ImNallen/Botstral/issues/32) and [issue 33](https://github.com/ImNallen/Botstral/issues/33) retain the earlier recovery and installation defects.

## Final verification conditions

`RUST_TEST_THREADS=2 make verify` passed. The [repository log](evidence/m06-parent/review-core.log) includes formatting, Clippy, default and feature-enabled tests, 14 packaging tests, six stdio contract checks, clean production release builds, and both nonproduction release refusals.

`RUST_TEST_THREADS=2 make verify-desktop` passed after correction, including native journal contracts and the debug application build. The [desktop log](evidence/m06-parent/review-desktop.log) retains the full result. Actual manager [local](evidence/m06-parent/review-owner-local.json) and [owner WSS](evidence/m06-parent/review-owner-wss.json) connector administration checks also passed with the updated binary.

The first unrestricted full checks ran alongside image builds and failed several wall-clock admission, phase, and native sign-in deadlines. Those [repository](evidence/m06-parent/review-core-concurrent-failed.log) and [desktop](evidence/m06-parent/review-desktop-concurrent-failed.log) failures remain retained. A second repository run overlapped desktop verification, which rebuilt the root manager executable while feature-enabled transport tests needed its nonproduction counterpart. That [shared-target failure](evidence/m06-parent/review-core-shared-target-failed.log) remains retained. The coordinator then ran repository verification separately with two test threads, and it passed. No production source change was made for these reruns.

## Repeat the focused checks

```sh
cargo test --locked -p botstral-manager --lib manager::state::connectors -- --nocapture
cargo test --locked -p botstral-agent --lib generated_configuration_loads -- --nocapture
python3 -m unittest discover -s manager/packaging -p 'test_*.py' -v
```

The original probes are retained in [Astra's probe directory](m06-reviews/astra-probes/) and [Sol's patch](m06-reviews/sol-repros.patch). They target the original reviewed source. In a disposable checkout of that source, copy Astra's files into `.audit` and apply its test-hook patch before running its scripts. Sol's patch contains its test hooks and probes. Do not apply these original probes to a working deployment.

The three temporary logs referenced by the original canonical decision trail are now also [retained with byte digests](evidence/m06-parent/retained-local-evidence.json). The historical trail keeps its original paths.

The reviewers inspected the retained live evidence and the available coordinator transcript. They found no demonstrated fabricated verification. Their limits remain explicit in the individual reports. Podman, Docker x86_64, a replacement-machine cutover, and paid inference were not verified.
