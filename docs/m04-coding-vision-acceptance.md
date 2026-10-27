# M04 coding and vision acceptance

These runners distinguish recorded-provider checks, real Vibe against a recorded endpoint, and paid production acceptance. [The completion record](m04-completion.md) maps observed gates to evidence, including the failed full reports and successful focused follow-up. The isolated implementation phase made no paid calls or provider-secret reads; live acceptance used the explicitly authorized private credential.

## Recorded checks

From the repository root:

```sh
scripts/verify-m04-coding-vision.sh
scripts/verify.sh
npm test
npm run build
```

The manager tests cover full-context admission before recorded upstream HTTP, helper and distinct retry requests, duplicate attempts, concurrent-call refusal, parent aggregate limits, unknown holds, cancellation, revoked authority, restart, worker identity, exact resume binding, and malformed request refusal. The shared provider tests cover bounded real Mistral usage-detail fields. The vision test decodes the actual HTTP data URL and compares its bytes with the authorized artifact; rejection cases cover ownership, digest, capability, billing, corruption, byte, pixel, and count bounds. Old text requests still decode.

A separate admission regression blocks the State queue, cancels the HTTP caller while its admission is pending, and verifies the resulting known-unsent hold is released without upstream HTTP.

The acceptance-ledger tests exercise idempotent retry, concurrent reservations, immutable prior allowance, cap exhaustion, and symlink refusal. These tests do not contact a provider.

## Actual Vibe contract

Use an installed, unmodified Vibe 2.24.2. This command uses a local recorded SSE endpoint and creates a unique temporary workspace:

```sh
python3 -B agent/tests/m04_vibe_contract.py \
  --vibe /absolute/vibe-2.24.2/bin/vibe \
  --report /absolute/evidence/vibe-contract.json
```

It verifies the exact file-tool set, keyless generic endpoint configuration, real edit, separate Python assertion, exact same-session resume, denied outside-path callback, unavailable bash, and usage-bearing SSE. The report records the version, executable and wrapper hashes, native identity, and resource observations. Linux reports also contain cgroup limits and peak memory. `--wire-report` optionally retains sanitized wire requests.

The production integration additionally starts the actual manager worker listener and release worker, enrolls with generated fixture certificates, and uses the worker's mTLS transport and Process supervisor:

```sh
cargo build -p botstral-agent --release --locked
BOTSTRAL_VIBE_RUNTIME=/home/botstral/m04-vibe-2.24.2 \
BOTSTRAL_RUN_AGENT_BIN="$PWD/target/release/botstral-agent" \
  cargo test -p botstral-manager --lib --locked \
  m04_coding_production_worker_pinned_vibe -- --ignored --nocapture
```

Run it as the non-root Linux guest account with a working systemd user manager. It proves no edit or call before approval, real edit, duplicate task suppression, explicit resume, settled coding calls, no provider key in worker logs, failed task after Vibe denies an outside-path edit despite CLI exit zero, and clean shutdown. Its provider requests go only to the recorded endpoint. It also observes the native CLI before cancellation, proves the scope empty, and verifies completed coding calls and exact artifact bytes after Manager restart. The final request stalls while the worker disconnects. Manager shutdown must drain within three seconds, and restart must retain the sent request as unknown and held without replacement.

The worker uses the existing ceilings: 256 MiB memory, 32 processes, and one KiB captured output. The test chooses 120 seconds. No global Process limit changed.

## Two-VM preparation

Prepare a unique remote base containing `bin/production/botstral-manager` on Manager VM and `bin/production/botstral-agent` on Computer VM. Build both release binaries from the same committed source in a unique source directory; do not overwrite shared builds or prior acceptance installations. Retain the source archive and its SHA-256 alongside the build logs. The runner records hashes of the current local source tree, both installed binaries, both runtime manifests, and the distinct machine identities.

The following example assumes the matching production binaries are already staged. `BASE` must select that new isolated base. The certificate directory contains synthetic test credentials, not provider credentials.

```sh
BASE=/home/botstral/m04-acceptance-UNIQUE
EVIDENCE="$PWD/.audit/m04-live-UNIQUE"
mkdir -m 700 -p "$EVIDENCE"
bash agent/tests/m02_certificates.sh "$EVIDENCE/tls" 192.168.1.40
scp botstral-computer:/home/botstral/w01-runtime-76b03bf1/runtime.json \
  "$EVIDENCE/browser-runtime.json"
python3 -B agent/tests/m04_two_vm.py \
  --base "$BASE" \
  --cert-dir "$EVIDENCE/tls/certificates" \
  --runtime-manifest "$EVIDENCE/browser-runtime.json" \
  --output-dir "$EVIDENCE/prepare"
```

Default aliases are `botstral-manager` and `botstral-computer`; the worker listener defaults to `192.168.1.40:43124`. Override `--manager-ssh`, `--worker-ssh`, and `--manager-address` when needed. The browser runtime defaults to `/home/botstral/w01-runtime-76b03bf1`, and Vibe to `/home/botstral/m04-vibe-2.24.2`.

Without `--live`, the runner creates fresh state, enrolls the worker, seals Vibe into a separate fixture manifest, starts and stops the production services, checks permissions and redaction, and emits `live_acceptance: false`. Every invocation creates a new fixture root under `BASE/runs/UUID`. Existing runtime installations are read only.

## Capped live acceptance

Run this lane after integration and independent review. The provider key file stays on Manager VM, must be a bounded regular file owned by the guest with mode `0600`, and is read only by the Manager-side helper. Its value is never returned to the local runner or Computer VM.

The live configuration pins EU `mistral-small-2603`, context 262,144, output 1,024, and acknowledged ceilings of 200,000 input and 800,000 output microdollars per million tokens. It verifies model capabilities through the production API. One call reserves 53,249 microdollars. The plan allows two reasoning calls, three edit calls, one resume call, one vision call, and one cancellation call: eight calls, reserving 425,992 microdollars.

A persistent Manager-side ledger reserves the entire plan before provisioning or paid inference. Its lock serializes concurrent runners. Repeating one run identity is idempotent; new runs require remaining allowance. Failure does not release reservations automatically. The ledger includes prior acceptance reservations, including unknown outcomes, and refuses changed caps or changed prior allowance. It is an acceptance control, not provider invoice reconciliation.

The eight prior acceptance attempts were bounded at 54,068 microdollars each. With that recorded campaign history, use 432,544 as the prior reservation, making the combined worst case 858,536 microdollars. If another paid attempt is performed, update the campaign accounting before invoking this runner. Do not reset the ledger to obtain another allowance.

```sh
python3 -B agent/tests/m04_two_vm.py \
  --base "$BASE" \
  --cert-dir "$EVIDENCE/tls/certificates" \
  --runtime-manifest "$EVIDENCE/browser-runtime.json" \
  --output-dir "$EVIDENCE/live" \
  --live --endpoint eu --cap-microusd 1000000 \
  --previous-reserved-microusd 432544 \
  --budget-ledger /home/botstral/PRIVATE-CAMPAIGN/m04-budget.json \
  --provider-key-file /home/botstral/PRIVATE-CAMPAIGN/provider-key
```

The ledger parent directory must already exist and be private. Live checks require browser input/click/text/screenshot work, a shared file outside coding directories, second-bot access, real reasoning with exact child approval, the random image-only six-digit challenge, Vibe edit, a separate approved Python test, same-session resume, owner download while the worker is stopped, confirmed coding cancellation, pre-call budget refusal, and unchanged artifacts/accounting after manager restart. Logs, task/call evidence, source/binary/runtime hashes, service exit evidence, and process cleanup are saved under the printed UUID directory.

A missing, failed, or merely prepared check cannot mark live acceptance passed. A budget stop is a failed acceptance result; the runner does not increase limits or retry the campaign.

## Reconcile a stopped fixture

When a fixture stops before using its planned calls, keep its database intact and reconcile only the unused allocations before starting another run:

```sh
python3 -B agent/tests/m04_reconcile_budget.py \
  --evidence /absolute/failed-run/results.json \
  --ledger /home/botstral/PRIVATE-CAMPAIGN/m04-budget.json
```

The tool requires successful shutdown and cleanup evidence. On Manager it takes the actual manager state lock, refuses a running manager, reads every durable inference attempt, and retains each attempt's full original reservation, including settled and unknown calls. It records the old allocation, retained attempts, database hashes, and evidence hash atomically under the campaign lock. The cap and prior allowance stay unchanged. Keep the reconciled fixture stopped. Missing or corrupt state cannot release an allocation.

The first live run observed one safely rejected model proposal in the first live run. Its single durable attempt retains 53,249 microdollars. Together with the immutable prior 432,544 and another full 425,992 plan, the campaign reserves 911,785 microdollars. This preserves the same US$1 cap. It does not erase or replay the rejected run.

## Observed implementation evidence

On 2026-10-27, `scripts/verify.sh` passed formatting, clippy, default and test-worker suites, release builds, and both release feature refusals. `scripts/verify-m04-coding-vision.sh` passed the recorded manager library suite with 56 tests and two Linux integration lanes ignored, the three acceptance-ledger tests, and generated-binding/TypeScript checks. `npm test` passed 17 desktop tests, and `npm run build` passed. The actual CLI contract passed on macOS and Linux without provider spending. The Linux contract ran under 256 MiB and 32-process limits and recorded an 82,722,816-byte cgroup peak. The production worker integration passed its real edit, resume, denied-tool failure, and shutdown checks. The stalled coding request initially exceeded the three-second manager shutdown bound. After registration and shared shutdown signaling, the same test drained within that bound and found the persisted unknown/held row before reopening State. The blocked-admission caller-cancellation regression also passed.

The two-VM preparation passed against separate Manager and Computer VMs, including private permissions, redaction, and clean shutdown. Its result explicitly reports `live_acceptance: false`. Its isolated base was `/home/botstral/m04-acceptance-566a8a7fffeb4d7ebd3d7e008f3bbb21`; the passing evidence UUID was `6dc6c98e-0b23-4aa1-81c5-a97cdef83b8d`. A first preparation attempt exposed import-created cache permissions; the helper now disables bytecode writes and establishes a private umask before local imports.

Observed live coding and vision checks passed in the retained full run before its cancellation failure. The final focused lifecycle run passed cancellation, pre-call refusal, reattached restart and clean shutdown. [The completion record](m04-completion.md) explains the combined evidence and remaining product scope.

## Focused lifecycle follow-up

Use the same live invocation with `--suite coding-lifecycle` to run cancellation, budget refusal and reattached restart in a fresh isolated fixture. It reserves one full 53,249-microdollar call before provider provisioning. It retains the same immutable prior allowance and all existing campaign reservations. Its passing report is scoped and deliberately keeps `live_acceptance: false`; it cannot stand in for browser, vision, editing or completed-artifact proof.

The observed follow-up reserved 965,034 microdollars in total. Its UUID is `5fc6971a-3c32-4d76-8f2a-5c165b6f8374`. Another one-call allowance would exceed this campaign's remaining cap. Keep the ledger intact and do not repeat paid acceptance under the existing authorization.
