# M02 worker and session architecture

M02 extends the existing manager state thread. It owns worker enrollment, connection generations, and per-bot logical sessions alongside M01 owner authentication and vault state.

## Selected design

The parent compared three designs and selected mutual TLS with one state owner. This adopts the Astra candidate and the judge's recommendation. It also adopts concrete transport bounds and a populated version-one migration test from Sol. A separate policy broker would split authority. A custom signature exchange would duplicate identity proof supplied by TLS. Best-effort takeover cannot establish that the worker stopped input.

The worker identity is SHA-256 of the verified client certificate DER. This narrows the candidate's public-key fingerprint proposal and avoids custom certificate key parsing. Certificate renewal requires owner revocation and enrollment of the replacement certificate.

## Interfaces and ownership

`link/` owns versioned Rust wire types and the generated JSON Schema. Request, command, task, bot, worker, and computer-session identifiers are distinct UUID types. Generations fence connections. Revisions fence input authority within one session. C01 now provides [generated TypeScript bindings and drift checks](c01-architecture.md#shared-protocol-bindings).

`manager/src/manager/state/` extends the existing SQLite owner. Migration 0002 adds workers, hashed invitations, and computer sessions without rebuilding M01 tables. Owner methods reauthorize within the queued operation. Socket tasks own transport only.

The selected transport uses a distinct mutual TLS listener. The daemon initiates WSS, validates the manager CA and hostname, and presents its certificate. Negotiation precedes a new generation and an authoritative acknowledged snapshot. An ownership change remains pending until the worker applies the revision and confirms it.

## Verification units

1. Shared schema and additive state migration. The populated-v1 test proves owner sign-in, bot retrieval, locked startup, vault unlock, exact ciphertext preservation, and credential decryption.
2. Enrollment, TLS, WSS, negotiation, connection fencing, and bounded shutdown.
3. Confirmed ownership, a feature-gated test worker, actual-binary acceptance, and release separation.

The implementation owner works alone in the T3-created worktree. The parent owns independent correctness and comment review. The decision trail is [m02-decisions.tsv](m02-decisions.tsv).

## Transport verification

`agent/tests/m02_transport.rs` starts the actual manager and daemon with a temporary CA. It checks invitation mismatch, expiry, replay, wrong server trust and hostname, untrusted and expired clients, unenrolled identities, owner-route separation, incompatible negotiation, forbidden methods, reconnect, and shutdown. Manager unit tests prove atomic invitation consumption, generation fencing, late disconnect handling, and queue saturation.

The worker listener holds each connection permit through TLS, HTTP upgrade, and WebSocket processing. Ordinary responses close the HTTP connection. Five-second deadlines bound TLS, HTTP, negotiation, writes, and shutdown. There are 16 connection permits and 32 outgoing messages per connection. Incoming messages are limited to 64 KiB. Both peers time out an idle connection after 30 seconds.

`link::tls` shares certificate loading and private-file validation between the two binaries. This small transport helper avoids duplicating secret-file checks. It uses the normal rustls certificate verifiers and the ring provider.

## Acceptance gaps

Actual two-VM deployment requires infrastructure and remains unverified. Local WSS cannot close that acceptance condition. M03 owns durable task journals and recovery. W02 owns real independent desktops. Production work execution remains unavailable in M02.

## Ownership and failure behavior

A connection has a synchronizing or ready phase. The manager retains the precise snapshot awaiting acknowledgment, so a concurrent session mutation cannot become confirmed accidentally. A superseded control acknowledgment is ignored. It cannot confirm a later revision or disconnect unrelated sessions. Old-generation disconnects do not remove the replacement connection.

The existing state queue serializes owner authorization and SQLite mutations. Dispatch stores bounded connection-local correlations. Socket tasks reauthorize queued delivery through that same owner. Delivery admission retains the outbound message while waiting for queue capacity. Disconnect cleanup also waits for admission. Saturation cannot silently discard a control message or leave a dead worker marked ready.

Takeover persists the desired owner and revision before sending control. Pending input is blocked. The worker applies control before confirming it. Its test executor stops the selected producer and discards queued old input. Each bot input also requires a manager receipt and a final daemon fence check. The other session continues under its own revision.

A failed control delivery can leave the committed desired owner disconnected. The caller must read current state and await reconnect synchronization. This never reports confirmed takeover. A lost task outcome remains unknown. No work or input is automatically replayed. The test worker suspends old producers on takeover and requires new work after control returns.

## Reconciliation and verification

The M02 design deferred TypeScript generation to C01. Those [bindings are now available](c01-architecture.md#shared-protocol-bindings). The compact work specification defines unavailable production work and explicit test input. It does not introduce unfinished browser or shell operation trees. Redacted logs use fixed codes. Artifact references carry metadata only.

The default production daemon and the separately named `botstral-test-agent` share transport and session admission. The test executor is behind `nonproduction-test-worker`, absent from defaults, and rejected when release compilation disables debug assertions. The manager requires its corresponding debug feature before admitting test work. The M01 status marker remains for compatibility; M02 worker status comes from `/v1/workers`.

Certificate renewal exposed a necessary owner action. Rebinding an existing bot session to a replacement worker requires revoking the old worker first. It preserves the session ID, advances its revision, and resynchronizes the replacement worker.

`make verify` checks formatting, warnings, schema drift, the populated-v1 migration, all M01 acceptance, production daemon refusal, invitation races and lost-response recovery, certificate failures, protocol and credential separation, connection permits, session fencing, cancellation, and bounded shutdown. It also builds into a fresh production directory, checks that the test binary is absent, and verifies that a release build with the test feature fails. A TLS probe drives the actual test daemon to verify stale-frame rejection and a delayed input receipt after confirmed takeover.

Independent review exposed delivery loss when the state queue was full. The focused queue test reproduced `delivery bypassed queue admission with Err(Busy)` before the fix. It now proves that delivery waits for capacity and reaches the state owner. The takeover test observes increasing applied effects in the other session while the selected session remains suspended. Transport acceptance is separated into enrollment, session, and fencing modules under `agent/tests/m02_transport/`.

The implementation uses Model the Domain through typed session ownership, connection phases, and separate revision and generation fences. Prove It Works determines the use of actual binaries and reproducible acceptance tests.

## Verified commands

The final implementation passed these checks on the local macOS worktree. `make verify` runs the complete reproducible sequence in [scripts/verify.sh](../scripts/verify.sh).

| Command | Outcome |
| --- | --- |
| `cargo fmt --all --check` | Passed. |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed. The same check with both nonproduction features also passed. |
| `cargo test --workspace --locked -- --nocapture` | Fifteen unit tests and both actual-binary acceptance tests passed. |
| `cargo test -p botstral-agent --features nonproduction-test-worker --locked --lib -- --nocapture` | Both daemon unit tests passed. |
| `cargo test -p botstral-agent --features nonproduction-test-worker --locked --test m02_transport -- --nocapture --skip enrollment_tls_negotiation_and_lifecycle_use_actual_binaries` | Both test-worker acceptance tests passed. |
| `cargo test --workspace --features botstral-agent/nonproduction-test-worker,botstral-manager/nonproduction-test-worker --locked --test m02_transport -- --nocapture` | All three transport acceptance tests passed. |
| `make verify` | Passed, including a fresh default release build, absence of the test binary, and separate expected release-feature compilation failures for manager and agent. |
