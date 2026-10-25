# Botstral

Botstral is a single-owner assistant system under development. Its manager is an always-on Go server with no user interface. M01 provides a local API with durable owner, session, and bot storage.

Desktop and mobile connect to the manager as clients. The bot virtual machine also stays running independently of those clients. VM connectivity and task execution belong to later milestones.

The Go module lives in `manager/`. Future desktop and mobile apps will live in `desktop/` and `mobile/`, with separate designs. Neither app exists yet. See [the plan](PLAN.md) for later milestones.

## Run the manager

Install Go 1.26 or later and Make. No frontend toolchain is required.

```sh
make deps
make build
./bin/botstral --data-dir /absolute/path/to/botstral-data --port 8080 --setup
```

For an unconfigured installation, `--setup` opens a separate temporary IPv4 loopback listener. Standard error contains one operator-only JSON record with `bootstrapEndpoint`, `capability`, and `expiresAt`. Send a JSON `POST` to that endpoint with `username` and `password`, authenticated by `Authorization: Bearer <capability>`. Omit `Origin`. Setup creates the owner without a session, then closes. The capability expires after fifteen minutes.

Sign in through `POST /api/v1/sign-in` on the main listener with the same credential fields and no Authorization header. Retain the returned token privately. Use `Authorization: Bearer <token>` for subsequent calls. Each sign-in creates an independent seven-day session. Session lists contain metadata only.

Restart with the same data directory. Normal startup needs no `--setup` after owner creation. A configured installation never reopens setup, even with that flag. `make dev` runs the manager with `./data` at the repository root and opts into setup when needed.

The manager binds only `127.0.0.1`. M01 does not provide remote access, device pairing, or TLS. Keep bootstrap capabilities, passwords, and session tokens out of shell history, shared terminals, and logs. Protect the data directory and let the manager enforce its file modes.

## Verify

```sh
make verify
```

This runs Go race tests, vet, a binary build, and an actual-binary acceptance driver. The driver copies the executable outside the checkout and verifies restart persistence, bot identity, session survival, revocation, sign-out, and setup closure.

See [the M01 API reference](docs/m01.md) for request contracts and verification limits.
