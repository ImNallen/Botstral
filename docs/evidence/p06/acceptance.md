# P06 implementation acceptance

The accepted base is `bf470fb00e98ed6e519b02f7a314fe48a808bdc1`. Its ancestry was verified before implementation in the assigned `t3/protected-login-autofill` checkout. No coordinator files, real credentials, paid inference, deployments, pushes or pull requests were used. No subagents were started.

The storage/native-entry unit is commit `51d0177c1684bdb3a88c03670c66f94308b99387`. The second unit adds supervised one-shot fills, live approval controls, remembered rules, per-fill TOTP, retained profile protection and private screening. The implementation details and operational limits are in [Protected reusable logins](../../p06-protected-logins.md). Source file digests are recorded in [implementation-files.json](implementation-files.json).

## Verification

`python3 scripts/verify-p06.py --full` exited zero. Its exact commands and exits are in `20261029T205800527207Z/commands.json`. The final focused driver run is in `20261029T211427680411Z/commands.json`. Its focused gates passed:

- Manager credential suite: 12 passed; one opt-in browser test is run separately.
- Worker profile provenance: 2 passed.
- Actual paired native service: 1 passed. Encrypted manager state, value-free receipts, lost-response reconciliation and native journal exclusion were checked against actual files.
- Actual macOS native controls: save, cancel, generation change, disconnect and unlock passed. All observed secure fields were empty afterward.
- Browser gate: 22 cases passed in the final focused driver, including post-release element replacement and late form-data mutation.
- Entry component replay: passed, with no page errors and no private HTML input fields.

The final browser command was:

```sh
BOTSTRAL_P06_EVIDENCE="$PWD/docs/evidence/p06/final-boundary" \
  cargo test -p botstral-manager --locked --lib \
  p06_controlled_browser_acceptance -- --ignored --nocapture
```

It exited zero. [browser.json](final-boundary/browser.json) records 22 cases and seven complete-runtime capture cases. The controlled server verifies actual submitted values and successful authentication separately from runtime completion. Cases cover password, remembered password, TOTP, redirects before and after POST, reflected DOM text, hostile main-world prototypes, same-origin frames, replaced fields/forms/documents/frames, changed action/method/visibility/submitter, input-event mutation, post-release replacement, late form-data method/payload mutation, denial and a dropped release response. Repeated release refuses.

The complete `run.mjs` process retained safe screened text and produced no output for private echoed text, screenshots, profile reads, aliased exports, missing release history and corrupt provenance. It scanned actual generated specifications and output files for raw and escaped synthetic values. Browser profiles are protected private state, not public artifacts. Current, retired and issued-code screening and backup rollback refusal also have focused manager tests.

[backup-restore.log](backup-restore.log) proves the real packaging backup/restore path preserves removed items and their receipts and retained screening values while reopening locked. [m01-schema.log](m01-schema.log) proves the real TCP binary integration and future-schema refusal. JavaScript syntax and `git diff --check` passed.

`./scripts/verify.sh` exited zero. Formatting, workspace Clippy, builds, packaging checks, workspace tests, nonproduction worker checks, production release builds and both nonproduction release refusals passed. The final manager unit suite had 150 passed and six opt-in tests ignored.

`./scripts/verify-desktop.sh` exited zero. Generated bindings, formatting, TypeScript, frontend tests (33 passed, one opt-in skip), native formatting/Clippy/tests, actual-manager local and journal integration, browser cleanup and the debug application build passed. The final JavaScript submission refinements were then verified by the focused real-browser/runtime gate and the standalone submission-boundary probe. No Rust or TypeScript behavior changed after the full gates.

## Interaction evidence

- [Before](baseline/handoff.png) and [before video](baseline/handoff.webm) replay the accepted unsupported handoff.
- [Entry after](20261029T205800527207Z/handoff.png) and [entry video](20261029T205800527207Z/handoff.webm) use the real entry component with public metadata.
- [Live review](final-boundary/live-review.png), [recorded decision](final-boundary/live-decision.png) and [approval video](final-boundary/live-review.webm) use the real review component, exact review parsing and actual manager decisions through the controlled fixture bridge.
- [Native control results](20261029T205800527207Z/native-controls.log) exercise actual AppKit secure fields. The separate native-service fixture exercises real authenticated paired transport. These are not a single packaged native-to-Linux session.

No private login values appear in the screenshots, video or result files. Historical evidence was retained. The [decision trail](decisions.tsv) records the changes and corrected failures.

## Corrected failures and remaining boundaries

The redirect test initially proved that ordinary Playwright routing followed an unapproved redirect. Chromium response interception now blocks both initial and POST redirects. Full tests found old connector lease table references and recovery/schema expectations; all were updated with migration 14. A new form-data mutation case then exposed a late submission gap. A second DOM call during paused navigation stalled; a direct probe proved the original synchronous DOM call completes before the request pause. Its post-submission validation now gates one exact frame/method/destination/payload request. Both mutation cases refuse and settle uncertain. One full run of the existing provider-slot deadline test remained `Cancelling`; its isolated rerun and the subsequent full manager suite passed without changing that test. The failed runs remain in their original evidence directories.

No whole-shape design was reselected. The initial adapter is narrower than general same-origin frame trees: it supports only a top-level form or one same-origin child frame, with no extra hidden controls or custom JavaScript submission. Unsupported forms explicitly refuse. This adapter scope is presented for root review, not claimed as an independently accepted completion deviation.

The actual Linux cgroup/private-TLS release path and a single packaged native-to-Linux sign-in remain unverified. Production Linux execution tests are opt-in and remain skipped by the local macOS suite. The native entry adapter refuses other platforms. Arbitrary code with full shared guest privileges, receiving website scripts, arbitrary secret transformations and memory zeroization outside controlled buffers are explicit trust limits. Existing cookies and remote sessions have separate revocation lifetimes.

No PLAN checkbox was marked complete. Independent code, security and maintainability review remains root-owned.
