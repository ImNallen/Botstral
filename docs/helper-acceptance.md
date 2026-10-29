# Verify the Vibe helper gate

Helper acceptance passed on October 29, 2026. The [live supplement](evidence/helper-acceptance/live.json) and [strict aggregate](evidence/helper-acceptance/aggregate.json) close the remaining first-acceptance gate. The existing eight-check evidence stays unchanged.

## Recorded verification

The October 29 recorded run passed with a production worker coding lease, the installed Vibe compactor, a saved matching summary, settled accounting, all six refusal controls, and clean shutdown. Its [preserved proof](evidence/helper-acceptance/recorded.json) is labeled recorded. It cannot close live acceptance. No live inference or provider-key retrieval occurred.

The [initial independent review](evidence/helper-acceptance/initial-review.md) reproduced two false-positive verdicts. The corrected verifier binds transport claims to the running manager executable and joins aggregate configuration to durable helper configuration. Both [regressions fail before correction](evidence/helper-acceptance/regressions-before.log). All 17 [acceptance regressions pass afterward](evidence/helper-acceptance/regressions-after.log). The [independent code recheck](evidence/helper-acceptance/code-review.md) approved the correction. Relabeling the real recorded report is [refused](evidence/helper-acceptance/recorded-relabel-refusal.json).

The first live attempt exposed a raw-JSON comparison that rejected valid multiline summaries. Its [failure remains recorded](evidence/helper-acceptance/live-attempt-1.json). The correction preserves exact parsed history equality and provider-summary joins. The [multiline regression reproduces the refusal before correction](evidence/helper-acceptance/multiline-before.log), and all 18 [focused checks pass afterward](evidence/helper-acceptance/multiline-after.log). [Repository verification](evidence/helper-acceptance/repository-verification.log) passed before this final Python-only correction; the later focused checks cover that correction.

The second live attempt passed with one helper call and two parent turns. The separate helper ledger retains both attempts at US$0.319494 in conservative ceilings under its unchanged US$1 cap. This is reserved exposure, not invoiced spending. The [final independent review](evidence/helper-acceptance/final-review.md) approved the helper gate and strict aggregate. It does not establish automatic CLI threshold triggering or P01. [Artifact hashes](evidence/helper-acceptance/manifest.json) preserve the proof and reviews.

The [decision trail](helper-acceptance-decisions.tsv) preserves the corrections and later credential-handoff clarification. The [transcript audit](evidence/helper-acceptance/trail-audit.md) had the original cross-judge result and complete root activity record. Its two trail findings are superseded by appended corrections. All 17 owned fixture roots were [removed from both guests](evidence/helper-acceptance/fixture-removal.json) after checking quiescence and available state locks. Shared services and build caches remain.

Shipping review reproduced a [concurrent-publication race](evidence/helper-acceptance/shipping-initial-review.json), tracked in [issue 44](https://github.com/ImNallen/Botstral/issues/44). The reader now treats incomplete JSON as not yet ready within the existing bounded poll. A permanently malformed file times out and cannot pass acceptance. The deterministic [before](evidence/helper-acceptance/publication-before.log) and [after](evidence/helper-acceptance/publication-after.log) regressions pause a real writer after opening an empty or partial file. All 20 focused checks pass. The sealed fixture and archived recorded/live evidence remain unchanged; strict verification and aggregation still pass.

## Prepare a current build

Use the two SSH aliases, systemd user managers, installed Vibe 2.24.2, and browser runtime required by [first acceptance](first-acceptance.md). Freeze the checkout before deployment. The driver checks every source digest in `source-files.json` before it uses the build.

```sh
python3 -B manager/tests/m06_integrated.py --prepare --output .audit/helper-build
```

Retained infrastructure can supply dependencies and build caches. An older deployment manifest does not establish compatible production binaries.

## Prove the worker lease without inference

Run the recorded suite without a provider-key argument. Use a fresh output directory for each attempt. Reuse its named offline campaign directory across retries.

```sh
python3 -B manager/tests/first_acceptance.py \
  --suite helper --recorded \
  --build .audit/helper-build/deployment.json \
  --output .audit/helper-recorded \
  --campaign-directory /home/botstral/helper-recorded-campaign
```

A successful recorded run exits zero and records `offline_passed: true`. Live acceptance remains `NOT VERIFIED`. The provider transport is simulated. The production worker still claims and executes an approved coding task through the manager's real lease enforcement.

The fixture seals a private copy of Vibe and replaces only `bin/vibe` with the acceptance launcher. The unchanged production worker launches that hashed runtime. The unchanged production `configuration()` and Relay supply its configuration and transport. The launcher calls the installed `AgentLoop.compact()` once. It observes the backend's serialized requests and responses through HTTP client event hooks. It does not replace completion logic.

Use `--recorded-mode fallback` to omit the primary summary and exercise Vibe's real fallback. Use `--recorded-mode empty` to omit both summaries. The empty mode must exit nonzero despite its compaction boundary. Each mode requires a fresh output directory.

## Run the separately funded supplement

After independent review, obtain the private key through the existing secret handoff. Run one live supplement with the successful recorded report. Use a separately named helper campaign with its permanent US$1 cap.

```sh
cargo build --manifest-path desktop/src-tauri/Cargo.toml \
  --no-default-features --locked --example c02-remote-driver
python3 -B manager/tests/first_acceptance.py \
  --suite helper \
  --build .audit/helper-build/deployment.json \
  --probe .audit/helper-recorded/results.json \
  --native desktop/src-tauri/target/debug/examples/c02-remote-driver \
  --key-file /absolute/private/provider-key \
  --output .audit/helper-live \
  --campaign-directory /home/botstral/helper-live-campaign
```

The recorded proof and copied runtime identities are checked before credential provisioning. The suite runs no completed first-acceptance workflows. Its parent run requests one coding task and then finishes. Model behavior that does not produce that task fails the supplement without an automatic retry.

To retain earlier binary acceptance, add `--baseline docs/evidence/first-acceptance/evidence.json` and `--retained-manager /absolute/remote/path/to/botstral-manager`. Supply the earlier desktop driver through `--native`. The helper suite does not execute the desktop driver. It checks its exact retained hash instead of rebuilding it. The source delta must contain only acceptance tooling, documentation, or the existing test-only fixture modules. The manager bytes must match the baseline before launch. Captured process evidence must match the executable that actually runs. A recorded test executable cannot establish live transport.

Pinned compaction has two paths, four overflow attempts per path, and three backend transport attempts per call. Its conservative runtime bound is 24 attempts. Manager limits admit at most six helper attempts and eight total attempts, including the two parent calls. The campaign reserves all eight ceilings before provisioning. Failed and uncertain attempts retain their durable ceilings until the existing stopped-fixture reconciliation releases unused exposure. Keep the campaign directory across failures.

The evidence requires the pinned prompt and source hashes, active worker lease and process scope, exact generated requests, durable coding-call joins, matching provider summary and saved history, model, configuration revision, settlement, parent accounting, and cleanup. Synthetic controls separately cover model mismatch, unsupported tools, unsupported execution capability, missing acknowledgment, and unconsented preview and latest identifiers. Every control must add zero provider attempts.

The endpoint claim follows the observed configuration and pinned production provider implementation. It does not claim a packet capture. The helper claim covers explicit `AgentLoop.compact()` integration. It does not cover automatic headless CLI threshold triggering.

## Aggregate compatible evidence

Use the live `results.json`, which contains the original supplemental evidence and runtime identities. Write a new aggregate file.

```sh
python3 -B manager/tests/first_acceptance_compaction.py \
  --baseline docs/evidence/first-acceptance/evidence.json \
  --supplement .audit/helper-live/results.json \
  --output .audit/helper-aggregate.json
```

Aggregation requires the exact retained production binary hashes, relevant runtime and configuration identities, complete prior checks, and a source delta limited to acceptance tooling and documentation. A binary mismatch blocks retention even when the sources appear equivalent. Establish compatibility or rerun affected gates before aggregation. Each retained check keeps its original report digest, source, and check digest.

## Run local regressions

```sh
python3 -m unittest discover -s manager/tests -p 'test_first_acceptance*.py' -v
python3 -m unittest discover -s agent/tests -p 'test_m04_acceptance.py' -v
```

The repository verification script includes the helper regressions. After collecting the private evidence, stop and remove only the owned fixture roots. Preserve shared runtimes and services. Do not start P01 until the live supplement, strict aggregation, and independent review pass.
