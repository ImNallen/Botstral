# Connect an owner protocol client

M05 provides the native `owner-probe` and reusable `manager::owner_client` library. C02 supplies graphical pairing, platform keychain storage, and native notification display.

## Pair through trusted local administration

1. Run `owner-probe init /private/client "My device"`. Save the public pairing request from standard output. The client saves its random 256-bit credential before requesting an invitation.
2. Sign in through the existing loopback owner interface. Independently confirm the pairing request's fingerprint. Send that request and `lifetime_seconds` to `POST /v1/device-invitations`. Lifetimes are at most 600 seconds.
3. Export trust with authenticated `POST /v1/device-trust`. Supply `endpoint`, `server_name`, `ca_pem`, and `certificate_pem`. The Manager supplies its durable identity and validates the certificate chain, name, time, and leaf fingerprint.
4. Combine the invitation's `role`, `fingerprint`, `expires_at`, and `invitation` with the exported `trust` object. Confirm that the grant's `manager` equals `trust.manager`. Transfer this bundle through the trusted local channel in a mode 0600 file.
5. Run `owner-probe import /private/client /private/invitation.json`. Run `owner-probe watch /private/client 60` to authenticate and recover.

Invitations are long random secrets. There is no short-code flow. A stolen invitation cannot enroll another credential. A stolen device credential can impersonate that device until revocation. List devices with authenticated local `GET /v1/devices`. Revoke a device with local `DELETE /v1/devices/{id}` and an empty JSON body.

The owner probe uses a mode 0700 directory, mode 0600 files, exclusive process locking, and atomic writes. The desktop uses the same extracted transport with an injected native Keychain or Secret Service record. It never creates `device.json`. One client admits one live session, and that session retains the directory lock until it ends. It tries its saved credential before redeeming an invitation. A lost successful enrollment response therefore retains the same device. Revoked credentials cannot enroll again.

## Configure the public listener

Configure one listener with `--public-listen`, `--public-tls-cert`, `--public-tls-key`, and `--public-worker-ca`. The old `--worker-listen`, `--worker-tls-cert`, `--worker-tls-key`, and `--worker-client-ca` flags remain aliases. Supplying both names for one setting is an error. `WorkerTls` and `serve_with_worker` remain available to Rust callers.

`listeners.json` reports the same address under `public` and legacy `worker`. The existing loopback owner and setup listeners remain separate. The public listener exposes only the worker protocol, production coding proxy, and `GET /owner/v1/connect`. It does not expose ordinary owner HTTP administration or setup. Forwarded address headers do not grant local authority.

Workers present certificates validated against the configured worker CA. Owners omit client certificates and use their separate device credential. Certificate-bearing requests cannot use owner routes. Workers retain wire version 3 and their existing paths. Owners use wire version 1.

## Resume and call methods

Run `owner-probe call /private/client /private/operation.json` with a typed operation from [the generated protocol schema](../link/schema/protocol.json). The allowlist covers task, approval, permission, conversation, message, document, inference, run, computer-session, fenced input, receipt, coding-call, and artifact operations. Local password, session, vault, provider-secret, invitation, and device administration are excluded.

Every reply uses `transfer_start`, ordered `transfer_chunk` messages, and `transfer_complete`. The start binds a transfer identity, request identity, total bytes, chunk count, and SHA-256 digest. Chunks carry at most 24 KiB before base64 encoding. Every WebSocket frame stays below the existing 64 KiB limit. One connection has one operation or transfer at a time. Transfers have an 80 MiB total limit and a 120-second deadline. [The complete snapshot bound](m05-snapshot-bound.md) fits those limits.

The server coalesces committed event-head wake hints. Each viewer has independent bounded control and bulk queues. The client subscribes before its first replay. It validates complete transfers in staging storage, fetches current authoritative state for event references, and atomically installs the image with its application cursor. Interrupted recovery preserves the previous cursor. A retention gap produces a complete M03 snapshot. Notifications contain only event-head hints and cannot grant approval authority.

The native driver sends heartbeats, detects dead peers, and reconnects with bounded exponential backoff and jitter. Successful authenticated recovery resets the delay. Capacity and durable rate limits cause bounded retry. Pin failures, invalid credentials, and revocation require explicit action. A lost mutation response is `UnknownOutcome`. Reconcile with authoritative reads and existing stable operation identities. The client never automatically replays arbitrary mutations.

Storage and protocol failures after mutation transmission also produce `UnknownOutcome` until a validated result or explicit refusal is available. Transient TLS peer alerts retry without replacing the pin. Failed anonymous attempts cannot exhaust valid owner authentication or worker enrollment.

## Replace trust locally

Export the replacement certificate through authenticated local `/v1/device-trust`. Verify the durable Manager identity. Transfer the profile in a protected file and run `owner-probe trust /private/client /private/replacement-trust.json`. Coordinate certificate activation and reconnect. An import for another Manager is rejected.

The client validates the normal TLS chain, name, validity, handshake signatures, and exact leaf pin before sending either credential. Every reconnect applies the same rule. A client that misses rotation remains disconnected until trusted local recovery. Connection failures never discard or replace pins. There is no online rotation handshake or owner certificate authority.

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

Open the paired workspace and create a device request. Its public fingerprint appears only after native protected storage has persisted and read back the credential. Authorize that fingerprint through trusted local administration. Import the protected invitation file through the native chooser, or sign in locally and choose **Authorize with local manager** with the authenticated trust export. Compare the displayed manager identity, endpoint, server name, owner role, device fingerprint, and exact certificate pin before confirming.

The desktop requests the additive version 1 `Workspace` image. The manager constructs its roster, sessions, run details, existing snapshot, and cursor under one serialized authority. `Resume` and its strict snapshot shape remain unchanged for existing clients. An unsupported Workspace reply blocks the app and requires a manager upgrade. Remote tasks never fall back to local HTTP.

The graphical composer starts separate durable M04 runs. Select the approved worker account, process policy, browser runtime digest, and budgets in execution settings. Browser outputs use the `text` and `image` slots. Missing configuration, locked vault, absent worker, unconfirmed session, disabled permission, and unresolved mutations block submission explicitly.

Action review displays the normalized task command and target. `GrantReviewedApproval` compares that exact command, complete prior approval state, and current permission revision at the manager boundary. A changed action, revoked approval, or stale permission revision refuses the grant. Existing `GrantApproval` remains available to older clients.

Notifications are hints. The app recovers a complete image before enabling actions. Cancellation remains pending until authoritative terminal evidence. A lost mutation reply retains its saved identity without replay. An owner can leave an uncertain identity unresolved and explicitly continue with other work after acknowledging the duplication risk.

Sign-out removes local access. A durable tombstone blocks automatic authentication even if native deletion fails. Server revocation remains a separate local administration action. Admitted effects may finish. To replace a certificate, import an authenticated replacement trust export in workspace details and confirm the same durable manager identity and new pin. Connection failures never change pins.

The [native workflow](c02-native-workflow.md) provides a recorded-provider fixture. The [acceptance record](c02-acceptance.md) separates service checks from packaged and production-worker acceptance.
