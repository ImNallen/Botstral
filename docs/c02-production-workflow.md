# Run C02 against the production worker

This fixture uses isolated state on the Manager and Computer VMs. The macOS driver calls the actual native `RemoteService` with the OS keychain. The installed M04 production worker executes browser and process work. A recorded provider supplies responses and synthetic credentials. No paid inference is used.

The Manager runs inside an ignored test executable. Its production runtime, SQLite state, public TLS listener, owner protocol, dispatcher, and worker gateway are unchanged. Test support selects the recorded provider and restarts the runtime. This is service acceptance. The packaged graphical walkthrough remains a separate parent gate.

## Build the fixture

Run these commands from the checkout containing this fixture:

```sh
cargo build --manifest-path desktop/src-tauri/Cargo.toml \
  --no-default-features --locked --example c02-remote-driver
python3 manager/tests/c02_production_build.py \
  --output .audit/c02-production/build
```

The build script archives production source `e81cdbd51e2e852d2e6a78b5505eccf3a43ff8ed`. It overlays only the three listed Rust test-support files. It builds on `botstral-computer`, verifies the archive before extraction, and records the binary hash. The Manager VM needs no Rust installation.

Supply the bore v0.6.0 Linux executable. Its SHA-256 must be `60548b7a145ba334981b19fda7cd0210d24a108a3d0dc113919b33fd2eaa90ab`. The upstream `bore-v0.6.0-x86_64-unknown-linux-musl.tar.gz` archive has SHA-256 `e484d1e3acba77169b773f31a5bfb34192d4b660f44a094a658a2522cd2270f7`.

## Run service acceptance

Use an unlocked macOS keychain. The SSH aliases must identify the two authorized VMs:

```sh
python3 manager/tests/c02_production.py run \
  --build .audit/c02-production/build/build.json \
  --bore /private/path/to/bore \
  --output .audit/c02-production/acceptance \
  --keep-for-ui
```

Choose a fresh output directory. Without `--keep-for-ui`, the driver removes the remote fixture after testing. An unhandled fixture failure also cleans up. With the flag, an orderly run retains only its private fixture services for graphical acceptance or reproduction of a recorded production issue. `fixture-control.json` records their exact identities, routes, public trust, and cleanup information.

The runner checks the retained worker hash and all browser runtime files before use. It creates separate transient user services for the Manager fixture, worker, browser test page, and public relay. It leaves the installed worker and runtimes unchanged.

`results.json` records each executed gate. These include direct LAN and public relay access from macOS, actual DOM and file effects, native digest-verified text and PNG exports, kernel process identities before cancellation, authoritative stop evidence, retention-gap recovery, actor recreation, runtime restart during active work, and one dropped committed reply. The dropped-reply check uses a private fault proxy with the fixture certificate. It requires one transmitted submission, one dropped reply, one durable run, and one real file append.

## Pair the packaged app

Use the parent's final debug bundle. Launch its executable with the isolated profile path recorded in `fixture-control.json`:

```sh
BOTSTRAL_DESKTOP_PROFILE=/absolute/acceptance/desktop-profile \
  /absolute/Botstral.app/Contents/MacOS/botstral-desktop
```

1. Open **Paired workspace** and choose **Create pairing request**.
2. Copy the public device fingerprint. Keep the generated device identity in this isolated profile.
3. Run the helper below with that fingerprint. Choose a new private local destination.
4. Import the resulting invitation through the native chooser.
5. Compare the manager identity, endpoint, role, device fingerprint, and exact leaf pin with `fixture-control.json`. Confirm the public relay connection.
6. Send a plain message. Observe the recorded assistant response and durable run history.
7. Expand execution settings. Select the fixture bot and existing Computer session. Set the printed runtime digest and guest account identifiers to 1000.
8. Send `C02 browser and file work`. Review the exact browser, DOM, file, and screenshot actions. Approve them. Save the verified text and image through native choosers.
9. Send `C02 cancel process group`. Approve the process. Cancel it after it starts. Observe the pending cancellation and authoritative terminal result.
10. Pause and resume. Quit and relaunch with the same isolated profile. Confirm the recovered history.
11. Record the walkthrough without showing the invitation contents. Capture native chooser behavior and actual assistant text.
12. Sign out before cleanup.

The invitation helper authenticates only to the fixture's loopback administration through SSH. It writes a mode 0600 local file and never prints the invitation secret:

```sh
python3 manager/tests/c02_production.py invite \
  --fixture /absolute/acceptance/fixture-control.json \
  --fingerprint PUBLIC_DEVICE_FINGERPRINT \
  --output /private/path/c02-invitation.json
```

Invitations expire after ten minutes. Reissue one if it expires. A new invitation must bind the fingerprint created by the packaged app.

## Remove the fixture

Run the exact cleanup helper for the retained fixture:

```sh
python3 manager/tests/c02_production.py cleanup \
  --fixture /absolute/acceptance/fixture-control.json
```

The helper stops only the recorded units, verifies their original processes are absent, checks the LAN and relay endpoints refuse connections, and removes both private remote roots. It records `cleanup.json`. Delete the imported invitation file and isolated app profile after sign-out. The separate build root in `build.json` can be removed after no fixture uses its outputs.

Physical sleep, keychain denial, delivered operating-system notifications, hostile packaged navigation, and the final interaction video require parent observation. A passing service result does not close those gates.
