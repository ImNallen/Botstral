# C02 production acceptance record

The initial run reported ISSUES. The native composer browser policy failed on production source `e81cdbd51e2e852d2e6a78b5505eccf3a43ff8ed`. All other executed service gates passed. The parent corrected and passed that gate in `b6081d2`, then completed the packaged graphical workflow. See [parent acceptance](c02-acceptance.md) for the updated results.

## Production issue for the parent

The native `Send` policy in `desktop/src-tauri/src/remote.rs` assigns 64 processes and 512 MiB to browser work. The approved production task exited 1 with `computer operation failed`. Both declared artifacts were unavailable. No browser request reached the fixture page.

A separately labelled control submitted the same operations through fixture loopback administration with the previously supported M04 policy of 128 processes and 1 GiB. It completed navigation, fill, click, text capture, file creation, and screenshot. Native review, approval, transfer, digest verification, and export used the public relay. The exported text is exactly `C02 production browser proof`. The guest file is exactly `C02 production file proof` followed by a newline.

This proves the combined native resource policy is insufficient for this supported runtime. It does not isolate which resource limit causes the failure. The test fixture does not change the native policy. The parent reproduced the failure over the restarted relay, changed native policy to the supported combined limits, and reran the actual native composer gate. It passed with actual DOM and guest-file effects and native verified text and PNG exports. This establishes the combined policy correction; neither resource was isolated individually. [Parent results](evidence/c02-production-parent/browser-policy-correction.json) retain the before and after binary identities and outcomes. Packaged graphical acceptance remains open.

The [failed task](evidence/c02-production/native-browser-failure.json), [complete results](evidence/c02-production/summary.json), [exported text](evidence/c02-production/browser-export.txt), and [actual browser PNG](evidence/c02-production/browser.png) retain the observations. The control is not counted as a passing native composer browser gate.

## Method and exact identities

The source HEAD matched the assigned SHA before any work. Its merge base with the requested base was `6c2b70b03295cdc8a5f69411f0bb0628f383c7fd`. No checkout was created or changed.

The Manager VM ran the production Manager runtime, SQLite state, dispatcher, public TLS listener, and owner protocol inside an ignored test executable. Test-only support injected a localhost recorded provider and exercised runtime restart. This is not a release Manager executable or a full operating-system process restart claim.

The macOS desktop host ran the actual `RemoteService` and real OS keychain through a private test driver. It connected over actual TCP to `192.168.1.40:34691` and `bore.pub:51870`. Both routes presented leaf `190fb91a71f5eda71976d00959b4b1d7112f2fbe8bc840745bff7af01cca79e7`. No desktop GUI was touched.

The Computer VM executed the unchanged installed worker at `/home/botstral/m04-final-acceptance.GycLnu/bin/production/botstral-agent`. Its SHA-256 was `a1dffc665e7eb6176a53fe5f325a0874f20f4ea526e9eb23b7a24ba412365a00`. The browser manifest remained `1e3b96245240d969ab380e4ce843f6ac68d27444d425cf211f75e4d2b6859061`. All 5,574 listed runtime files matched before and after execution. Vibe was not used or modified.

The [build record](evidence/c02-production/build.json) lists the three explicit test-only overlays. All other archived production source came from the frozen SHA. The archive SHA-256 was `8bc02330af8988b083c32a8a926e62415218af89dab8c8896266a272f20f7be1`. The Linux fixture binary was `d55e93e7d1e8bcbd301cdc07d239d7ab27734b7c9bba54ccc23dd60040f74de3`. The macOS driver was `1c4e11b57af3a3d22adba54ff39eda431f4014fba85e197a209f187c1a8b619e`.

The [post-run identity check](evidence/c02-production/final-identities.json) matched the retained files and the actual `/proc/PID/exe` bytes for the running Manager fixture, relay, and installed worker.

## Executed gates

Every row below applies to source `e81cdbd51e2e852d2e6a78b5505eccf3a43ff8ed` with the explicit test overlays. The [result record](evidence/c02-production/summary.json) contains exact task, execution, run, process, endpoint, and artifact identities.

| Gate | Outcome and evidence |
| --- | --- |
| Actual native pairing and durable chat over LAN | PASS. Real keychain and authoritative workspace at cursor 15. |
| Actual native service through public relay | PASS. Recorded assistant output and unchanged pinned inner TLS from macOS. |
| Native composer production browser | ISSUES. Approved task exited 1. Artifacts were unavailable. |
| Supported M04 policy diagnostic | PASS. Actual DOM and file effects. Native verified text and PNG exports over relay. Submission used loopback control. |
| Installed worker process-group cancellation | PASS. Python PID 415449 and child PID 415450 were observed in the exact execution cgroup, then both were absent. Manager recorded `SystemdScopeEmpty`. |
| Retention-gap recovery | PASS. Cursor 64 was outside the 128-event window. Actual Resume returned a snapshot. Native resume recovered all 160 messages at cursor 225. |
| Actor recreation | PASS. The same keychain record restored authoritative history at cursor 225. This did not restart the packaged desktop process. |
| Runtime restart during active production work | PASS. Manager runtime and public listener restarted. Native and worker reconnected. Cancellation stopped PIDs 415606 and 415607. |
| Lost committed response | PASS. A private TLS fault proxy forwarded one real SubmitRun and dropped its matching TransferStart after the Manager committed. Native returned `unknown_outcome`. Recovery reconciled one durable run without replay. Exact approval produced one real file append. |
| Protected invitation helper | PASS. The requested public fingerprint received a mode 0600 local invitation. Its secret was not printed. An existing symlink was refused before authorization without changing its target. |

The lost-reply fault proxy used the fixture certificate and a private macOS loopback endpoint. That injected route is distinct from the actual bore.pub route checked earlier. It was stopped before handoff.

## Commands and exit outcomes

The following commands ran in this checkout:

```sh
cargo build --manifest-path desktop/src-tauri/Cargo.toml \
  --no-default-features --locked --example c02-remote-driver
python3 manager/tests/c02_production_build.py \
  --output .audit/c02-production/build-2
python3 manager/tests/c02_production.py run \
  --build .audit/c02-production/build-2/build.json \
  --bore .audit/c02-production/bore \
  --output .audit/c02-production/run-5 --keep-for-ui
cargo clippy --manifest-path desktop/src-tauri/Cargo.toml \
  --no-default-features --locked --example c02-remote-driver -- -D warnings
```

The native build, Linux build, and Clippy exited 0. The production runner exited 1 because the native browser gate is ISSUES. It completed all independent gates and retained the authorized fixture. The exact source archive and build exit are in [the build record](evidence/c02-production/build.json).

Python compilation, both Rust formatting checks, and Git whitespace checks exited 0. The same Linux fixture executable ran `m04_ --test-threads=1 --nocapture` on the Manager VM and exited 0 with 18 passed and two ignored tests. Those ignored production lanes are not counted as passes. The [command record](evidence/c02-production/checks.json) and [M04 log](evidence/c02-production/check-4.log) retain the exact invocation and outcomes. The broader repository and packaged desktop suites were not rerun by this acceptance agent.

Earlier runs remain failed. The initial Linux fixture build exited 1 before a test-module visibility correction. A Python file-opener error interrupted the first fixture attempt. The next attempt supplied invalid process ceilings and never reached worker readiness. Another run stopped at the actual native browser failure. A later run requested 300 messages, exceeding the 256-record bound, and initially sampled relay shutdown too early. Repeated cleanup verified its unloaded units, absent root processes, closed endpoints, and removed roots. [Run 2](evidence/c02-production/run-2-diagnostic.json), [run 3](evidence/c02-production/run-3-diagnostic.json), and [run 4](evidence/c02-production/run-4-diagnostic.json) preserve those outcomes.

## Retained fixture and exact cleanup

Only these four isolated units remain running for parent graphical reproduction and acceptance. The [unit record](evidence/c02-production/retained-units.json) contains observed PIDs and process command lines.

| Host | Unit | Observed PID |
| --- | --- | --- |
| Manager | `botstral-m05-c02-e305079cbc4e-manager` | 255862 |
| Manager | `botstral-m05-c02-e305079cbc4e-bore` | 257528 |
| Computer | `botstral-m05-c02-e305079cbc4e-worker` | 414219 |
| Computer | `botstral-m05-c02-e305079cbc4e-page` | 413945 |

Both private roots are `/home/botstral/c02-production-e305079cbc4e`. The public [control record](evidence/c02-production/fixture-control.json) contains no invitation or device credential. The two build roots and stale local private fixture directories were removed. Their [build cleanup](evidence/c02-production/build-cleanup.json) and [local cleanup](evidence/c02-production/local-cleanup.json) records verify removal. Earlier fixture roots are removed. Unrelated units and installed runtimes were not changed.

Use the [runnable workflow](c02-production-workflow.md) with the parent's final bundle. Generate the app's public fingerprint first, then run:

```sh
python3 manager/tests/c02_production.py invite \
  --fixture docs/evidence/c02-production/fixture-control.json \
  --fingerprint PUBLIC_DEVICE_FINGERPRINT \
  --output /private/path/c02-invitation.json
```

Import that file through the native chooser. Use an isolated debug profile at `/Users/nallen/.t3/worktrees/Botstral/t3-d08be483/.audit/c02-production/run-5/desktop-profile`. The fixture certificate expires on 2026-10-30 at 09:23:06 UTC. Invitations expire ten minutes after issue.

After parent sign-out, remove the fixture with:

```sh
python3 manager/tests/c02_production.py cleanup \
  --fixture docs/evidence/c02-production/fixture-control.json
```

The helper stops only the recorded units, verifies no root processes remain, checks both routes refuse TCP, removes both fixture roots, and records `cleanup.json`. Remove the local imported invitation and isolated desktop profile after sign-out.

## Remaining boundaries

The parent corrected and passed the native browser production policy gate in `b6081d2`. The service results above remain the original frozen-source observations. The parent completed the bundled remote-route walkthrough and [interaction video](assets/c02-paired-desktop.mp4). The final delta review and subsequent helper recheck are complete. All reproduced findings are resolved. Native choosers, actual app process restart during work, physical sleep and network changes, keychain denial, delivered OS notifications, and hostile packaged navigation remain unexecuted here. Linux Secret Service and Windows remain unverified.

Build the Lever produced the repeatable two-VM driver. Model the Domain kept public fixture control separate from per-gate results. Prove It Works kept the failed native browser gate open despite the successful diagnostic. No subagents or independent review ran, as requested. Parent review remains required. No push, PR, watch, merge, deployment, paid inference, real secrets, or real owner-state mutation occurred.
