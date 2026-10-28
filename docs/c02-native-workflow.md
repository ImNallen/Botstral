# Exercise the paired desktop with synthetic data

This fixture runs the actual manager and owner WSS listener with the recorded M04 provider. Its worker supplies explicitly synthetic evidence. It does not execute a production browser or process. No paid inference is used.

## Run the native contract

Use an unlocked macOS Keychain or supported Linux Secret Service. From the repository root, run:

```sh
python3 desktop/fixtures/paired_manager.py --verify-native
```

The command creates private temporary state, starts the real manager, runs the native service contract against the OS keychain, and stops the fixture. It removes its synthetic keychain entry on success or test failure. The contract also feeds the actual native projections through the strict renderer parser. It covers persisted pairing, real run turns, a linked task, reviewed approval, verified artifact export, synthetic cancellation evidence, suspension, restoration, credential scans, and local revocation.

## Run the graphical workflow

Build the actual native bundle:

```sh
npm ci
npm run desktop -- build --debug --bundles app
python3 desktop/fixtures/paired_manager.py
```

The fixture prints its local address, synthetic password, trust export, browser runtime digest, and isolated debug profile directory. Keep that terminal open. Launch the bundle's executable in another terminal with the printed profile path:

```sh
BOTSTRAL_DESKTOP_PROFILE=/printed/fixture/root/desktop-profile \
  desktop/src-tauri/target/debug/bundle/macos/Botstral.app/Contents/MacOS/botstral-desktop
```

The profile override exists only in debug builds. It keeps the synthetic app identity separate from the normal owner profile. Do not run an isolated fixture against an existing owner profile.

1. Open **Local administration**. Sign in with the printed address and synthetic password.
2. Open **Paired workspace**. Choose **Create pairing request**. Its fingerprint comes from a saved native credential.
3. Choose **Authorize with local manager** and select the printed `trust.json` through the native chooser.
4. Compare the manager identity, endpoint, role, fingerprint, and exact pin. Choose **Confirm and connect**.
5. Send a plain message. Observe the actual recorded assistant turn and durable run history.
6. Expand **Execution settings and prerequisites**. Enter the printed browser runtime digest. Keep the explicitly synthetic fixture account at user 1000 and group 1000.
7. Send `Create a file`. Inspect the generated task and normalized arguments. Choose **Review action**, then **Approve exact action**.
8. Wait for the synthetic worker result and run completion. Choose **Save verified file**. The exported text explicitly identifies its synthetic source.
9. Send `Please cancel a long operation`. Approve its bounded sleep proposal. Cancel the task. Observe cancellation requested until the manager records synthetic stop evidence.
10. Pause and resume the connection from workspace details. Close the window and restore it through the tray menu. Hiding the window keeps the activity listener running. Quit and relaunch with the same debug profile. Observe recovered history and the same native device identity.
11. In workspace details, list paired devices through local administration and revoke the synthetic desktop. Observe protected views clear and future access fail.
12. Sign out of the device before stopping the fixture. Use Control-C in its terminal. The fixture removes its temporary files and processes.

Each message starts a separate run. Prior submissions are visible history, not implicit model context. A production browser task needs the supported Linux worker, its approved account, and its installed runtime digest. A real process cancellation check needs that worker's actual process-group stop evidence.

## Exercise protected transfer and links

To pair another desktop, confirm its fingerprint independently. In **Local device administration**, expand **Invite another desktop**, enter its label and fingerprint, choose the authenticated trust export, and save the protected invitation with the native chooser. Transfer that file privately.

The native handler also accepts `botstral://pair?bundle=` followed by URL-safe unpadded base64 of the invitation bundle. The link contains a short-lived secret and must not enter screenshots, logs, or shared shell history. It stages a candidate without navigating the webview or fetching a URL. The owner must still compare and confirm trust. Protected file import is the preferred fixture path.
