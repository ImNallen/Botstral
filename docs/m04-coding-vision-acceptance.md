# M04 coding and vision acceptance

M04 remains open until the parent observes the live two-VM results and finishes independent review. These runners distinguish recorded-provider checks, real Vibe against a recorded endpoint, and paid production acceptance. Implementation work made no paid calls and did not read a provider secret.

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

Run it as the non-root Linux guest account with a working systemd user manager. It proves no edit or call before approval, real edit, duplicate task suppression, explicit resume, settled coding calls, no provider key in worker logs, failed task after Vibe denies an outside-path edit despite CLI exit zero, and clean shutdown. Its five provider requests go only to the recorded endpoint. The final request stalls while the worker disconnects. Manager shutdown must drain within three seconds, and restart must retain the sent request as unknown and held without replacement.

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

Only the parent runs this lane after integration and review. The provider key file stays on Manager VM, must be a bounded regular file owned by the guest with mode `0600`, and is read only by the Manager-side helper. Its value is never returned to the local runner or Computer VM.

The live configuration pins EU `mistral-small-2603`, context 262,144, output 1,024, and acknowledged ceilings of 200,000 input and 800,000 output microdollars per million tokens. It verifies model capabilities through the production API. One call reserves 53,249 microdollars. The plan allows two reasoning calls, three edit calls, one resume call, one vision call, and one cancellation call: eight calls, reserving 425,992 microdollars.

A persistent Manager-side ledger reserves the entire plan before provisioning or paid inference. Its lock serializes concurrent runners. Repeating one run identity is idempotent; new runs require remaining allowance. Failure does not release reservations automatically. The ledger includes prior acceptance reservations, including unknown outcomes, and refuses changed caps or changed prior allowance. It is an acceptance control, not provider invoice reconciliation.

The parent's eight prior attempts were bounded at 54,068 microdollars each. With that recorded campaign history, use 432,544 as the prior reservation, making the combined worst case 858,536 microdollars. If the parent performs another paid attempt, update the campaign accounting before invoking this runner. Do not reset the ledger to obtain another allowance.

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

## Observed implementation evidence

On 2026-10-27, `scripts/verify.sh` passed formatting, clippy, default and test-worker suites, release builds, and both release feature refusals. `scripts/verify-m04-coding-vision.sh` passed the recorded manager library suite with 56 tests and two Linux integration lanes ignored, the three acceptance-ledger tests, and generated-binding/TypeScript checks. `npm test` passed 17 desktop tests, and `npm run build` passed. The actual CLI contract passed on macOS and Linux without provider spending. The Linux contract ran under 256 MiB and 32-process limits and recorded an 82,722,816-byte cgroup peak. The production worker integration passed its real edit, resume, denied-tool failure, and shutdown checks. The stalled coding request initially exceeded the three-second manager shutdown bound. After registration and shared shutdown signaling, the same test drained within that bound and found the persisted unknown/held row before reopening State. The blocked-admission caller-cancellation regression also passed.

The two-VM preparation passed against separate Manager and Computer VMs, including private permissions, redaction, and clean shutdown. Its result explicitly reports `live_acceptance: false`. Its isolated base was `/home/botstral/m04-acceptance-566a8a7fffeb4d7ebd3d7e008f3bbb21`; the passing evidence UUID was `6dc6c98e-0b23-4aa1-81c5-a97cdef83b8d`. A first preparation attempt exposed import-created cache permissions; the helper now disables bytecode writes and establishes a private umask before local imports.

Live execution of this new coding/vision runner remains pending with the parent. Parent-observed provider compatibility and earlier browser/file acceptance are grounding evidence, not proof of these newly implemented live paths.
