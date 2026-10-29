# Verification boundaries

The focused memory suite passes 26 tests with no ignored tests. It is separate from the default manager library, which passes 137 tests and ignores five environment-dependent cases.

The native journal tests listed below are ignored by their default Rust invocation and executed separately by `verify-desktop.sh` through the paired-manager fixture. The local-manager sign-in test is also invoked explicitly. The C02 desktop manager fixture is invoked by those drivers. These runs do not establish packaged application interaction or a real operating-system keychain round trip.

The frontend test `parses actual-manager native projections at the renderer boundary` remains opt-in because `BOTSTRAL_C02_VIEW_OUTPUT` is absent. The conversational-memory component tests run separately and pass.

The following entries reproduce the exact default ignored names and reasons from the final logs. Linux production-worker, pinned Vibe, isolated virtual-machine, and real-keychain cases remain unexecuted.

| Log | Test | Reason |
| --- | --- | --- |
| repository | `process_acceptance::w01_crash_runtime_bound` | Verifies the independent runtime ceiling after an uncatchable daemon crash. |
| repository | `process_acceptance::w01_journal_replacement` | Verifies that a replacement journal cannot prove an old process scope empty. |
| repository | `process_acceptance::w01_production_contract` | Runs the opt-in production process contract on Linux with a systemd user manager. |
| repository | `process_acceptance::w01_review_interrupted_probe` | Verifies interrupted capability probes on Linux with the production daemon. |
| repository | `process_acceptance::w01_review_reconnect_confirmation` | Verifies reconnect confirmation waits for the previous process scope to stop. |
| repository | `process_acceptance::w01_unsupported_host` | Checks production capability refusal on an unsupported host. |
| repository | `manager::inference::tests::coding_production::m04_coding_production_worker_pinned_vibe` | Requires Linux systemd, BOTSTRAL_VIBE_RUNTIME=Vibe 2.24.2 and BOTSTRAL_RUN_AGENT_BIN=production agent. |
| repository | `manager::inference::tests::integrated_fixture::m06_integrated_remote_fixture` | Requires manager/tests/m06_integrated.py and isolated Linux VMs. |
| repository | `manager::inference::tests::run_tests::desktop_fixture::c02_desktop_fixture` | Starts the isolated C02 graphical fixture until its stop file appears. |
| repository | `manager::inference::tests::run_tests::production::m04_run_production_worker_browser_file_process_and_artifact` | Requires Linux x86_64, a systemd user manager, and BOTSTRAL_RUN_RUNTIME prepared by agent/runtime/setup.sh. |
| repository | `manager::inference::tests::run_tests::production_fixture::c02_production_remote_fixture` | Requires the isolated two-VM C02 production acceptance driver. |
| repository | `process_acceptance::w01_crash_boundaries` | Uses existing debug-only fault injection with real Linux production-agent processes. |
| desktop | `admitted_journal_must_remain_readable_after_restart` | Requires desktop/fixtures/paired_manager.py with its isolated actual manager. |
| desktop | `conversational_memory_and_obsolete_journal` | Requires desktop/fixtures/paired_manager.py --verify-memory. |
| desktop | `historical_approval_cannot_confirm_an_unknown_regrant` | Requires desktop/fixtures/paired_manager.py with its isolated actual manager. |
| desktop | `journal_read_failure_must_preserve_unknown_and_block_mutation` | Requires desktop/fixtures/paired_manager.py with its isolated actual manager. |
| desktop | `missing_journal_does_not_become_an_empty_paired_profile` | Requires desktop/fixtures/paired_manager.py with its isolated actual manager. |
| desktop | `observed_revocation_blocks_cached_activity_after_offline_restart` | Requires desktop/fixtures/paired_manager.py with its isolated actual manager. |
| desktop | `p03_routine_controls_and_crash_journal_recovery` | Requires desktop/fixtures/paired_manager.py with its isolated actual manager. |
| desktop | `readable_unknown_journal_blocks_mutation_without_replay` | Requires desktop/fixtures/paired_manager.py with its isolated actual manager. |
| desktop | `trimmed_run_history_does_not_strand_unknown_intents` | Requires desktop/fixtures/paired_manager.py with its isolated actual manager. |
| desktop | `actual_manager_owner_bot_workers_and_sign_out` | Build the manager, then run with --ignored --test-threads=1. |
| desktop | `actual_manager_native_keychain_run_approval_artifact_and_lifecycle` | Requires desktop/fixtures/paired_manager.py and the real unlocked OS keychain. |

The test-feature transport invocation intentionally filters `enrollment_tls_negotiation_and_lifecycle_use_actual_binaries`; the production workspace run executes that scenario. The feature build also refuses both nonproduction binaries in release mode. See the complete repository log for the separate invocations.
