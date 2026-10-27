# M05 remote owner access

## Purpose

M05 adds paired owner devices to the existing manager transport. One public TLS listener serves owner devices and workers. Local setup and password administration keep their separate loopback listeners. C02 supplies the desktop pairing interface and platform credential storage.

## Owner workflow

The owner client generates and saves a random 256-bit device credential before enrollment. It exports a public pairing request containing the credential fingerprint and device label. An authenticated local owner creates a ten-minute, single-use invitation for that exact request.

The invitation bundle carries the manager identity, owner role, credential fingerprint, endpoint, certificate trust, exact certificate pin, expiry, and a separate random invitation secret. The owner transfers this bundle through a trusted channel. The client verifies TLS and the pin before transmitting either secret.

The client redeems the invitation with its existing credential. The manager consumes the invitation and creates the device in one transaction. A lost response does not lose the credential. The client first attempts authentication with its saved credential before retrying redemption. Revoked credentials cannot enroll again.

A stolen device credential can impersonate that device until revocation. Binding enrollment to the independently confirmed fingerprint prevents an invitation alone from enrolling a different credential. Protected client storage and revocation address credential exposure.

A native protocol client and probe exercise pairing, supported owner operations, watching, reconnect, and explicit trust replacement. Password login, setup, local session administration, vault unlock, provider-secret entry, and device issuance remain local operations.

## Authority and state

The organizing data shape is an owner authority enum with separate local-session and device-lease variants. Device leases have private constructors. They bind a durable device identity and revocation epoch to an active connection identity.

State remains the only durable writer. Shared owner business operations authorize that enum inside the state queue on every invocation. Existing public local Manager methods keep their SessionToken signatures and convert to local authority. The device gateway calls the same State implementations. It does not manufacture local sessions or copy task and approval logic.

Keep the existing local-session authorization method for local administration. The remote operation enum explicitly lists domain operations and their typed arguments. It cannot dispatch arbitrary HTTP paths or access the local router. It includes the existing task, approval, conversation, document, inference, run, computer-session, fenced-input, and result operations needed by C02. Notifications never grant authority.

Migration 0008 stores a stable manager identity, owner devices, invitation bindings, and durable attempt limits. Store only domain-separated hashes of credentials and invitation secrets. Invitations bind manager, role, target, and expiry. Existing enrolled workers retain their identities, generations, and state. Expire outstanding legacy invitations if their missing bindings cannot be established safely.

Revocation commits a durable revoked state before signaling every active device connection. Each queued operation rechecks authority. Blocked reads and writes select on cancellation, so revocation interrupts recovery transfers. Disconnect cleanup removes only its own connection. Restart retains device credentials and revocations while clearing connection leases and local login sessions.

Use high-entropy invitation bundles. Short codes are outside this implementation. A future short-code flow requires trusted confirmation and bounded guesses. Bound invitations, device count, active connections, pairing attempts, outstanding requests, and handshake duration in the implementation.

## One public listener

Extract public TLS acceptance from worker_http.rs. Its rustls verifier permits an absent client certificate for owner pairing and authentication. Any certificate that is presented must pass the existing worker CA, validity, and signature checks.

Worker paths require the verified certificate capability and existing worker registry checks. Owner paths require the separate device credential and reject certificate-bearing worker requests. Path selection alone cannot establish authority. Preserve existing worker enrollment, WSS, and coding endpoints and payloads.

Keep the old worker configuration as aliases for the same listener where compatibility requires it. Reject conflicting public and legacy options. Do not start another public listener. Preserve existing worker listener metadata for current tools and expose public metadata explicitly.

The public listener never receives LocalSetup or the ordinary loopback router. Setup paths remain absent, including requests with misleading Forwarded and X-Forwarded-For headers. Keep explicit native-origin and hostile-browser-header checks. Credentials never enter URLs or ordinary logs.

## Recovery and client lifecycle

Subscribe to a coalescing event-head watch before the first authoritative replay. State publishes committed event-head changes without waiting for a viewer. Each connection reads retained events from its cursor until caught up. A viewer has at most one outstanding recovery job. Slow viewers cannot block State or fill its queue with backlog.

Reuse the complete M03 snapshot and cursor from one consistent State operation. Preserve every snapshot collection and document reconciliation. Serialize outside the State queue. Derive a conservative total bound from the admitted record counts and sizes, including JSON escaping. Add bounded iteration if a source lacks a finite materialization bound.

Limit concurrent snapshot materializations. Send large snapshots and operation replies through one bounded transfer format with an identity, ordered chunks, byte count, digest, and completion marker. Each frame stays below the existing message limit. Bound queued bytes, transfer lifetime, outstanding transfers, and temporary storage. Resource refusal is explicit and never a truncated success.

The client writes to protected staging storage and verifies the complete transfer before atomically installing state and cursor. Interrupted transfers leave the previous cursor intact. Event references cause authoritative reads before the client commits its corresponding state. Those reads represent current state rather than a historical image at the event cursor. Retention rollover during recovery starts a new snapshot without skipping events. Acknowledgements represent application progress, not network receipt alone.

The real client owns heartbeat, dead-peer detection, bounded exponential backoff with jitter, and recovery. Reset retry delay after successful authenticated recovery. Pin mismatch, invalid credentials, and revocation require explicit action. Transient network failures reconnect. Lost mutation responses produce an unknown outcome unless an existing stable operation identity permits safe reconciliation. Do not automatically repeat arbitrary mutations.

Small control messages and cancellation remain responsive during bulk transfers. A stalled viewer is disconnected independently. Notification messages carry minimal committed-event hints. They wake the consumer to fetch authoritative state and cannot replace durable tasks or approvals.

## Pins, rotation, and recovery

Validate the certificate chain, server name, validity, handshake signature, and exact leaf pin before sending application credentials. Keep pin enforcement on every reconnect and route change.

Initial rotation is an explicit trusted local procedure. Export the replacement trust bundle through local owner administration, verify the manager identity, and import the replacement pin through protected client input. Coordinate certificate activation and reconnect. A client that misses rotation remains disconnected until trusted recovery. Never discard or replace a pin because a connection failed. Automatic online rotation is outside this implementation.

Credential loss or exposure requires a new client credential, a new invitation, and revocation of the old device. Existing worker certificate recovery remains explicit. Native platform storage follows in C02.

## Routes and acceptance

The owner has no known existing mesh or public relay. Keep the endpoint configurable. For a lasting installation, document a self-hosted opaque outbound TCP relay, such as [rathole](https://github.com/rathole-org/rathole#configuration). Its manager-side agent connects outward and forwards bytes to the single manager TLS listener. The relay does not receive the manager key or terminate its TLS connection.

For external acceptance, use a temporary [bore](https://github.com/ekzhang/bore) v0.6.0 tunnel from a unique synthetic manager fixture to bore.pub. Its control port is reachable from both the operator and manager VM. Verify the actual external path before marking acceptance complete. Keep the inner manager TLS pin, use only synthetic owner state, and stop the tunnel and fixture afterward. This establishes an observed external relay route without changing the router or selecting permanent hosting.

SSH remains fixture bootstrap and recovery transport. It does not carry owner protocol acceptance. Record the public endpoint, direct LAN path, TLS identity, tunnel version and hashes, process shutdown, and cleanup. Failed external connectivity leaves that gate open.

## Implementation units

1. Add owner wire/domain types, migration, invitation and device state, and central shared authority. Verify local-session behavior and atomic enrollment and revocation.
2. Extract the single public listener. Verify existing worker binaries and coding endpoints, certificate rejection, cross-role refusal, and public setup exclusion.
3. Add the bounded owner protocol, complete recovery, reusable client, and probe. Verify oversized snapshots and replies, interrupted recovery, retention rollover, live notifications, slow viewers, and actual reconnect.
4. Run actual manager, worker, and owner clients on the two acceptance VMs, then through the external relay. Verify revocation of current and future access and every setup boundary. Run the repository checks and retain evidence.

## Synthesis decision

The Astra candidate supplies the base. Pinned TLS with separate owner credentials preserves worker compatibility and avoids another certificate authority. Sol contributes explicit unknown mutation outcomes, application acknowledgements, and control-message responsiveness during snapshots. Luna reinforces separate role admission and connection budgets. Certificate issuance and custom Ed25519 handshakes add lifecycle work that M05 does not require.

The pin lifecycle uses trusted local rotation rather than the candidates' staged online rotation protocol. The user reported no existing remote route, so synthetic acceptance uses the temporary public relay described above. These are coordinator decisions. [Independent cross-judgment](m05-design-verdict.md) approved this synthesis and requires a demonstrated snapshot allocation bound before recovery can pass.

## Implementation reconciliation

The implementation keeps local public Manager signatures and admits the private authority enum inside shared State methods. Device leases register by independent connection identity, so several connections can share a device without sharing disconnect cleanup.

Owner wire version 1 is separate from worker wire version 3. Every reply uses the same counted and hashed chunk transfer, including ordinary domain replies. A global permit covers snapshot materialization and serialization. Each viewer releases that permit before sending. The [snapshot bound](m05-snapshot-bound.md) accounts for every collection and the additional recovery allocations.

The reusable native client lives in `manager::owner_client` and has an `owner-probe` binary. Its private filesystem storage is a protocol acceptance implementation. Native keychain integration remains C02. Events prompt a complete authoritative M03 image read before the client commits its replacement image and cursor. This is a conservative implementation of the required authoritative-read rule. It avoids presenting event references as replacement entity state.

The old worker listener names remain aliases for the public listener. Public command-line aliases reject duplicate configuration. Worker paths and wire types remain unchanged. Legacy unbound worker invitations expire during migration. Existing worker registrations remain intact.

No external route acceptance is claimed until the temporary relay has carried actual pinned owner traffic and cleanup has been verified. Independent implementation review remains the parent's gate.
