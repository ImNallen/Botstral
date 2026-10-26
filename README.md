# Botstral

Botstral is a self-hosted bot manager built around Mistral-developed models. Its first milestone runs locally and retains one owner's bots and encrypted provider credentials. Model execution and native apps follow later.

## Run the manager

Use Rust 1.93.0 on Linux or macOS with a C compiler for bundled SQLite.

```sh
make verify
make dev
```

Verification uses synthetic credentials and real TCP connections. Development starts the regular listener at `127.0.0.1:43120` and temporary setup at `127.0.0.1:43121`.

Use the [local API reference and protected input example](docs/local-api.md) to create the owner, sign in, and create a bot. The bootstrap capability is in `.state/bootstrap.token` until setup completes. Keep credentials out of command arguments and environment variables.

Keep an offline copy of the separate vault passphrase before setup. Setup leaves the vault locked. Every restart requires sign-in and explicit vault unlock. The [security and recovery reference](docs/security.md) describes the no-escrow policy.

```sh
make build
./target/release/botstral-manager --state-dir .state
```

Stop the manager with Control-C. Its SQLite database and encrypted credentials remain in `.state`. The [M01 architecture](docs/m01-architecture.md) records ownership, interfaces, verification, and design reconciliation.

See [DESIGN.md](DESIGN.md) for the product architecture and [PLAN.md](PLAN.md) for later milestones. One manager and a separate Computer VM remain the product target. M01 has no worker, inference, pairing, remote access, or native client.
