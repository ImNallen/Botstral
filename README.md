# Botstral

Botstral is a self-hosted bot manager built around Mistral-developed models. It retains one owner's bots and encrypted provider credentials. A separate Rust daemon connects through mutual TLS WebSockets and tracks logical bot sessions. Durable tasks, approval claims and Markdown memory recover through manager and worker restarts. A Tauri desktop app pairs through the owner WSS protocol and retains its device credential in the native keychain. The manager supports explicitly configured text inference and durable tool runs.

## Repository layout

`manager/` contains the manager, `agent/` contains the worker daemon, and `link/` contains shared schemas and Rust and TypeScript bindings. `desktop/` contains the Tauri app. `mobile/` follows later. Each component owns its code, tests, configuration, and packaging. Shared workspaces, tooling, scripts, and documentation stay at the repository root.

## Run the manager

Use Rust 1.93.0 on Linux or macOS with a C compiler for bundled SQLite.

```sh
make verify
make dev
```

Verification uses synthetic credentials, actual binaries over TCP and mutual TLS WebSockets, and separate production and test-worker builds. Development starts the regular listener at `127.0.0.1:43120` and temporary setup at `127.0.0.1:43121`.

Use the [local API reference and protected input example](docs/local-api.md) to create the owner, sign in, and create a bot. The bootstrap capability is in `.state/bootstrap.token` until setup completes. Keep credentials out of command arguments and environment variables.

Keep an offline copy of the separate vault passphrase before setup. Setup leaves the vault locked. Every restart requires sign-in and explicit vault unlock. The [security and recovery reference](docs/security.md) describes the no-escrow policy.

```sh
make build
./target/release/botstral-manager --state-dir .state
```

Stop the manager with Control-C. Its SQLite database and encrypted credentials remain in `.state`. The [M01 architecture](docs/m01-architecture.md) records ownership, interfaces, verification, and design reconciliation.

See [DESIGN.md](DESIGN.md) for the product architecture and [PLAN.md](PLAN.md) for later milestones. Use [the worker deployment guide](docs/worker-deployment.md) to provision certificates and enroll a worker. [The M02 architecture](docs/m02-architecture.md) records verification and remaining gaps. [Two-VM acceptance](docs/m02-two-vm-acceptance.md) passed with isolated production and synthetic test fixtures. [M03 recovery acceptance](docs/m03-acceptance.md) records durable task verification and exact synthetic limits.

The Linux worker can run one explicitly approved guest command through the task API. Execution requires opt-in configuration and systemd 255. [The process guide](docs/w01-process-acceptance.md) explains setup and verification. [The verification record](docs/w01-verification.md) covers bounded output, descendant cancellation, and recovery across both VMs. [Computer work](docs/w01-computer.md) adds typed browser and file commands, shared persistent login state, and durable owner-downloadable artifacts through the same supervisor.

[M04 is complete](docs/m04-completion.md). Model turns use exactly approved browser, file, process and Vibe operations, with bounded PNG vision and durable artifacts. Acceptance combines live EU Mistral work across two VMs with repeatable recorded-provider checks. M05 supplies owner-device pairing and remote access. Packaged desktop acceptance over the configured remote route remains open.

## Run the desktop

Install Node.js 24 or later and the [Tauri platform prerequisites](https://v2.tauri.app/start/prerequisites/). Start and initialize the local manager, then run:

```sh
npm ci
make desktop
```

Use local administration to sign in and manage bots and device grants. In the paired workspace, create a native fingerprint, import a protected invitation or authorize it with the local manager, and confirm the manager identity and certificate pin. Paired work uses WSS throughout.

Each message starts a durable run with real assistant turns and linked tasks. Select the worker account and bounded execution settings, review exact actions before approval, cancel work, and save verified artifacts through a native chooser. Inference configuration, vault unlock, and an approved worker remain explicit prerequisites. Earlier runs stay in history without implying shared model context.

See the [C02 architecture](docs/c02-architecture.md), [native fixture workflow](docs/c02-native-workflow.md), and [acceptance record](docs/c02-acceptance.md). Run `make verify-desktop` for desktop checks. The packaged macOS workflow passed over the public relay with the production worker, including browser work, cancellation and recovery. See the acceptance record for independent review and platform limits.
