# W01 process verification record

The first production process unit is implemented for parent review. Full W01 remains open. No pull request exists. The implementation branch is `t3/w01-process-runner`.

The baseline is `228641fa8525ebafa292ed8afae6311aac206e20`. The implementation preserves the parent-owned design and acceptance files. The only sequencing adjustment combines the supervisor and lifecycle integration into one verified code commit. Its rationale is recorded in [Approved production processes](w01-process.md).

## Verified behavior

| Gate | Observation | Evidence |
| --- | --- | --- |
| Pre-change behavior | The production manager returned HTTP 400 `invalid_json` instead of accepting the process task. | [Baseline log](../agent/tests/evidence/w01/pre-change.log) |
| Approved execution | The production binaries wrote the exact expected file and byte output. Literal arguments, working directory, and explicit environment matched. | [Linux success](../agent/tests/evidence/w01/linux/success.json) |
| Authority | Disabled execution left valid approval unconsumed. Changed arguments conflicted. Expired and revoked grants did not execute. | [Capacity and authority](../agent/tests/evidence/w01/linux/capacity-and-authority.json) |
| Descendants | Cancellation stopped the parent and new-session descendant that ignored termination. The task retained observed output. | [Cancellation](../agent/tests/evidence/w01/linux/cancel-descendants.json) |
| Normal completion | A parent exited with a surviving descendant. Completion waited for descendant cleanup. | [Normal exit](../agent/tests/evidence/w01/linux/normal-exit-descendant.json) |
| Ownership | Control confirmation arrived after both process identities stopped. | [Control stop](../agent/tests/evidence/w01/linux/control-stop.json) |
| Bounds | Both streams produced 4 MiB. Each retained exactly 1 KiB with truncation. One active process held the next approval unconsumed. | [Output](../agent/tests/evidence/w01/linux/bounded-output.json) |
| Exit and timeout | Exit code 17 produced failure. Runtime expiry produced failure with an explicit limit reason after verified stopping. | [Nonzero exit](../agent/tests/evidence/w01/linux/nonzero.json), [Runtime limit](../agent/tests/evidence/w01/linux/runtime-limit.json) |
| Reconnect and crash | Manager restart, daemon crash, and supported shutdown preserved the command identity and one-entry invocation ledger. | [Manager restart](../agent/tests/evidence/w01/linux/manager-restart.json), [Daemon crash](../agent/tests/evidence/w01/linux/daemon-crash.json), [Shutdown](../agent/tests/evidence/w01/linux/shutdown.json) |
| Independent supervision | After killing the daemon, systemd removed both ordinary processes within the runtime and stop bounds. Restart retained uncertainty because stop proof was unavailable. | [Independent runtime](../agent/tests/evidence/w01/linux/independent-runtime.json) |
| Crash boundaries | Crashes before start submission and after startup produced zero guest invocations. A crash before result import produced one invocation. Recovery never resubmitted or released. | [Launch intent](../agent/tests/evidence/w01/linux-debug/process_launch_intent.json), [Startup](../agent/tests/evidence/w01/linux-debug/process_start_observed.json), [Result import](../agent/tests/evidence/w01/linux-debug/process_result_observed.json) |
| Capacity regression | The previous release had no remaining reservation after completion. The fixed fixture observed one slot for a racing cancellation. | [Before](../agent/tests/evidence/w01/reservation-before.log), [After](../agent/tests/evidence/w01/reservation-after.log) |
| Journal replacement | The previous release wrote an unexpected replacement effect while the old scope ran. The fix blocked the effect and refused false cancellation evidence. | [Before](../agent/tests/evidence/w01/missing-scope-before.log), [After](../agent/tests/evidence/w01/missing-scope-after.log), [Final observation](../agent/tests/evidence/w01/linux/journal-replacement.json) |
| Unsupported host | The macOS release daemon remained ready but created no execution and did not consume approved authority. | [Unsupported host](../agent/tests/evidence/w01/unsupported-host.log) |
| Compatibility | The full workspace verifier, generated bindings, desktop tests, desktop local integration, and desktop build passed. Both forbidden release features were rejected. | [macOS verifier](../agent/tests/evidence/w01/verify-macos.log), [Linux verifier](../agent/tests/evidence/w01/verify-linux.log), [Desktop](../agent/tests/evidence/w01/verify-desktop.log), [Bindings](../agent/tests/evidence/w01/bindings.log) |

## Commands and environment

The local host runs Darwin 27.0.0 arm64, Rust 1.93.0, Cargo 1.93.0, Node 26.12.1, and npm 11.20.0. Both guests run Ubuntu with kernel `6.8.0-147-generic` and systemd `255.4-1ubuntu8.18`. The Computer VM uses Rust 1.93.0. The compiler was absent at the supplied Manager VM path, so Linux binaries were built on the Computer VM and copied into isolated fixture state.

The recorded commands are:

```sh
make verify
make verify-desktop
npm run bindings:check
./scripts/verify-w01.sh /home/botstral/w01-570e76c4/final-evidence
```

The baseline fixture at `ac9a35f` used the default release manager built from the baseline production source:

```sh
cargo build --workspace --release --locked
BOTSTRAL_MANAGER_BIN="$PWD/target/release/botstral-manager" \
cargo test -p botstral-agent --locked --test m02_transport \
  w01_production_contract -- --ignored --nocapture
```

The capacity and replacement-journal negative controls used the earlier `b7b6f71` release binaries with the strengthened fixture. They failed on missing reserved capacity and an unexpected replacement effect, respectively. The final release passes both assertions. The macOS refusal check used:

```sh
BOTSTRAL_MANAGER_BIN="$PWD/target/release/botstral-manager" \
BOTSTRAL_PROCESS_AGENT_BIN="$PWD/target/release/botstral-agent" \
cargo test -p botstral-agent --locked --test m02_transport \
  w01_unsupported_host -- --ignored --nocapture
```

The final scoped Linux run also passed `cargo clippy --workspace --all-targets --locked -- -D warnings`, all five agent library tests, all three release process tests, and the debug crash-boundary test. [The retained log](../agent/tests/evidence/w01/final-linux.log) includes every result. The complete Linux verifier was then repeated against the same source.

The full Linux verifier runs through strict-host-key SSH in `/home/botstral/w01-570e76c4/source`, with `/home/botstral/.cargo/bin` added to `PATH`. The production tests select default release binaries through `BOTSTRAL_MANAGER_BIN` and `BOTSTRAL_PROCESS_AGENT_BIN`. These variables contain public binary paths. Credentials never use arguments or environment variables.

The final two-machine command was:

```sh
python3 agent/tests/w01_two_vm.py \
  --base /home/botstral/w01-570e76c4 \
  --cert-dir .audit/w01-tls/certificates \
  --output-dir .audit/two-vm-current
```

[Its result](../agent/tests/evidence/w01/two-vm/results.json) records actual binary hashes, separate machine fingerprints, process identities before and after stopping, and 21 passed checks. [The rerun instructions](w01-process-acceptance.md#run-across-two-existing-machines) describe binary and certificate setup. The setup listener stayed on manager loopback. The fixture did not reboot a machine, change privileges, or alter permanent owner state.

[Source hashes](../agent/tests/evidence/w01/source-files.json) confirm that all 22 changed source and fixture files match the Linux build tree. [Cleanup evidence](../agent/tests/evidence/w01/cleanup.json) records removal of four failed units from early attempts. No matching fixture units remained on either guest.

## Limits and review boundary

The encoded maximum-output report measured 4,459 bytes in the protocol test. The test also validates a 2,048-byte specification, report pages, inventory envelopes, and a complete retained task snapshot. The snapshot fixture measured 1,402,881 bytes for 128 records. These are serialization checks, not throughput or capacity benchmarks.

Fault injection uses the existing debug-only feature. Default release binaries separately pass production execution, cancellation, limits, shutdown, restart, and uncatchable-crash tests. The supervisor requires exactly systemd 255. Other systemd versions, host reboot recovery, hostile same-account isolation, executable-byte pinning, and atomic exactly-once effects are not claimed.

Unknown stopping remains pending or requires attention. Journal loss can block future process claims. There is no automatic bypass of that uncertainty in this unit. Independent comment review, adversarial review, and final acceptance remain with the parent. The implementation owner did not count its own inspection as independent review.

The evidence manifest identifies the tested code commit. The following documentation and evidence commit changes no production source.

The canonical decision trail is [the implementation log](../.audit/w01-implementation.tsv). [The evidence manifest](../agent/tests/evidence/w01/manifest.json) records the source commit and artifact hashes.
