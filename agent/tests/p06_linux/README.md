# Linux protected sign-in fixture

This fixture uses the production manager, worker, systemd backend, Unix peer admission, mutual TLS endpoint, paired binary entry, owner approval, and sealed Computer runtime. It uses no test bridge or inference provider. All credentials and certificates are generated inside an owned container. Containers publish no host ports.

The driver validates one exact synthetic POST and an applied review. It checks a peer outside the cgroup, a missing client certificate, a changed generation, and a repeated release separately. Evidence contains public metadata, process groups, command exits, and fixed outcomes.

On ARM Docker hosts, systemd runs natively and the x86_64 Rust, Node, and Chromium binaries use the host's existing emulation. The production architecture checks, seals, resource controllers, and deadlines remain active. Pure amd64 systemd failed to start services on the reviewed Docker Desktop host.

The fixture masks systemd's binfmt, sysctl, and module services before boot. Its runner compares Docker's emulation registrations before and after execution. Do not run an unmasked privileged systemd image.

Build release binaries with Rust 1.93, its installed `x86_64-unknown-linux-gnu` target, and Zig available. Choose fresh output and evidence directories.

```sh
python3 agent/tests/p06_linux/build.py --output /tmp/botstral-p06-build-UNIQUE
python3 agent/tests/p06_linux/run.py \
  --binary-directory /tmp/botstral-p06-build-UNIQUE/target/x86_64-unknown-linux-gnu/release \
  --evidence /tmp/botstral-p06-full-UNIQUE
```

The default run installs and seals the full runtime with `agent/runtime/setup.sh`. It records a failure when the production probe refuses.

An explicit smaller fixture removes only the unused full-Chromium and FFmpeg directories, then runs the unchanged `seal.py`. It retains the headless browser, Node, dependencies, and runtime source. Both this fixture and the full runtime passed on the reviewed host. Earlier full-runtime attempts missed the helper readiness deadline.

```sh
python3 agent/tests/p06_linux/run.py \
  --binary-directory /tmp/botstral-p06-build-UNIQUE/target/x86_64-unknown-linux-gnu/release \
  --headless-only --evidence /tmp/botstral-p06-headless-UNIQUE
```

Use `--image IMAGE` to reuse an already built fixture image. The runner removes only its own container. Built images remain for inspection. The smaller fixture alone does not certify the full installation. Neither fixture covers a packaged macOS native-to-Linux session.

The current driver also performs two server-validated TOTP submissions. Both must wait for a fresh owner decision, even though the item permits remembered password rules. The fixture waits for an adequate current-code window before approval. It uses an explicit persistent session cookie because a session-only cookie is not promised to survive the runtime's browser process ending.

Use the optional packaged-client mode to verify actual macOS native input and approval against Linux. It publishes only the paired TLS endpoint on host loopback. It does not publish manager administration or the receiving website.

```sh
python3 agent/tests/p06_linux/run.py \
  --binary-directory /tmp/botstral-p06-build-UNIQUE/target/x86_64-unknown-linux-gnu/release \
  --native-port 44329 --evidence /tmp/botstral-p06-native-UNIQUE
```

The owned container name appears in `commands.json`. Its private fixture root is `/home/p06/fixture`. Complete these steps through an isolated packaged app profile.

1. Create a native pairing request. Write its public fingerprint to the container's `native-fingerprint.txt` as UID 10001. Read `native-entry.json` for the exact origin, bot and manager identity.
2. Copy `native-invitation.json` out of the container to an owned file with permissions 0600. Import it through the app and compare the manager identity and certificate pin with the authenticated fixture export. The invitation is private and must not be included in evidence.
3. Use the label `Packaged Linux account`, the exact generated origin, and the selected bot. Enter only the disposable synthetic constants from the `native_port` branch of `fixture.py` through actual native controls. Verify the committed receipt, metadata-only listing and cleared fields when reopening entry.
4. Create `native-start` inside the container only after the app is ready. Wait for `native-review.json`, then approve the exact live form through the app before its normal expiry. The fixture does not approve on the packaged client's behalf or extend the production review deadline.
5. Require `native-completed.json` and the server-validated sign-in result. Verify the app's completed task and applied submission. Sign out and close the app, remove its owned keychain credential, then create `native-finish` to let the driver clean up.

Packaged mode runs one password flow. The automated lane covers the two TOTP approvals separately. Read-only `busy` responses retry for at most ten seconds; writes do not automatically retry. Helper sampling records public runtime file paths during integrity checks. Every run still compares Docker emulation registrations before and after execution and removes its own container.
