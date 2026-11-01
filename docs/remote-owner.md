# Connect an owner protocol client

M05 provides the native `owner-probe` and reusable `manager::owner_client` library. C02 supplies graphical pairing, platform keychain storage, and native notification display.

## Pair a device with a join code

On the manager host, after first-owner setup, print a device join code and a QR code for the same text. The command reads the owner password from the terminal or from standard input. It does not take a device fingerprint.

```sh
./target/release/botstral-manager invite device --label "My desktop" --state-dir manager-state
```

The code is `bj1/device/HOST:PORT/PIN/SECRET`. The desktop pastes that string, or a QR decode of it. It shows the manager pin. Confirm that pin before continuing. No further loopback sign-in on the manager host is required for pairing.

The owner API equivalent is `POST /v1/device-invitations` with `{"label":"My desktop","lifetime_seconds":600}`. The response includes `code`. List devices with authenticated local `GET /v1/devices`. Revoke a device with local `DELETE /v1/devices/{id}` and an empty JSON body.

The owner probe uses a mode 0700 directory, mode 0600 files, exclusive process locking, and atomic writes. The desktop uses the same extracted transport with an injected native Keychain or Secret Service record. It never creates `device.json`. One client admits one live session, and that session retains the directory lock until it ends. It tries its saved credential before redeeming an invitation. A lost successful enrollment response therefore retains the same device. Revoked credentials cannot enroll again.

## Configure the public listener

```sh
./target/release/botstral-manager --state-dir manager-state \
  --listen 127.0.0.1:43120 --setup-listen 127.0.0.1:43121 \
  --worker-listen 192.168.1.40:43122
```

Bind the address devices should dial. That address is copied into the join code. `--public-listen` is an alias of `--worker-listen`. The manager generates the TLS identity. Certificate and CA flags are not accepted.

`listeners.json` reports the same address under `public` and `worker`. The existing loopback owner and setup listeners remain separate. The public listener exposes the worker protocol, production coding proxy, and `GET /owner/v1/connect`. It does not expose ordinary owner HTTP administration or setup. Forwarded address headers do not grant local authority.

Worker routes require a client certificate whose Ed25519 public key the manager can extract. Owner routes require the absence of a client certificate and use the device credential. Workers retain wire version 3 and their existing paths. Owners use wire version 3 of the owner hello.

## Resume and call methods

Run `owner-probe call /private/client /private/operation.json` with a typed operation from [the generated protocol schema](../link/schema/protocol.json). The allowlist covers task, approval, permission, conversation, message, document, inference, run, computer-session, fenced input, receipt, coding-call, and artifact operations. Local password, session, vault, provider-secret, invitation, and device administration are excluded.

Every reply uses `transfer_start`, ordered `transfer_chunk` messages, and `transfer_complete`. The start binds a transfer identity, request identity, total bytes, chunk count, and SHA-256 digest. Chunks carry at most 24 KiB before base64 encoding. Every WebSocket frame stays below the existing 64 KiB limit. One connection has one operation or transfer at a time. Transfers have an 80 MiB total limit and a 120-second deadline. [The complete snapshot bound](m05-snapshot-bound.md) fits those limits.

The server coalesces committed event-head wake hints. Each viewer has independent bounded control and bulk queues. The client subscribes before its first replay. It validates complete transfers in staging storage, fetches current authoritative state for event references, and atomically installs the image with its application cursor. Interrupted recovery preserves the previous cursor. A retention gap produces a complete M03 snapshot. Notifications contain only event-head hints and cannot grant approval authority.

The native driver sends heartbeats, detects dead peers, and reconnects with bounded exponential backoff and jitter. Successful authenticated recovery resets the delay. Capacity and durable rate limits cause bounded retry. Pin failures, invalid credentials, and revocation require explicit action. A lost mutation response is `UnknownOutcome`. Reconcile with authoritative reads and existing stable operation identities. The client never automatically replays arbitrary mutations.

Storage and protocol failures after mutation transmission also produce `UnknownOutcome` until a validated result or explicit refusal is available. Transient TLS peer alerts retry without replacing the pin. Failed anonymous attempts cannot exhaust valid owner authentication or worker enrollment.

## Replace trust locally

Export the current pin through authenticated local `POST /v1/device-trust` with the endpoint. Verify the durable Manager identity and the pin. Transfer the profile in a protected file and run `owner-probe trust /private/client /private/replacement-trust.json`. An import for another Manager is rejected.

The client checks that the server certificate's Ed25519 public key matches the pin. It does not check a CA or a hostname. Every reconnect applies the same rule. A client that misses a manager-key change remains disconnected until it receives a new pin. Connection failures never discard or replace pins.

## Use an outbound relay

The endpoint is configurable. A lasting deployment can use [rathole's raw TCP forwarding](https://github.com/rathole-org/rathole#configuration). The Manager-side process connects outward to the relay and forwards the public service to the single Manager TLS listener. For example, the Manager-side configuration is:

```toml
[client]
remote_addr = "relay.example:2333"
[client.services.manager]
token = "<relay service secret>"
local_addr = "127.0.0.1:9443"
```

The relay configuration is:

```toml
[server]
bind_addr = "0.0.0.0:2333"
[server.services.manager]
token = "<same relay service secret>"
bind_addr = "0.0.0.0:9443"
```

Provision the relay secret locally and protect both configuration files. The Manager certificate covers the validated server name in the client profile. The relay receives neither the Manager TLS key nor device credentials. It can observe traffic metadata and deny service. No home-router forwarded port is required.

M05 acceptance uses temporary [bore v0.6.0](https://github.com/ekzhang/bore/releases/tag/v0.6.0) forwarding to `bore.pub`. This is a synthetic acceptance route, not a permanent deployment or service guarantee. SSH only bootstraps fixtures and collects evidence. Owner and worker protocol data travel over the observed LAN or relay TCP connection.

## Rerun acceptance

Run `cargo build --workspace --locked`, then:

```sh
python3 manager/tests/m05_acceptance.py \
  --manager-bin target/debug/botstral-manager \
  --probe-bin target/debug/owner-probe \
  --output .audit/m05-local
```

Run the two-VM fixture with Linux binaries from the reviewed source and a checksum-verified bore v0.6.0 binary:

```sh
python3 manager/tests/m05_two_vm.py \
  --binaries /private/linux-binaries \
  --bore-bin /private/bore \
  --output .audit/m05-two-vm
```

The script uses the configured `botstral-manager` and `botstral-computer` SSH aliases. It creates unique private roots and transient units, verifies deployed binary hashes, compares direct and relayed Manager leaves, and exercises the real native client and production worker. It records cleanup before removing temporary credentials and fixture roots. A failed gate leaves the run incomplete.

Run the native client review regressions after building their contract executable:

```sh
cargo build -p botstral-manager --locked --bins --example m05-client-contract
python3 manager/tests/m05_client_regressions.py --output .audit/m05-client-regressions
```


## Use the paired desktop

Open the paired workspace and create a device request. Its public fingerprint appears only after native protected storage has persisted and read back the credential. Paste the device join code. Compare the displayed manager identity, endpoint, manager pin, owner role, and device fingerprint before confirming.

The desktop requests the additive version 1 `Workspace` image. The manager constructs its roster, sessions, run details, existing snapshot, and cursor under one serialized authority. `Resume` and its strict snapshot shape remain unchanged for existing clients. An unsupported Workspace reply blocks the app and requires a manager upgrade. Remote tasks never fall back to local HTTP.

The graphical composer starts separate durable M04 runs. Select the approved worker account, process policy, browser runtime digest, and budgets in execution settings. Browser outputs use the `text` and `image` slots. Missing configuration, locked vault, absent worker, unconfirmed session, disabled permission, and unresolved mutations block submission explicitly.

Action review displays the normalized task command and target. `GrantReviewedApproval` compares that exact command, complete prior approval state, and current permission revision at the manager boundary. A changed action, revoked approval, or stale permission revision refuses the grant. Existing `GrantApproval` remains available to older clients.

Notifications are hints. The app recovers a complete image before enabling actions. Cancellation remains pending until authoritative terminal evidence. A lost mutation reply retains its saved identity without replay. An owner can leave an uncertain identity unresolved and explicitly continue with other work after acknowledging the duplication risk.

Sign-out removes local access. A durable tombstone blocks automatic authentication even if native deletion fails. Server revocation remains a separate local administration action. Admitted effects may finish. To replace the manager pin, import an authenticated replacement trust export in workspace details and confirm the same durable manager identity and new pin. Connection failures never change pins.

The [native workflow](c02-native-workflow.md) provides a recorded-provider fixture. The [acceptance record](c02-acceptance.md) separates service checks from packaged and production-worker acceptance.
