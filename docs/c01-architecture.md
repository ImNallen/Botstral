# Desktop architecture

Botstral desktop provides local owner sign-in, a real bot roster, idempotent bot creation, manager status, a native tray, and an explicit notification test. Chat, tasks, computer control, pairing, and remote access remain unavailable. The interface does not simulate activity.

## Run the desktop

Use Rust 1.93.0, Node.js 24 or later, npm, and your platform's [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/). macOS requires the command-line developer tools. Linux requires the WebKitGTK and tray development packages listed in `.github/workflows/desktop.yml`.

Start an initialized manager with `make dev`. Create its owner through the [local API setup instructions](local-api.md). The desktop does not expose owner setup, vault entry, provider-key entry, or worker control.

```sh
npm ci
make desktop
```

Sign in with the owner password and a literal loopback socket address. The default is `127.0.0.1:43120`. Hostnames, URLs, non-loopback addresses, and port zero are rejected. IPv6 loopback uses `[::1]:43120`.

Select a bot in the roster. The circular search button or Command-K or Control-K reveals search for actual bot names. Escape dismisses search and restores the full roster. Search clears on sign-out and manager changes. Use the circular plus button to create a bot. Click the centered bot title to toggle details for manager reachability, session state, workers, vault state, and provider metadata. The owner initial opens refresh, theme, details, notification-test, and sign-out controls. **Connect apps** explains that app connections are unavailable.

Closing the window hides it. Use **Show Botstral** in the native tray to restore it. **Quit Botstral** exits. **Test notification** submits a fixed test notification and reports the result. The desktop notification plugin reports permission as granted on desktop platforms. That result does not prove system authorization or delivery. Operating-system settings can suppress notifications.

Sign-out clears the native credential and protected interface immediately. Server revocation is best effort and unconfirmed. Existing requests may still commit. Revocation can be skipped when the operation admission limit is full, interrupted by process exit, or fail while the manager is unavailable. Other sessions remain independent. Restarting the manager invalidates all manager sessions.

## Build and verify

The desktop crate is excluded from the server Cargo workspace and has its own lockfile. Existing `make build` and `make verify` do not acquire GUI dependencies.

```sh
make verify
make verify-desktop
npm run desktop -- build --bundles app
```

`make verify-desktop` checks formatting, Rust-to-schema drift, TypeScript byte drift, TypeScript compilation, frontend behavior, the production frontend build, Rust formatting, clippy, native tests, actual-manager acceptance, and a packaged-content debug binary. It does not launch a window or prove tray behavior and notification delivery. CI runs this check on Linux and macOS with explicit native prerequisites.

The macOS build produces `desktop/src-tauri/target/release/bundle/macos/Botstral.app`. Launch it with:

```sh
open desktop/src-tauri/target/release/bundle/macos/Botstral.app
```

The directly executable binary is `desktop/src-tauri/target/release/botstral-desktop`. Release signing, notarization, updates, and cross-platform distribution acceptance remain later work.

## Native UI fixture

The fixture starts the actual manager in a temporary directory. It creates a synthetic owner and three real bot records. It creates no messages, tasks, worker activity, or provider secrets.

```sh
cargo build --locked -p botstral-manager
python3 desktop/fixtures/local_manager.py
```

Sign in at `127.0.0.1:43120` with the fixture password `SYNTHETIC desktop password`. Stop the fixture with Control-C. The launcher stops its child manager and removes its temporary state. Use `--empty` to inspect the empty roster and `--listen 127.0.0.1:0` to choose a free port. The launcher prints the actual address.

Verify these behaviors in the actual app:

1. Inspect the signed-out layout, both themes, and details.
2. Sign in by keyboard. Confirm that the password clears on submission.
3. Search and switch between the fixture's bots.
4. Create a bot and confirm that it appears once after refresh.
5. Stop the fixture and refresh. Confirm that the app distinguishes its cached roster from current manager reachability.
6. Test a notification and inspect the operating system's result.
7. Close the window, restore it from the tray, and quit from the tray.
8. Sign out and confirm that protected owner and bot data clear.

The packaged release app rendered and passed native sign-in, password clearing, bot selection, search, creation, refresh, details, both themes, unavailable connections, sign-out, and empty-roster checks. A bot created in the app remained present after desktop relaunch. A separate manager at a non-default port retained its address after sign-out. Stopping the manager preserved a clearly labelled cached roster and reported it offline.

The [native walkthrough](assets/c01-desktop.mp4) records synthetic fixture data at three times the captured speed. It contains no reference conversation content. Closing the native window left its process alive. The automation could not inspect the hidden tray menu, so **Show Botstral** remains a manual check. Notification submission returned success to the app, with system delivery explicitly unconfirmed.

## Public application contract

`desktop/src/domain.ts` defines the reusable `Application` interface. It has `inspect`, `signIn`, `refresh`, `createBot`, `signOut`, and `testNotification`. `desktop/src/application.ts` owns lifecycle and creation state without React or Tauri imports. `desktop/src/native.ts` is the Tauri adapter. It parses every native success and failure with Zod. Unexpected fields, malformed values, and credential-bearing snapshots are rejected.

Application snapshots have a signed-out or signed-in discriminant, a monotonic generation and revision, a literal manager endpoint, and a connection observation. Signed-in snapshots include owner, bots, session expiry, vault and provider metadata, and worker summaries. Signed-out snapshots contain no owner or bot records. Bearer credentials never cross the native boundary. Unknown native errors become fixed application codes. Raw server bodies and transport diagnostics do not reach the renderer.

Creation accepts an explicit UUID and name. An uncertain outcome keeps both unchanged, including a successful PUT followed by a failed snapshot refresh. **Retry same bot** repeats that pair against the same manager. **Keep for later** retains it in memory. Explicit sign-out and switching managers abandon the saved draft. A prior request can still finish. Credentials and drafts are not persisted.

The frontend rejects lifecycle results from earlier sign-ins and sign-outs. It also rejects snapshots older than the accepted generation and revision. Password fields clear on submission. No credentials enter browser storage.

## Native ownership and request limits

`desktop/src-tauri/src/local.rs` owns the HTTP client and an ephemeral native bearer. A short-held mutex protects auth state and snapshots. A separate semaphore serializes complete network operations. At most five operations are admitted, including the running operation. Gate acquisition has a 24-second deadline. Sign-out does not wait for the gate before clearing auth and advancing its generation.

Each operation captures its generation. It checks that generation after acquiring the gate and before publishing. Old unauthorized responses cannot clear a newer auth generation. Queued operations from a signed-out generation cannot send writes. A discarded late sign-in grant receives bounded best-effort revocation. No auth-state mutex stays locked over a network request.

The client accepts only parsed literal loopback socket addresses. Routes and methods are a private enum restricted to health, session creation and deletion, owner, bot list and creation, status, and workers. There is no general HTTP, shell, filesystem, setup, secret-entry, or worker-control command. Environment proxies and redirects are disabled. Connection timeout is one second. Each request and response has a four-second total deadline. Response bodies are limited to one mebibyte, including responses without a declared length. Manager mutations remain subject to the manager's stricter request limits.

Worker observations come from `GET /v1/workers`. The legacy `/v1/status.worker` marker does not determine worker connectivity. Reachability, session authentication, worker readiness, vault state, and provider configuration remain separate facts. Provider metadata does not prove a functioning model connection.

`desktop/src-tauri/src/shell.rs` limits application commands to the main window and the packaged document. Development additionally permits the exact local Vite origin. Other navigation and new windows are denied. Rejected callers receive only a fixed unauthorized code. The main-window capability grants the six application commands and only the core window dragging command. It grants no general native plugins. The native command names are `inspect`, `sign_in`, `refresh`, `create_bot`, `sign_out`, and `test_notification`. The first five return validated credential-free snapshots. `test_notification` returns `submitted`, `denied`, or `unavailable`. The renderer's production content security policy does not permit direct manager connections or remote content. Existing manager browser protections remain unchanged.

## Shared protocol bindings

`link/src/lib.rs` remains the M02 authority. `scripts/generate-link.mjs` compares its Rust-exported JSON Schema byte for byte with `link/schema/protocol.json`. It uses the pinned `json-schema-to-typescript` generator to create `link/typescript/protocol.d.ts`. `@botstral/link` exposes those declarations without React or Tauri dependencies.

```sh
npm run bindings:generate
npm run bindings:check
npm run check --workspace @botstral/link
```

The check rejects both Rust schema drift and generated TypeScript drift. Local desktop application views remain separate from the M02 worker protocol. The desktop does not claim to execute that protocol or pair a remote device.

## Decisions and reconciliation

The accepted design used complete native application operations and credential-free snapshots. A separate auth mutex and bounded operation gate replace a broker that would queue sign-out behind network work. Endpoint, auth generation, and snapshot revisions stay under one native owner. This keeps stale-response policy consistent across refresh, sign-in, and creation.

The existing JSON Schema supplies generated TypeScript declarations. Moving manager HTTP data types into the worker protocol would change working M01 interfaces and mix unrelated contracts. The renderer validates its distinct native application contract at its boundary.

The excluded desktop crate preserves the existing server production build. Root npm workspaces share tooling and the protocol package. Component code and packaging stay under `desktop/`; shared schemas and bindings stay under `link/`.

The visual specification is the screenshot supplied by the owner. It replaces the earlier web-article reference. The screenshot and its private conversation content are not included in the repository. The layout has a full-height 280-pixel gray sidebar, circular search and creation controls, a centered 96-by-92-pixel tile for the first actual bot, compact rows for the rest, and bottom owner and app-connection controls. Original saturated organic avatars have white eyes. The selected bot title floats in a small capsule at the top center. The conversation has a neutral unavailable-messaging status bubble and a single 44-pixel disabled composer capsule inset 16 pixels at the bottom. Both themes share this geometry. No conversation content, activity, or favorite metadata is invented.

On macOS, the window uses an overlay title bar and hidden title. Native traffic lights remain at the top left. The top regions allow window dragging. macOS-specific builder calls are compile-time guarded. Other platforms retain native decorations. Native launch and visual acceptance of this composition passed on macOS.

Two defects were found during the earlier native walkthrough. Tauri loads its packaged root as `tauri://localhost` with an empty path, so the navigation policy admits that exact root along with its slash and index variants. The sign-out command is asynchronous because local sign-out starts bounded best-effort revocation with `tokio::spawn`. Tauri must dispatch that command inside its asynchronous runtime. Both fixes remain in the corrected layout.

The Model the Domain principle shaped the discriminated snapshots and explicit uncertain creation state. Boundary Discipline shaped the fixed native commands and Zod parsing. Type System Discipline kept external values unknown until parsed. Prove It Works required actual-manager requests and native packaging. Attack the Premise led to loading the failed release dylibs directly before changing the release profile.

## Implementation evidence

The implementation checks ran on macOS 27.0.1 arm64 with Rust 1.93.0, Node.js 26.12.1, npm 11.20.0, and Apple's command-line developer tools.

| Command | Observed result |
| --- | --- |
| `npm run bindings:generate` | Generated declarations from the committed Rust schema. |
| `npm run bindings:check` | Rust schema and TypeScript byte comparisons passed. |
| `make verify-desktop` | Formatting, typechecks, 16 frontend tests, production frontend build, clippy, two native shell policy tests, five native lifecycle and transport tests, actual-manager acceptance, and packaged-content debug binary passed before the final endpoint regression was added. |
| `npm run format:check && npm run check && npm test` | Final formatting, binding drift, typechecks, and 17 frontend tests passed, including the non-default manager address regression. |
| `make verify` | Existing manager and worker default, test-feature, production, and release-refusal checks passed. |
| `npm run desktop -- build --bundles app` | Built the corrected release macOS application after all interface edits. The compile-time stripping correction below remains in effect. |
| `python3 desktop/fixtures/local_manager.py --listen 127.0.0.1:0` | Started the real manager, created the synthetic owner and bots, printed its address, and stopped cleanly on Control-C. |

The native transport tests send a real redirect Location header to a second listening endpoint and confirm that no redirected request occurs. They exercise response size and timeout limits, late-grant revocation, sign-out while writes are queued, an old unauthorized response during a newer sign-in, and retry after post-PUT refresh failure. Actual-manager acceptance verifies owner retrieval, idempotent creation, status and worker reads, local sign-out, and normal best-effort server revocation through an independent observer session.

Initial release packaging failed with Rust E0463 for present proc-macro dylibs. Loading those exact files with `ctypes.CDLL` reported `mis-aligned LINKEDIT string pool`. This matches [Rust issue 157750](https://github.com/rust-lang/rust/issues/157750). The desktop profile now disables stripping for compile-time build dependencies while retaining stripping for its shipped binary. The release app then built successfully. The server release profile did not change.

Earlier logs are in `.audit/c01/desktop-verification.log`, `.audit/c01/server-verification.log`, and `.audit/c01/macos-package-final.log`. The reference correction records its coupled check in `.audit/c01/reference-desktop-verification.log`. Final frontend checks and packaging are in `.audit/c01/final-frontend-tests.log` and `.audit/c01/final-native-package.log`. These ignored logs are local evidence, not portable CI artifacts. Native launch, visual inspection, and interactive video acceptance passed. Tray restoration, system notification delivery, other operating systems, signing, notarization, and remote access remain unverified.
