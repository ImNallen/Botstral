# P06 browser and worker review

Reviewed head: `3527f3cfc2a3e3486df1d7e166141de19c96c70b`. It matches the assigned checkout. The review brief and both requested claim documents were read.

## Findings

No confirmed finding in this lane. In the fresh browser probe, a hostile page submitted during the adapter's `input` event. The controlled HTTPS server received exactly one POST, and settlement was applied. The probe uses production `agent/runtime/protected.mjs::login` and only synthetic values. The runtime probe uses the actual `agent/runtime/run.mjs` and refused echoed text, screenshots, profile file reads, symlink alias export, missing release inventory, and corrupt provenance. It accepted only the safe screened text. `cargo test -p botstral-agent --locked protected_browser::tests -- --nocapture` passed both profile provenance tests.

Source review confirms that `login()` holds real DOM objects from a Chromium isolated world and revalidates them before release, assignment, and submission (`agent/runtime/protected.mjs:94-140, 173-188`). The route admits only the approved frame, POST destination, and exact field payload, and consumes the submission before continuing (`agent/runtime/protected.mjs:67-82`). The worker derives scope from the current journal execution, checks release-possible lifecycle, session fence, expiry, UID and exact cgroup, persists consumed release history before manager release, and replaces client-supplied screening history with reconciled profile history (`agent/src/journal/process.rs:59-96`, `agent/src/protected_browser.rs:120-209`).

## Limits

This checkout ran on macOS arm64. I did not verify Linux cgroup admission, worker Unix socket permissions under the production systemd scope, worker-to-manager mutual TLS, or an authenticated manager-to-worker release. The unit test verifies profile provenance and journal replacement, not those deployment boundaries. I also did not rerun the integrated manager browser acceptance; the fresh probes use a synthetic manager callback for browser/runtime isolation, so they do not prove end-to-end manager authority.

The probe code is in `.audit/p06-review/duplicate-submit.mjs` and `.audit/p06-review/runtime-capture.mjs`. Exact commands and observed exits are in `.audit/p06-review/commands.log`. No production code or tests were changed.

## Verdict

PASS for the browser and worker source-review lane with the Linux transport and cgroup limits above. This is not final release approval.
