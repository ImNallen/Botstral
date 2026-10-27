# Botstral

Botstral is a self-hosted bot manager built around Mistral-developed models. It retains one owner's bots and encrypted provider credentials. A separate Rust daemon connects through mutual TLS WebSockets and tracks logical bot sessions. Durable tasks, approval claims and Markdown memory recover through manager and worker restarts. A Tauri desktop app manages the local owner session and bot roster. Model execution follows later.

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

The Linux worker can run one explicitly approved guest command through the task API. Execution requires opt-in configuration and systemd 255. [The process guide](docs/w01-process-acceptance.md) explains setup and verification. [The verification record](docs/w01-verification.md) covers bounded output, descendant cancellation, and recovery across both VMs. [Computer work](docs/w01-computer.md) adds typed browser and file commands, shared persistent login state, and durable owner-downloadable artifacts through the same supervisor. Model integration, owner-device pairing, and remote access remain later work.

## Run the desktop

Install Node.js 24 or later and the [Tauri platform prerequisites](https://v2.tauri.app/start/prerequisites/). Start and initialize the local manager, then run:

```sh
npm ci
make desktop
```

Sign in with the local owner password. The app lists and creates real bots and shows manager connection details. Messaging and task execution are unavailable. See the [desktop architecture and verification guide](docs/c01-architecture.md) for packaging, native UI fixtures, and acceptance gaps. Run `make verify-desktop` for desktop checks.
