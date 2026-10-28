M06 must let the owner install connections once, grant tools independently, stop uncertain work, and restore usable credentials and worker state.

Verdict **ISSUES**. Reviewed `4483b183671aec5bc36a65fa11c25308fb127423..48bf1bb2cc917499a6889f24519426f5f0666755`. The checkout remains at that exact head. Three defects were reproduced. No production fixes, external service operations, commits, pushes, or nested agents were made.

All commands below run from `/Users/nallen/.t3/worktrees/Botstral/t3-ce9380b6`. The two tracked modifications are test-only includes. `.audit/m06-astra-test-hooks.patch` preserves them for an independent checkout. Copy the accompanying `.audit/m06-astra-*` probe files and apply that patch before rerunning elsewhere.

1. **P1. A small discovered schema blocks the sole manager State writer.**

Location `manager/src/manager/connectors.rs:262-267`. The synchronous State caller is `manager/src/manager/state/connectors.rs:583-585`.

Command:

```sh
python3 .audit/m06-astra-schema-timeout.py
```

The script compiles before starting the timeout. It runs the same actual discovery settlement, grant creation, and `State::submit_task` path with a simple control and a shallow reference graph. Each graph node has two `allOf` references to its predecessor. The schema stays inside the byte, JSON depth, and member-count bounds.

Observed output for the control includes `exit: 0` and `task submission returned`. The adversarial case prints:

```text
Schema reference depth 32
TIMEOUT after 10 seconds inside test; killed only probe process group

running 1 test
schema bytes: 2090
catalog persisted and exact grant created; submitting empty arguments
```

Required behavior is bounded rejection or bounded evaluation of the schema. A granted connector's schema must not monopolize State when validating `{}`. The current check bounds regex backtracking but not repeated local references. Accepted schema references expand validation work combinatorially. `prepare_task` invokes this synchronous validation on the single State writer, before task execution or its transport timeout. Other authority and settlement work cannot run while that writer is occupied. The observed stall is ten seconds, not a claimed eventual runtime.

The probe is `.audit/m06-astra-probes.rs`. Logs are `.audit/m06-astra-schema.log` and `.audit/m06-astra-schema-depth-32.log`. The script kills only its isolated test process group. An earlier direct self-reference probe completed successfully and was rejected as a finding.

2. **P2. The documented connector configuration generator produces a configuration the worker rejects.**

Location `agent/connector/configure.py:20`. The rejecting paths are `agent/src/connector.rs:29` and `link/src/process.rs:31-34`.

Command:

```sh
python3 .audit/m06-astra-configure.py
```

This runs the actual generator against the installed Python executable, writes its untouched output into a private fixture, and loads it through production `connector::Configuration::read`.

```text
Generated ceilings: {'runtime_seconds': 60, 'output_bytes': 8192, 'processes': 32, 'memory_bytes': 536870912}
Production worker configuration accepted: false
Production worker configuration error: InvalidMessage
```

Required output is `Production worker configuration accepted: true` for the documented setup command. Generated output permits 8192 output bytes and 512 MiB of memory. `ProcessLimits::validate` permits at most 1024 output bytes and 256 MiB. The worker propagates this error during process configuration, so following `docs/m06-connectors.md` prevents startup with connectors enabled.

The integrated acceptance driver writes its own valid configuration at `manager/tests/m06_remote.py:133-137`. Its successful live run does not exercise this generator. This is a missing setup gate, not fabricated runtime evidence. The log is `.audit/m06-astra-configure.log`.

3. **P2. Recovery and diagnostics omit the pinned connector interpreter.**

Locations `manager/packaging/recovery.py:229` and `manager/packaging/diagnose.py:34`.

Command:

```sh
python3 .audit/m06-astra-recovery.py
```

The fixture first passes the actual worker configuration verifier. It then changes the pinned interpreter bytes, creates and verifies a bundle without selecting that external interpreter root, retires its temporary originals, and restores at the original state path.

```text
Connector Python included in references: False
Production worker configuration accepted: true
Configured interpreter digest matches: False
Production runtime diagnostics returned: []
Production worker configuration accepted: false
Production worker configuration error: connector runtime changed
Backup with no interpreter root verifies: True
Restored configured interpreter exists: False
Restored runtime diagnostics returned: []
Production worker configuration accepted: false
Production worker configuration error: No such file or directory (os error 2)
```

Required behavior is to include the connector interpreter in configured-runtime coverage and reject missing or changed bytes in diagnostics. Both maintenance functions enumerate only `computer.json` and `vibe.json`. They ignore the new `connector.json`, although worker startup requires its interpreter and exact hash. A successful restore therefore leaves a configuration that production startup rejects. This reproduces with a private configured interpreter, so the documented exclusion of operating-system packages does not explain it.

Only `worker_quiet` is substituted because this review runs on macOS. Reference selection, copying, manifest verification, original-path restoration, diagnostics, and the Rust configuration verifier are production code. No Linux quiescence claim follows from this probe. The log is `.audit/m06-astra-recovery.log`. Both Python probes use `.audit/m06-astra-worker-probe.rs`.

The following hypotheses were rejected by executed checks.

- Stale grants, rotated credentials, repeated local release, lost settlement, restart, late evidence resuming a parent, forged receipts, and Vibe authority drift did not reproduce. `cargo test -p botstral-manager --lib manager::state::connectors::tests -- --nocapture` passed all 17 existing connector tests before probe additions. See `.audit/m06-astra-authority.log`.
- The tested HTTP correlation, JSON and SSE paths, server sampling requests, changed catalogs, authentication failure, and revocation before transmission behaved as specified. `cargo test -p botstral-manager --lib manager::connectors::http::tests -- --nocapture` passed. See `.audit/m06-astra-http.log`.
- The stdio environment, redaction, deadline, revocation, lost-permit, and saved-settlement cases passed all six checks in `python3 agent/tests/m06_connector_contract.py`.
- Protocol negotiation, generated schema consistency, exact argument binding, and canonical digests passed all nine checks in `cargo test -p botstral-link --lib -- --nocapture`. See `.audit/m06-astra-link.log`.
- Missing summary budgets did not reproduce. `cargo test -p botstral-manager --test m05_bounds -- --nocapture` passed. The serialized snapshot bound is 64350208 bytes and the allocation bound is 67338240 bytes. `link/src/owner.rs` includes `MAX_CONNECTIONS` in both bounds and derives workspace bounds from them.

The canonical decision trail was checked against its local evidence and accessible parent history. `python3 .audit/m06-astra-evidence.py` confirms that all referenced evidence files exist, transferred executable inputs match the reviewed head, and the integrated workspace log contains 125 passed tests, zero failed tests, and 12 ignored tests. The baseline and integration temporary logs are still present and passing. See `.audit/m06-astra-evidence.log`.

I read messages and relevant tool activity in [Find Next Step in Poteto Mode](t3-thread://v1/07fd9f8d-5fc7-4247-abdc-064382a46489). Parent transcript access succeeded. I inspected the integrated driver, runtime preparer, recorded-provider boundary, SDK server, deployment and source manifests, recorded gates, failed-attempt records, and packaging evidence. I found no demonstrated fabrication or invented user decision. The live report's claims remain narrower than full milestone closure. The configuration generator and connector-runtime maintenance coverage are the omitted gates demonstrated above. The already documented Podman, Docker x86_64, native activation, retained-worker, and independent-installation limits are not new defects.

Prove It Works required runnable disagreements rather than source-only suspicions. Model the Domain shaped the shallow schema-reference graph and the separate configuration-versus-restoration probes. The parent must independently rerun accepted findings.
