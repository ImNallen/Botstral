# C02 acceptance record

C02 implementation is available. The packaged graphical walkthrough passed with a real manager, macOS Keychain, recorded provider, and explicitly synthetic worker. Full milestone acceptance remains open for independent review and production checks over the configured remote route. No production owner state was changed.

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

The C02 manager contract passed consistent workspace hydration, unchanged strict Resume fields, changed-action refusal, stale permission refusal, intervening approval revocation refusal, and exact execution grant. The parent found the retained pre-extraction probe and verified its SHA-256 against the committed M05 identity record. All 15 M05 contracts passed with that old binary against the new manager. Its hash is `938da41b8a3329dbc6fbaa966dca365edc54c06d0fac70ad138eb512f3f4086b`.

The actual-manager native contract passed real keychain creation and restoration, persisted graphical-pairing service operations, durable user and assistant run turns, linked task creation, reviewed grant, digest-verified export, synthetic cancellation with authoritative synthetic stop evidence, suspension, restoration, credential scans, and local device revocation. It checks that credentials and invitations are absent from snapshots and ordinary app files and that no `device.json` exists. The test deletes its synthetic keychain item and stops its fixture.

Native unit checks passed unavailable-store refusal before fingerprint publication, failed-deletion tombstone restoration, bounded link rejection, restoration of a journal larger than 64 KiB, and sign-out admission fencing while an earlier native call finishes. Frontend tests passed inert remote content, durable turn and task rendering, offline action refusal, strict credential-free parsing, stale revisions, and immediate sign-out fencing. All 25 ordinary frontend tests passed. The fixture also passed the separate renderer schema contract with actual native pairing, review, run, artifact, and signed-out projections. Existing local administration tests remain in the desktop verification command.

## Verification and packaging

`make verify` passed, including production and nonproduction builds, actual-binary recovery tests, release builds, and both forbidden nonproduction release-feature checks. The initial run exposed a new bot-admission limit that changed old behavior. The limit was removed. A later recovery assertion counted the newly committed reconnect roster events as duplicate report events. Its baseline now starts after reconnect and still proves zero events from replaying a report receipt.

`make verify-desktop` passed, including frontend checks and tests, native clippy and tests, actual-manager local administration checks, and the bundled-interface native build. A concurrent feature-changing build caused one intermediate rustdoc dependency failure; the serial verification and direct doctest rerun passed without changing the test gate.

The command `npm run desktop -- build --debug --bundles app` produced `desktop/src-tauri/target/debug/bundle/macos/Botstral.app`. This build contains the interface and requires no development server. Its graphical workflow is documented in [the native fixture guide](c02-native-workflow.md).

## Packaged native walkthrough

The parent launched the bundled app with an isolated debug profile. Graphical checks passed trusted local sign-in, fingerprint creation, native trust-file selection, public manager and pin confirmation, paired workspace recovery, actual recorded assistant turns, exact-action review, synthetic task completion, native result save, synthetic cancellation, pause and resume, same-binary quit and relaunch, and current and future device revocation. Sign-out cleared the synthetic keychain credential. Both synthetic profile items and fixture processes were removed.

The exported file contained 65 bytes. Its SHA-256 matched the manager declaration, `c8a964046ab394d5f439d44153ef69eea6d53edb4912eff6cde223535b621906`. Its contents explicitly state that no production process ran. The native contract now waits on activity notifications without sending refresh commands. That contract passed. A stale accessibility diff initially suggested a missed update; a fresh rendered capture showed completion without a Refresh action. No event-delivery defect was confirmed.

The walkthrough exposed unreadable approval ordering and overflowing history. The parent moved exact operations and arguments before execution identifiers, kept the conversation inside its scroll area, arranged the sidebar controls vertically, displayed spending limits in dollars, and restricted activity delivery to the authorized main webview. The updated desktop verification passed, including 25 frontend tests. Final layout capture remains part of the production walkthrough.

Rebuilding the unsigned development binary triggered an operating-system keychain access prompt for an item created by the earlier binary. Automation could not inspect the protected system prompt. That process was stopped. Same-binary relaunch passed using a fresh synthetic profile. Signed release and update behavior remains in the release track.

## Remaining exact acceptance boundaries

The interaction video must not expose invitations or device credentials. Native origin and capability policy tests passed, but hostile runtime navigation, nested windows, popups, remote frames, unsolicited links, and event subscription attempts still need packaged observation.

The configured remote route from the actual desktop host and the supported worker on the Computer VM have not been exercised by this implementation owner. Real production browser execution, process-group cancellation, manager and app restart during active production work, desktop retention-gap recovery, and lost-reply reconciliation through the complete native graphical path remain acceptance gates. The M05 probe and synthetic native fixture support those checks but do not replace them.

Actual keychain lock or denial, failed deletion through the operating-system backend, physical sleep and network changes, and delivered operating-system notifications still need native observation. Deterministic unit tests cover the service failure states. Linux Secret Service and Windows remain unverified.

The parent owns final independent review, integration, visual proof, and pull request creation. No push, pull request, watch, merge, deployment, or production owner mutation was performed.

Independent lifecycle review reproduced a missing-profile sign-out defect. The new regression first failed because the native credential survived successful sign-out. After correction, the regression and all six deterministic native lifecycle tests passed. Both successful deletion and deletion failure after profile removal are covered. The independent recheck confirmed the missing-profile correction and found a second concurrent sign-out defect. Its regression failed before correction and passed afterward. All seven deterministic lifecycle tests now pass. Security review also reproduced journal-validation data loss and journal overflow. The parent corrected both and added isolated actual-manager recovery contracts to `make verify-desktop`. Fresh independent rechecks and production acceptance remain open.

Run `python3 desktop/fixtures/c02_source_record.py --check-controls` to verify the current component source manifest. The verifier also rejects changed hashes, omitted source files, and unexpected files. The implementation owner's earlier manifest is preserved as `evidence/c02/source-implementation.json`.


Sanitized [verification results](evidence/c02/verification.json), [source hashes](evidence/c02/source.json), [M05 results](evidence/c02/m05.json), [native results](evidence/c02/native.txt), and the [decision trail](evidence/c02/decisions.tsv) are retained with the implementation. The source manifest excludes generated target directories and contains no credentials.

Additional [two-VM production acceptance](c02-production-acceptance.md) ran against frozen source `e81cdbd51e2e852d2e6a78b5505eccf3a43ff8ed`. Actual macOS RemoteService access passed over LAN and the public relay. Production cancellation, retention-gap recovery, runtime restart recovery, and lost-reply reconciliation passed. The native browser resource policy failed. A separately labelled supported M04 policy control completed real browser work and verified exports. The production issue and parent graphical gates remain open.
