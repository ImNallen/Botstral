# M05 corrected-source acceptance

Final corrected-source acceptance passed on 2026-10-27. All 15 local gates, both native correction checks, all 7 two-VM gates, both Linux production compatibility lanes, and the requested repository and desktop checks passed. The evidence validator passed with all five negative controls.

This verification ran in `/Users/nallen/.t3/worktrees/Botstral/t3-06c17af9` on `t3/m05-final-acceptance-evidence`. The parent owns evidence review, PR creation, and integration. This agent created no PR, push, merge, or watch and used no subagents.

## Source and artifact identities

The observed source head was `3a4aecdea15c1d1294cdff5ee42498cd8da97a4b`. It includes the independently approved production correction `ef2b09e291eeef0a02c5639341dd7a426927fbce`. All 12 production objects match that frozen correction. The final evidence and documentation commit changes none of them.

The archive SHA-256 is `ced3fc59593a44572e074f98078a03007bd3a4153ccc0241006413fd72afe33e`. The archive came from `git archive --format=tar` at the observed source head, compressed with Python `gzip.compress(..., mtime=0)`. The Computer VM verified its hash before extraction. The runner's actual `source_head` remains unchanged in the remote summary.

[The source manifest](../manager/tests/evidence/m05/m05-source-manifest.json) records all 12 exact Git object hashes, archive provenance, and Linux binary identities.

| Linux artifact | SHA-256 |
| --- | --- |
| Manager | `e6d76bd1e3be3d97cd6730a5a5f1f6c28778e2be2126a2c4d4051360e3f135c0` |
| Native owner probe | `e2d1a23db624ce4e2862833b919166937fc54a0526ef43603a09abf51fa7b9bf` |
| New production worker | `4d97040727389302a7576c1f1d7f4b39f5ebee87f482a1f14bf46cba604069e5` |
| Retained M04 worker | `a1dffc665e7eb6176a53fe5f325a0874f20f4ea526e9eb23b7a24ba412365a00` |
| bore v0.6.0 | `60548b7a145ba334981b19fda7cd0210d24a108a3d0dc113919b33fd2eaa90ab` |

The retained worker, bore executable, and bore archive hashes match [the initial manifest](../manager/tests/evidence/m05/initial/m05-source-manifest.json). Their original files were read without modification. The bore archive hash remains `e484d1e3acba77169b773f31a5bfb34192d4b660f44a094a658a2522cd2270f7`. The retained worker's earlier associated source is `2c9a4356289538216ec45e5dcf9283d1f9785391`.

[The final local identity record](../manager/tests/evidence/m05/m05-final-local-identities.json) records the macOS binaries and contract executable. An explicit default workspace build preceded immutable binary copies. Both final local runners used those copies. Post-run hashes match the local acceptance summary.

## Executed checks

Every command below exited zero. [The command record](../manager/tests/evidence/m05/m05-commands.json) retains exact invocations, timestamps, environment, source root, and exit codes. Complete sanitized logs remain beside it.

| Check | Observed result | Durable evidence |
| --- | --- | --- |
| Parent macOS `scripts/verify.sh` | Passed. Copied byte-for-byte from the parent-provided log. The parent's handoff reports exit zero. This agent did not run in the parent checkout. | [Log](../manager/tests/evidence/m05/verify-final.log), [provenance](../manager/tests/evidence/m05/m05-parent-verification-provenance.json). |
| Linux `scripts/verify.sh`, then `cargo build --workspace --locked` | Passed formatting, Clippy, default and test-feature builds and tests, release build, and both forbidden test-feature release checks. The final explicit build restored default production binaries before collection. | [Linux log](../manager/tests/evidence/m05/m05-linux-verify.log). |
| `python3 manager/tests/m05_acceptance.py --manager-bin .audit/macos-default-bin/botstral-manager --probe-bin .audit/macos-default-bin/owner-probe --agent-bin .audit/macos-default-bin/botstral-agent --output .audit/m05-local-source-final` | All 15 exact named checks passed. | [Summary](../manager/tests/evidence/m05/m05-local-source-final/summary.json), [log](../manager/tests/evidence/m05/m05-local-source-final.log). |
| `cargo build -p botstral-manager --locked --bins --example m05-client-contract`, then `python3 manager/tests/m05_client_regressions.py --binaries .audit/macos-default-bin --output .audit/m05-client-corrections` | Both native correction checks passed. The run exercised session admission, retained directory locks, cross-client recovery refusal, committed unknown outcomes, and transient TLS recovery without pin replacement. | [Build](../manager/tests/evidence/m05/m05-client-contract-build.log), [run](../manager/tests/evidence/m05/m05-client-corrections.log), [contract and TLS logs](../manager/tests/evidence/m05/review/client-corrections/summary.json). |
| `cargo test -p botstral-manager --locked -- --nocapture` | Complete Manager suite passed. The library had 62 passed and 2 environment-specific ignored tests. Both required State and provider-concurrency regressions passed. Snapshot bounds and actual-binary local tests also passed. | [Manager log](../manager/tests/evidence/m05/m05-manager-suite.log). |
| `python3 manager/tests/m05_two_vm.py --binaries .audit/linux-bin --legacy-agent-bin .audit/linux-bin/m04-botstral-agent --bore-bin .audit/bore-linux --output .audit/m05-two-vm` | All 7 exact named gates passed through real LAN and bore.pub traffic. | [Summary](../manager/tests/evidence/m05/m05-two-vm/summary.json), [log](../manager/tests/evidence/m05/m05-two-vm.log). |
| `cargo test -p botstral-manager --locked --lib m04_coding_production_worker_pinned_vibe -- --ignored --nocapture` on Linux | Recorded-provider production coding compatibility passed against final Manager source and the retained worker. | [Coding log](../manager/tests/evidence/m05/m05-coding-legacy.log). |
| `cargo test -p botstral-manager --locked --lib m04_run_production_worker_browser_file_process_and_artifact -- --ignored --nocapture` on Linux | Recorded-provider browser, file, process, and artifact compatibility passed against final Manager source and the retained worker. | [Browser and process log](../manager/tests/evidence/m05/m05-browser-file-process-artifact.log). |
| `npm run check`, `npm test`, `npm run build` | Rust schema and generated TypeScript bindings matched. TypeScript checks, all 17 desktop tests, and the desktop build passed. | [Check](../manager/tests/evidence/m05/desktop-final-check.log), [tests](../manager/tests/evidence/m05/desktop-final-tests.log), [build](../manager/tests/evidence/m05/desktop-final-build.log). |
| `cargo test --manifest-path desktop/src-tauri/Cargo.toml --no-default-features --locked` | Existing native desktop protocol tests passed. | [Protocol log](../manager/tests/evidence/m05/desktop-native-protocol.log). |
| `cargo test --manifest-path desktop/src-tauri/Cargo.toml --no-default-features --locked --test local actual_manager_owner_bot_workers_and_sign_out -- --ignored --test-threads=1 --nocapture` | Existing native desktop service passed against the actual final-source Manager. | [Actual-Manager log](../manager/tests/evidence/m05/desktop-actual-manager.log). |
| `python3 manager/tests/m05_record.py --check-controls` | Acceptance, cleanup, hashes, and committed production objects passed. Changed logs, false acceptance, changed source, replaced gate names, and missing endpoint proof were each rejected. | [Precommit validator log](../manager/tests/evidence/m05/m05-record-precommit.log). |

The complete Manager suite includes `rejected_handshakes_cannot_lock_out_valid_owner_or_worker_credentials` and `m05_provider_verification_keeps_other_owner_operations_available`. Both also passed in Linux repository verification.

Both Linux production lanes verified these exact installed paths before use:

- `BOTSTRAL_RUN_AGENT_BIN=/home/botstral/m04-final-acceptance.GycLnu/bin/production/botstral-agent`.
- `BOTSTRAL_VIBE_RUNTIME=/home/botstral/m04-vibe-2.24.2`.
- `BOTSTRAL_RUN_RUNTIME=/home/botstral/w01-runtime-76b03bf1`.

The lanes used unique private temporary directories under `/home/botstral/m05-final-4a0df3ffd5d6`. Existing recorded localhost provider fixtures supplied synthetic responses. No paid inference or real provider credentials were needed.

[Software versions](../manager/tests/evidence/m05/m05-software-versions.json) record Rust and Cargo 1.93.0, Vibe 2.24.2, systemd 255, Python, OpenSSL, Node, npm, operating systems, and Git. The Manager VM has no Rust installation. The Computer VM built the Linux binaries and both VMs ran the acceptance fixture.

## Observed remote route and lifecycle

The Computer VM at `192.168.1.235` connected directly to `192.168.1.40:36891` and externally to `bore.pub:45455`. The Manager VM at `192.168.1.40` ran a unique synthetic Manager and bore v0.6.0 transient unit. SSH transferred fixtures and collected evidence. Owner acceptance used actual LAN and public-relay TCP traffic.

The direct, Computer VM relay, and operator relay leaf observations all equaled `29b7bea21280dd2f5e5c0405dc47afdabcd73ff16921bc7c7f7008bf94001a51`. The relay forwarded opaque bytes and preserved pinned inner Manager TLS. Forwarded headers did not expose setup, local login, or device administration.

The native client recovered 160 messages at cursor 161 after Manager restart. Current owner revocation ended the active client with exit 1. Future access failed, including after another Manager restart. The retained worker stayed ready and reconnected independently. The successful watch exited zero. Manager and worker shutdowns exited zero. Bore ended on signal 15. [The remote summary](../manager/tests/evidence/m05/m05-two-vm/summary.json) retains every unit, PID, exit observation, application cursor, binary hash, and cleanup result. Sanitized process logs remain beside it.

## Cleanup

All seven remote fixture PIDs were absent and their units inactive after shutdown. The expected revoked-client failure unit was reset after its failure was recorded. Both explicit endpoint proofs are true, `lan_refuses_tcp` and `relay_refuses_tcp`.

Both VMs independently reported the acceptance root `/home/botstral/m05-fixture-0d5dd7d84514` absent. The separate build root `/home/botstral/m05-final-4a0df3ffd5d6` was deleted. No build-root process or unit remained before removal. Local acceptance removed its private fixture. The unchanged native-correction runner completed its cleanup block.

The installed worker, Vibe directory, and browser runtime remain present. The installed worker hash and browser runtime manifest hash match their pre-test observations. Existing failed or exited units from earlier work remain untouched. [The cleanup record](../manager/tests/evidence/m05/m05-cleanup.json), [inspection log](../manager/tests/evidence/m05/m05-build-cleanup-inspection.log), and [removal log](../manager/tests/evidence/m05/m05-cleanup.log) retain these observations. No credentials, private keys, raw fixture state, binaries, or archives are packaged as new evidence.

## Preserved diagnostics

The first local run passed all 15 checks while a concurrent default contract build replaced shared target-path binaries. Its post-run hashes exposed the provenance ambiguity. [That run](../manager/tests/evidence/m05/diagnostics/m05-local-default-rebuild-overlap/summary.json) and [its log](../manager/tests/evidence/m05/diagnostics/m05-local-default-rebuild-overlap.log) remain intact. Final acceptance and native corrections were repeated successfully against immutable copies from a fresh explicit default build.

An untracked software-version probe initially failed because shell quoting broke its Python expression. [The failed probe](../manager/tests/evidence/m05/diagnostics/m05-software-versions-initial-error.json) remains separate from the successful version record. An additional inventory spot-check excluded the nested historical `initial/files.json` by basename. [Its diagnostic](../manager/tests/evidence/m05/diagnostics/m05-inventory-diagnostic.json) records the failed assertion and successful corrected comparison. The actual package mapping and evidence validator were correct. No production, test-runner, or validator source changed.

## Historical evidence and remaining scope

[Initial evidence](../manager/tests/evidence/m05/initial/m05-source-manifest.json) belongs to production revision `e2547e132732f8b7ed9774e4f1c54c4e3f6da5fa`. Its successful runs and diagnostic failures remain historical. They do not establish acceptance for the corrected source. [Correction review evidence](m05-correction-review.md) and the existing review logs are preserved. Only the requested `review/client-corrections` records were replaced by this final successful run.

The [snapshot calculation](m05-snapshot-bound.md) remains 64,317,440 serialized bytes and 67,272,704 decoded bytes, with additional owner recovery allocation below 1 GiB. These are conservative calculated bounds. Runtime acceptance demonstrated complete chunked snapshots and oversized ordinary replies.

[Independent correction review](m05-correction-review.md) approved the frozen production source. This agent does not claim independent review of its own evidence. Parent inspection and integration remain pending. No observed source or acceptance blocker remains.

C02 still owns graphical pairing, native keychain integration, and notification display. Durable public hosting remains a separate operator choice documented in [remote owner access](remote-owner.md). Temporary bore forwarding was removed. A stolen device bearer credential can impersonate that device until revocation. Short-code pairing, another owner certificate authority, custom signature handshakes, and automatic online pin rotation remain outside M05.

The final file-manifest SHA-256 is `ea7b186e1b5b0e19f3b7958fc73d4be75610f88d9e555c3d17bd9681f35b975b`. [The top-level file manifest](../manager/tests/evidence/m05/files.json) maps every retained evidence file except itself to SHA-256, including all historical initial and review files. Prove It Works required actual binary traffic and verified cleanup. Show Me Your Work kept the decisions and preserved diagnostic attempts available for parent review.
