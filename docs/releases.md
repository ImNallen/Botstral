# Install and upgrade an exact release

Botstral releases contain separate Ubuntu 24.04 amd64 manager and worker Debian packages and a macOS Apple silicon DMG. Each set identifies one version, source revision, distribution channel, and CI build. Link ships compiled into each executable. Worker protocol 4 and owner protocol 3 retain their existing admission checks.

Stable and nightly replace the same installation. The desktop identifier remains `com.botstral.desktop`, and credentials remain in `com.botstral.desktop.owner.v1`. Channel switching requires an explicit coordinated upgrade. There is no background updater or rolling protocol upgrade.

## Verify the complete set

Choose one immutable version. Download its `manifest.json`, `manifest.sig`, and all three named artifacts into a fresh directory. Obtain the manifest public key through an independent trusted channel. A key downloaded beside the artifacts does not establish trust.

Use the release tooling from the manifest's source revision, obtained from a trusted source checkout. Python 3.9 or newer and OpenSSL are required. Do not execute artifact contents before verification.

```sh
python3 scripts/release.py verify /path/to/release-set \
  --trusted-key /trusted/botstral-manifest-public.pem --production
```

The verifier rejects unsigned sets, missing or extra artifacts, mixed identities, unsafe filenames, wrong package architectures, invalid signatures, corruption, and missing native acceptance. It inspects Debian metadata and embedded executable identities without running the payload. On macOS it also mounts the DMG read-only and checks the app identifier, arm64 executable, native version, Developer ID signature, notarization, and Gatekeeper assessment. Linux relies on the signed macOS acceptance receipt for DMG contents. Both receipts bind the exact artifact digests.

`--unsigned` is an explicit local verification mode. It cannot pass production verification or publication. Locally signed but unnotarized builds still use this mode.

## Install on new hosts

Manager and worker require separate hosts. Packages conflict if installed together. Packages neither create accounts nor activate services. They contain binaries, maintenance tools, service units, and examples under `/usr/share/botstral-manager` or `/usr/share/botstral-agent`. They never package live configuration, state, certificates, runtimes, or user homes.

Existing unmanaged installations are refused. Stop and back them up, record their paths and identities, and retire their binaries and service units explicitly before installing a package. Preserve their state and configuration. There is no implicit adoption switch.

After verification, install exactly the local package named by the manifest on each host. The following version is illustrative. Substitute the chosen immutable version throughout the procedure.

```sh
sudo python3 scripts/release_upgrade.py /path/to/release-set \
  --version 0.1.0 --component manager --fresh-install \
  --trusted-key /trusted/botstral-manifest-public.pem \
  --receipt /private/manager-install.json --apply
```

On the separate worker host, run the same command with `--component agent` and its own receipt. The command invokes `dpkg --install` on the verified exact package. Install any declared Ubuntu dependencies with `apt` first. An unresolved dependency leaves the package unconfigured and services stopped.

Create the manager account and private state directory according to [manager packaging](../manager/packaging/service.example.json). Copy the example to `/etc/botstral-manager/service.json` only when that file is absent. Set ownership and mode 0600, supply private TLS files, and follow [recovery and installation](m06-recovery.md) for account and runtime setup. Enroll the worker explicitly with its existing pinned runtime, process profile, and TLS inputs. Runtime files are independent installation inputs and remain outside the package.

For a new desktop installation, use the exact verified DMG through the same preflight command:

```sh
python3 scripts/release_upgrade.py /path/to/release-set \
  --version 0.1.0 --component desktop --fresh-install \
  --trusted-key /trusted/botstral-manifest-public.pem \
  --receipt /private/desktop-install.json --apply
```

Use `sudo` only if `/Applications` requires it. The default destination is `/Applications/Botstral.app`. An established installation at a different path requires its explicit `--app-path`. Never install a second stable or nightly copy against the same data.

## Upgrade all components offline

1. Finish or cancel admitted work. Review uncertain outcomes before retrying anything. Quit desktop and stop new admission. Record the current `--version-json` output from desktop, manager, worker, and the worker supervisor binary.
2. Stop manager and worker. Use the existing coordinated backup command without `--restart-after-success`. It stops manager before worker and verifies both offline backups.

```sh
python3 scripts/m06-backup-pair.py \
  --manager-host MANAGER --worker-host WORKER \
  --worker-plan /private/recovery-plan.json \
  --manager-destination /private/backup-manager \
  --worker-destination /private/backup-worker \
  --receipt /private/coordinated-backup.json
```

3. Copy manager service configuration, TLS material, worker configuration, and selected external runtime roots to protected backup storage. Retain the recovery passphrase offline. The backup receipt refers to remote backup locations. Verify those backups on their respective hosts before replacing binaries. Back up desktop data separately while the app is closed. Preserve Keychain rather than deleting or recreating its entries.
4. Run the exact upgrade command on each respective host. Retain the prepared receipt if replacement fails. Each receipt records previous and target identities, the target artifact digest, the coordinated backup receipt digest, and the configuration backup location.

```sh
sudo python3 scripts/release_upgrade.py /path/to/release-set \
  --version 0.1.0 --component manager \
  --trusted-key /trusted/botstral-manifest-public.pem \
  --backup-receipt /private/coordinated-backup.json \
  --config-backup /private/configuration-backup \
  --receipt /private/manager-upgrade.json --apply
```

Run this with `--component agent` on the worker host and `--component desktop` on the Mac. Add `--switch-channel` only for an intentional channel change. A lower semantic version is always refused. A different source or build at the same product version is also refused. Never resolve `latest` midway through this process.

5. Confirm the exact installed identity on every host before startup:

```sh
/usr/bin/botstral-manager --version-json
/usr/bin/botstral-agent --version-json
/usr/bin/botstral-agent-supervisor-check --version-json
/Applications/Botstral.app/Contents/MacOS/botstral-desktop --version-json
```

Run only the commands for that host. Compare version, revision, build identifier, link schema digest, and protocols with the signed manifest.

6. Reload systemd units explicitly. Start manager, then worker on its separate host. Unlock the existing manager vault explicitly through local administration. Reopen desktop and confirm the retained manager identity and pairing.

```sh
sudo systemctl daemon-reload
sudo systemctl start botstral-manager.service
# On the worker host, as its existing account:
systemctl --user daemon-reload
systemctl --user start botstral-agent.service
```

Package hooks refuse running executables, unmanaged paths, and downgrades. They never start, enable, reload, or restart a service. These checks cannot stop a competing operator from launching a process after preflight. Keep admission and both services stopped until identity confirmation is complete.

## Repair replacement or restore old state

A failed startup leaves state for diagnosis. Do not automatically replay work or roll back binaries. Keep both services stopped. Database migrations may already have run.

For an interrupted Debian installation, verify the same exact release again, then repair it with `dpkg --install` on that exact artifact. Resolve dependencies explicitly. Replacing a missing executable with the same package preserves external state. Do not remove state or use purge as an upgrade step.

A DMG has no package receipt, downgrade enforcement, or atomic copy operation. The tool retains the old app as a sibling `Botstral.app.retired-BUILD_ID`, then copies the verified bundle. If copying fails, rerun with `--repair-receipt` pointing to the original prepared receipt, the same backup references, and a fresh `--receipt`. Repair requires an unfinished receipt for that exact destination and target. An intact newer executable still prevents downgrade. Successful installation marks its receipt applied, which prevents reuse as repair authority. Inspect and retire a conflicting saved bundle explicitly before retrying. Do not launch a partial bundle. Keep backup app copies outside ordinary application search paths after recovery.

To recover an older version after migration, stop both services and desktop. Retire the failed state roots and binary packages explicitly. Install the old verified package set into absent native installation paths, then restore the matching coordinated offline backups into absent state destinations. Use the existing manager and worker restore commands in [recovery](m06-recovery.md). Restore configuration and external runtime roots as that backup requires. Preserve the original worker retirement acknowledgment. Confirm old identities, start manager then worker, and unlock explicitly. Binary replacement alone cannot undo migrations or resolve uncertain external effects.

## Build and accept a release

`release.json` owns the base source version, required artifact registry, protocol evidence, data compatibility, and runtime requirements. Check source mirrors and both Cargo lockfiles with:

```sh
python3 scripts/release.py versions --check
```

Update the authority, then run `versions --write` to update source mirrors. Runtime source pins cover the connector, browser dependency lock, browser launcher and sealing code, and Vibe adapter and sealing code. Update those pins when changing their inputs. Bindings remain checked with `npm run bindings:check`. Stable identity creation requires a clean checkout at its matching `vVERSION` tag. Nightly uses `VERSION-nightly.YYYYMMDD.SEQUENCE`, with a unique CI run and attempt in both sequence and build identifier.

```sh
python3 scripts/release.py identity --channel nightly --build-id RUN-ATTEMPT \
  --sequence 12345 --output /tmp/identity.json
python3 scripts/release.py versions --write --identity /tmp/identity.json
python3 scripts/release.py build --identity /tmp/identity.json \
  --target ubuntu24.04-amd64 --output /tmp/linux-build --unsigned
```

Build Linux packages on Ubuntu 24.04 amd64 and the desktop DMG on Apple silicon. Both jobs consume the same immutable identity. Each output includes an artifact descriptor. Aggregate already-built outputs, not independently rebuilt binaries:

```sh
python3 scripts/release.py aggregate --input /tmp/linux-build \
  --input /tmp/macos-build --output /tmp/release-set
python3 scripts/release.py verify /tmp/release-set --unsigned
```

Debian maps nightly to `VERSION~nightly.DATE.SEQUENCE-1` and stable to `VERSION-1`. Stable sorts later for the same base. macOS uses the base version for `CFBundleShortVersionString`, the base plus `d1` for nightly `CFBundleVersion`, and the base for stable. All nightly builds of one base share that native development version. The embedded full SemVer and preflight govern ordering between nightly dates and sequences. DMG delivery has no native version receipt.

For source archive or existing Docker builds, supply the full revision explicitly as `BOTSTRAL_SOURCE_REVISION`. Existing Dockerfiles require `--build-arg BOTSTRAL_SOURCE_REVISION="$(git rev-parse HEAD)"`. A Git checkout always determines its revision from Git and rejects a conflicting environment override. Production package construction requires that checkout and its recorded revision. `--local` identity creation allows uncommitted development builds for verification only and does not establish production source provenance.

Run two distinct real artifact sets through the native harness:

```sh
python3 scripts/release_acceptance.py --old /tmp/native-old \
  --new /tmp/native-new --target macos-arm64 --output /tmp/mac-acceptance.json
# Only on a disposable Ubuntu host, with no Botstral installation:
sudo env BOTSTRAL_DISPOSABLE_RELEASE_HOST=1 python3 scripts/release_acceptance.py \
  --old /tmp/native-old --new /tmp/native-new --target ubuntu24.04-amd64 \
  --production --output /tmp/linux-acceptance.json
```

The Linux harness installs real Debian packages sequentially on an isolated host to preserve component conflicts. It seeds actual manager state through existing test-only state helpers, runs the packaged manager before and after replacement, reopens encrypted credentials only after explicit unlock, compares retained identities, history, artifacts, worker pairing records, and attention evidence, and exercises schema failure plus backup recovery. Worker checks retain the production journal, selected certificate bytes, and runtime bytes. A temporary local TLS probe observes the new executable's ClientHello after journal opening. The probe restores the original configuration and does not prove a live paired fleet or browser/Vibe execution. The packaged manager also uses its retained encrypted provider credential against a recorded HTTPS provider on the disposable host. Production mode additionally requires systemd 255 and the real worker supervisor check.

For an ordinary local Docker container, add `--package-only` to exercise native package behavior without systemd acceptance. Its receipt records limited coverage and cannot pass the production gate. Full worker backup, diagnostics, and supervisor behavior still require the native systemd host.

The macOS harness installs two actual DMGs into a temporary bundle path, executes both packaged identities, checks active-process refusal, repairs an interrupted bundle, and uses the packaged executables to write and read one synthetic native Keychain entry. `--release-keychain-check UUID write|verify|delete` is restricted to the synthetic `release-acceptance-UUID` slots. `--release-acceptance-hold` supports active-process refusal without opening real desktop data. These checks do not prove interactive UI behavior or live remote pairing. `--production` requires actual Developer ID, notarization, and Gatekeeper acceptance.

## Configure production CI

The release workflow runs for stable tags, scheduled nightly builds, or explicit dispatch on trusted refs. Ordinary source and pull-request CI cannot publish. The prepare job pins one identity and revision. Both source verification workflows must pass before independent native jobs build two fixture identities and run acceptance. The aggregation job requires the complete registry, exact acceptance digests, app and DMG signatures, notarization, and a detached manifest signature. Publication creates the immutable release and uploads all assets before updating its channel pointer. Nightly releases remain prereleases and never become stable latest.

Configure these credentials before expecting production success:

- Native build secrets: `APPLE_CERTIFICATE_P12_BASE64`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_NOTARY_KEY_P8`, `APPLE_NOTARY_KEY_ID`, and `APPLE_NOTARY_ISSUER`.
- Native build variable: `APPLE_SIGNING_IDENTITY`, using the existing Developer ID Application team for credential continuity.
- Signing environment secret: `RELEASE_MANIFEST_PRIVATE_KEY`.
- Independently configured signing and publication variable: `RELEASE_MANIFEST_PUBLIC_KEY`.
- A protected `release-publication` environment with the permitted GitHub token contents-write capability. Only that job receives publication permission.

A failed draft publication requires explicit inspection and cleanup before retrying. The publisher refuses to overwrite an existing immutable release tag.

Missing credentials fail the workflow. There is no unsigned production fallback. Manifest signing runs on Apple silicon and inspects real Debian payloads, app signatures, notarization, and the DMG before using the private key. It runs only after native acceptance; the publication job re-verifies the signed complete set and never rebuilds it. Configure environment approvals according to the repository's publication policy. No release workflow was dispatched as part of implementation.
