# First acceptance

Eight of the nine first acceptance checks passed on October 28, 2026. Helper inference routing remains unverified. The milestone stays open, and the full driver returns a nonzero status for that gap.

The retained acceptance workflow used one synthetic owner, bot, and worker on separate Ubuntu x86_64 guests. It used default production release binaries, the native macOS desktop service and operating system keychain, an official SDK connector over authenticated HTTPS, and live `mistral-small-2603` inference at the EU endpoint. Earlier failed attempts used default-feature debug binaries before the release-build correction. The connector wrote controlled test receipts. It did not connect a personal external account.

## Observed checks

The machine-readable [evidence](evidence/first-acceptance/evidence.json) retains the task values, approvals, worker journals, model requests, artifacts, and results behind these observations.

| Checklist item | Result |
| --- | --- |
| Separate VMs and worker authority. | Verified. Worker credentials could not approve tasks, enroll owner devices, or read provider credentials. |
| Native browser, file, exact approval, result, and cancellation. | Verified. The native service proposed and approved the requested browser operations and file write. Exported text matched its content digest. Cancellation left an empty process scope. |
| Real connector and closed-client P03 routine. | Verified. Live inference proposed the exact connector action. Both owner clients were suspended during execution. Repeated delivery of the same authenticated publisher event retained the original occurrence, run, and one external effect. |
| Manager restart during execution. | Verified. Startup locked the vault. Unlock and reconnect retained the original task and journal. The worker stopped its original scope after connection loss and required attention before owner cancellation. It did not launch replacement work. |
| Worker disconnect after an external effect. | Verified after the [unknown-effect correction](connector-unknown-recovery.md). Pausing the worker caused an actual idle WebSocket disconnect. Recovery retained the original execution, stop proof, unknown ledger outcome, and one counted effect. The task required attention. |
| Client gap beyond replay retention. | Verified. The native service disconnected at cursor 615 and recovered through a snapshot at cursor 936. All 160 test messages were present. |
| Denied, altered, expired, consumed, and revoked authority. | Verified. Each stale request was refused. The denied, altered, expired, and revoked jobs wrote no files. The consumed job completed once and rejected stale reapproval. |
| Manager and Computer backup restoration. | Verified. Both original instances were stopped and retired before restoring original state paths. History, Markdown memory, encrypted credentials, artifact bytes, worker journal, certificates, tools, browser runtime, and selected workspace bytes survived. A new authenticated connector task and provider catalog verification worked without credential reentry. |
| Model, helper, privacy, and capability policy. | Partly verified. All five requests in the retained workflow used the configured Mistral-developed model and EU endpoint. Missing privacy acknowledgment and unconsented latest or preview identifiers were refused. Unsupported execution failed and appeared in the native workspace. No helper path was configured or invoked. |

The graphical app paired through a protected invitation and displayed the retained routine and browser results. Its [before and after captures](connector-unknown-recovery.md#desktop-evidence) show the original unresolved connector task. Browser, approval, cancellation, export, snapshot, and restore assertions used the real native service driver. They do not claim every control was clicked in the graphical app.

## Source and spending

Initial workflows ran at `b89321e`. The corrected manager was built at `0383c96`; its release digest is `e3128606efbbe1edae020daed2ceb2870feb8f012cb9888ffb25d186c0cf0913`. The worker and desktop sources were unchanged. The evidence records the exact changed manager paths and compiler artifact. The live disconnect was repeated after the execution-state correction. The final deletion-only correction has its own failing-before regression. Both backup restores ran with the final corrected manager.

The [attempt manifest](evidence/first-acceptance/attempts.json) retains hashes and outcomes for all 20 checkpoints, including failed driver attempts and review-discovered defects. Earlier checks carried forward only after they had an explicit verified verdict. The [decision trail](first-acceptance-decisions.tsv) records the corrections. Original private reports and source snapshots remain local under `.audit/first-acceptance/`.

The separate [campaign ledger](evidence/first-acceptance/budget.json) retained US$0.958482 under an unchanged US$1 cap. This is conservative reserved exposure, not an invoice or measured charge. Every actual or uncertain model attempt retained its ceiling. Unused reservations were released only against stopped fixture databases. The final continuations made no new inference requests. The earlier M04 campaign was not reset or reused.

All seven reconciliation evidence hashes resolve to the [preserved byte snapshots](evidence/first-acceptance/budget-snapshots/index.json). Six snapshots were recovered by removing only the subsequently appended `reconciled_budget` field from the retained report and reproducing its original JSON serialization. Their bytes match the immutable ledger hashes. Ledger history was not rewritten. The driver now saves these bytes before reconciliation.

## Restoration and cleanup

Backup initially refused stale exited diagnostic units. Their recorded main process identifiers and cgroups were empty. Only their transient unit state was cleared. Backup also refused restoration beneath the nonprivate home directory. The test workspace was moved beneath the private fixture root, with its proof digest unchanged. The home directory permissions were not changed. The selected workspace was then backed up and restored at that new path. This does not prove restoration of the original short workspace path.

The [manager cleanup](evidence/first-acceptance/manager-cleanup.json) and [Computer cleanup](evidence/first-acceptance/computer-cleanup.json) record process absence, state locks, database digests, and removal of owned fixture state and backups. The temporary provider-key file and consumed invitations were removed. No provider key was found in retained evidence. The graphical test profile signed out, its named keychain entry was absent, and the app exited. Installed services, system trust, and shared runtimes were preserved.

Resumed cleanup lacked the plaintext comparison key, so its remote byte scan is unverified. Absence of that temporary file does not prove an empty vault or a successful scan. The final local scan covered all retained evidence before the key was removed. The driver now labels missing comparison keys `NOT SCANNED` and continues remote cleanup even when native termination times out.

## Local verification and review

`RUST_TEST_THREADS=2 make verify` exited successfully. It checked formatting, Clippy, builds, packaging, connector contracts, workspace tests, production release binaries, and refusal of the nonproduction worker feature in release builds. The [complete log](evidence/first-acceptance/final-verify.log) records ignored environment-specific tests separately.

Three [tool regressions](evidence/first-acceptance/harness-regressions.log) verify cleanup after native timeout, an explicit unscanned result without a comparison key, and immutable budget evidence. All three [failed against the frozen earlier driver](evidence/first-acceptance/harness-regressions-before.log) and pass after correction. The repository verification script now runs them. Python compilation and the connector server's Node syntax check also passed.

The [independent product review](evidence/first-acceptance/issue39-review.md) approved the corrected execution lifecycle. The [evidence audit](evidence/first-acceptance/evidence-audit.md) required the restored accounting snapshots and appended trail corrections recorded here. The [harness review](evidence/first-acceptance/harness-final-review.md) reproduced the cleanup interruption and missing scan coverage. Those two findings are corrected and covered by the tool regressions. These reviews do not close helper routing.

## Repeat the run

Use disposable Linux guests with the existing SSH aliases, Rust 1.93.0, a systemd user manager, and the browser runtime described in `manager/tests/m06_integrated.py`. Use a private provider-key file supplied through the secret handoff. Do not put its contents in the command line.

```sh
python3 -B manager/tests/m06_integrated.py --prepare --output .audit/acceptance-build
cargo build --manifest-path desktop/src-tauri/Cargo.toml --no-default-features --locked --example c02-remote-driver
python3 -B manager/tests/first_acceptance.py \
  --build .audit/acceptance-build/deployment.json \
  --output .audit/acceptance-run \
  --native desktop/src-tauri/target/debug/examples/c02-remote-driver \
  --key-file /absolute/private/provider-key \
  --campaign-directory /home/botstral/first-acceptance-campaign
```

The driver reserves inference before provisioning and preserves the campaign cap across retries. `--resume` continues a stopped fixture from its recorded checkpoint. `--suite p03` runs only the P03 portion. A full run deliberately remains incomplete until helper routing can be exercised. Fixture shutdown removes temporary plaintext provider material; the operator must remove retained private fixture states and backups after collecting evidence.

Replacement-machine cutover, helper inference, additional bots, mobile clients, and release readiness are outside this evidence.
