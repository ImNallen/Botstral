# Local release verification

These receipts record parent verification of real fixture Debian packages and signed, unnotarized macOS DMGs. All native installation changes occurred in disposable containers or temporary application paths. The Mac checks used a synthetic Keychain slot and removed it afterward.

The two fixture identities have different nightly versions but share source revision `6adcbfe2d8d540a5e774356646aecf6515c24d85`. Their builds included uncommitted implementation files. They are development fixtures, not official releases or artifacts built from the final reviewed revision. The parent reran the installer and acceptance harness after fixing the independent review findings.

- `parent-cli-acceptance.json` records exact installation, active-process and completed-receipt refusal, and interrupted-installation repair.
- `parent-stale-repair-regression.json` records a real old-to-new DMG installation followed by rejected attempts to downgrade through completed and stale prepared receipts. Its backup metadata is synthetic.
- `parent-linux-package-acceptance.json` records real Debian replacement, retained state, backup recovery, and the new worker's TLS ClientHello after journal opening. This ran under amd64 emulation with package-only coverage.
- `parent-macos-native-acceptance.json` records real DMG replacement, synthetic Keychain continuity, active-process refusal, and interrupted-copy repair.
- `parent-negative-controls.json` records refusals for corrupted, missing, incomplete, mixed, wrong-target, and unsafe-path release inputs.

These receipts do not establish native systemd worker acceptance, Apple notarization or production Gatekeeper assessment, official manifest-key trust, CI signing access, hosted publication, or an upgrade from a previously published revision. The production release gate remains closed until its native checks and signing requirements pass. See the [release guide](../../releases.md) for commands and the [decision log](../../releases-decisions.tsv) for the review findings and fixes.
