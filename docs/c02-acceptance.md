# C02 acceptance record

C02 implementation is available. Full milestone acceptance remains open for the parent's independent review, packaged graphical walkthrough, and configured remote route. No production owner state was changed.

## Verified service and protocol behavior

The implementation was exercised on macOS 27.0.1, build 26A434, on Apple silicon. Native credentials used the real macOS Keychain through keyring 3.6.3 with `apple-native`. The Linux backend is compiled with `sync-secret-service` and `crypto-rust`; Linux native behavior and Windows acceptance have not run.

The following commands passed against isolated synthetic managers:

```sh
cargo build --workspace --examples --locked
python3 manager/tests/m05_client_regressions.py --output /tmp/c02-client-verified
python3 manager/tests/m05_acceptance.py \
  --manager-bin target/debug/botstral-manager \
  --probe-bin target/debug/owner-probe \
  --agent-bin target/debug/botstral-agent \
  --output /tmp/c02-m05-verified
python3 manager/tests/c02_workspace.py --output /tmp/c02-workspace-verified
python3 desktop/fixtures/paired_manager.py --verify-native
```

All 15 M05 actual-binary contracts passed. They cover pinned authentication, credential-first enrollment recovery, worker and owner separation, retention rollover, interrupted transfers, heartbeats, dead peers, revocation, and uncertain mutations without replay. The extracted client also passed exclusive client and session ownership, committed mutation with response-storage failure, and transient TLS retry.

The C02 manager contract passed consistent workspace hydration, unchanged strict Resume fields, changed-action refusal, stale permission refusal, intervening approval revocation refusal, and exact execution grant. A pre-extraction probe binary was unavailable in the parent checkout. The separate old-binary gate remains unverified; the legacy Resume shape and existing probe contracts passed.

The actual-manager native contract passed real keychain creation and restoration, persisted graphical-pairing service operations, durable user and assistant run turns, linked task creation, reviewed grant, digest-verified export, synthetic cancellation with authoritative synthetic stop evidence, suspension, restoration, credential scans, and local device revocation. It checks that credentials and invitations are absent from snapshots and ordinary app files and that no `device.json` exists. The test deletes its synthetic keychain item and stops its fixture.

Native unit checks passed unavailable-store refusal before fingerprint publication, failed-deletion tombstone restoration, bounded link rejection, restoration of a journal larger than 64 KiB, and sign-out admission fencing while an earlier native call finishes. Frontend tests passed inert remote content, durable turn and task rendering, offline action refusal, strict credential-free parsing, stale revisions, and immediate sign-out fencing. All 24 ordinary frontend tests passed. The fixture also passed the separate renderer schema contract with actual native pairing, review, run, artifact, and signed-out projections. Existing local administration tests remain in the desktop verification command.

## Verification and packaging

`make verify` passed, including production and nonproduction builds, actual-binary recovery tests, release builds, and both forbidden nonproduction release-feature checks. The initial run exposed a new bot-admission limit that changed old behavior. The limit was removed. A later recovery assertion counted the newly committed reconnect roster events as duplicate report events. Its baseline now starts after reconnect and still proves zero events from replaying a report receipt.

`make verify-desktop` passed, including frontend checks and tests, native clippy and tests, actual-manager local administration checks, and the bundled-interface native build. A concurrent feature-changing build caused one intermediate rustdoc dependency failure; the serial verification and direct doctest rerun passed without changing the test gate.

The command `npm run desktop -- build --debug --bundles app` produced `desktop/src-tauri/target/debug/bundle/macos/Botstral.app`. This build contains the interface and requires no development server. Its graphical workflow is documented in [the native fixture guide](c02-native-workflow.md).

## Remaining exact acceptance boundaries

The parent still needs to launch the final bundle, use the isolated debug profile, and record pairing, native choosers, actual assistant text, task review, cancellation, and result save. The interaction video must not expose invitations or device credentials. Native origin and capability policy tests passed, but hostile runtime navigation, nested windows, popups, remote frames, unsolicited links, and event subscription attempts still need packaged observation.

The configured remote route from the actual desktop host and the supported worker on the Computer VM have not been exercised by this implementation owner. Real production browser execution, process-group cancellation, manager and app restart during active production work, desktop retention-gap recovery, and lost-reply reconciliation through the complete native graphical path remain acceptance gates. The M05 probe and synthetic native fixture support those checks but do not replace them.

Actual keychain lock or denial, failed deletion through the operating-system backend, physical sleep and network changes, and delivered operating-system notifications still need native observation. Deterministic unit tests cover the service failure states. Linux Secret Service and Windows remain unverified.

The parent owns final independent review, integration, visual proof, and pull request creation. No push, pull request, watch, merge, deployment, or production owner mutation was performed.


Sanitized [verification results](evidence/c02/verification.json), [source hashes](evidence/c02/source.json), [M05 results](evidence/c02/m05.json), [native results](evidence/c02/native.txt), and the [decision trail](evidence/c02/decisions.tsv) are retained with the implementation. The source manifest excludes generated target directories and contains no credentials.
