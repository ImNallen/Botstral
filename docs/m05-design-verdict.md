# M05 independent cross-judge

Use Astra as the base, with Sol's compatibility inventory, authoritative-read rule, and concrete route selection. The preferred pinned-TLS plus bound 256-bit device-secret design is justified by the actual alternatives. M05 does not earn a second certificate authority or an application signature protocol. A stolen bearer credential remains sufficient to clone a device; digest-bound enrollment prevents invitation theft from enrolling a different credential, not credential theft. State that tradeoff plainly.

Scores are design quality, not implementation proof.

| Candidate | Authority and roles | Invitation, pin, revocation | Compatibility | Bounded durable recovery | Depth and verification |
| --- | ---: | ---: | ---: | ---: | ---: |
| Astra | 5 | 4 | 5 | 4 | 5 |
| Sol | 5 | 4 | 5 | 4 | 5 |
| Luna | 3 | 2 | 3 | 2 | 3 |

Astra wins narrowly. Its explicit remote-operation allowlist, refusal of certificate-bearing owner requests, protected snapshot spool, oversized-domain-response handling, and separately owned client driver make the important boundaries concrete. Its snapshot permits preserve existing filesystem-document reconciliation rather than inventing an independent database view. Sol is equally plausible as a base, but leaves oversized ordinary replies uncovered and introduces a client-readiness rotation protocol without showing why it is needed.

Luna's certificate alternative is structurally different but underdeveloped. It adds both an owner CA and a persistent Ed25519 identity/signature mechanism without specifying issuance crash recovery, renewal, signing transcripts, or how later TLS handshakes bind the signed identity to the current peer. Its paged snapshot reads do not establish an immutable image across queue jobs and filesystem documents. Its statement that every worker path requires an already enrolled fingerprint would break worker enrollment. Its proposed shared ordinary router also creates unnecessary exposure risk. The assertion that bearer callers must understand refresh is unsupported; the other candidates hide credentials and reconnect behind the SDK.

## Required synthesis corrections

1. Keep pin rotation local and explicit. Export/import a trusted replacement profile, stage a bounded old/next pin set when overlap is useful, and retain normal chain, name, time, and handshake-signature checks. No readiness registry or online rotation protocol is necessary. Never accept a replacement pin because a connection failed. Keep compromise recovery local. A UUID labels the Manager; the authenticated pin and trusted transfer establish cryptographic identity.
2. Prove the snapshot allocation bound before choosing full materialization. Both strong candidates assert existing cardinality and byte limits without supplying the calculation. Two permits bound concurrency, not image size. Include every collection, filesystem-backed documents, strings, encoding expansion, serializer buffers, and temporary storage. If any source is unbounded, resolve a consistent bounded snapshot construction before implementation can claim this contract. A generic retryable size error is not recovery for permanently oversized admitted state.
3. Graft Sol's explicit rule that events identify changed entities and require authoritative reads. Astra's `apply_events` interface must not imply events contain complete replacement state. Commit projection and cursor durably together; define a fetched view as current authoritative state rather than falsely claiming it is the historical state at the event cursor. Preserve the snapshot/head atomicity guarantee.
4. Retain Astra's transfer envelope for oversized ordinary replies as well as snapshots. Specify whole-transfer deadline, total byte limit, one bulk transfer per connection, heartbeat/cancellation priority, and cancellation cleanup of spool/stage files. Test revocation while a send is blocked. Commit revocation before signaling and reject subsequent queued operations; do not promise to retract bytes already sent.
5. Graft Sol's source/config compatibility list: `WorkerTls`, existing entry points, worker metadata, flags, routes, wire version, coding authorization, and optional worker pins. Keep old workers functional. Explicitly expire legacy outstanding invitations lacking new bindings; preserve established worker records. Require a verified worker certificate for enrollment, but require enrolled registry identity only where the current protocol requires it.
6. Use long random invitation bundles only in M05. Persist the independent device credential before redemption, bind invitation to its digest, and atomically consume/create. Test lost successful response, unique digest races, revoked-credential reenrollment rejection, and wrong-manager/role attempts. The trusted local confirmation must actually receive the intended device digest.

## Remote route verdict

The old claim that acceptance must wait for user-supplied mesh credentials is obsolete. Select configurable/self-hosted rathole for production deployment and allow a temporary bore v0.6.0 raw TCP tunnel for synthetic fixture acceptance. This is safe under the supplied constraints: inner TLS terminates at the fixture Manager, exact pin verification precedes either secret, no real owner state or existing service is exposed, and tunnel processes, temporary credentials, configuration, and fixture roots are cleaned up.

Operator reachability to bore.pub:7835 proves only a prerequisite. Verify fixture-host outbound connectivity, an actual external client data path, unchanged Manager certificate through the relay, forced reconnect, replay/snapshot recovery, and current/future revocation denial. Record the temporary endpoint and topology. A public relay can observe traffic metadata and deny service; it must never receive TLS private keys or terminate the inner TLS connection. Do not represent synthetic public-relay acceptance as a production deployment or a durable service guarantee. No custom relay protocol is needed.

The main implementation blockers are the demonstrated snapshot bound and precise recovery contract, not missing mesh enrollment. External acceptance remains unproven until the real tunnel and client run succeed. All candidates remain sketches, and none earns a completed M05 checkbox yet.
