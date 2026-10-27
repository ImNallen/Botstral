# Worker protocol reference

The source contract is [`link/src/lib.rs`](../link/src/lib.rs). [`link/schema/protocol.json`](../link/schema/protocol.json) is generated JSON Schema. Regenerate it with `cargo run -p botstral-link --bin export-schema --locked > link/schema/protocol.json`. Workspace tests reject drift. Rust consumers use `botstral-link`. TypeScript consumers use the generated declarations in [`@botstral/link`](../link/package.json). Run `npm run bindings:generate` to regenerate them and `npm run bindings:check` to reject Rust schema or TypeScript drift. C01 does not execute the worker protocol.

## Identity and enrollment

The worker-only TLS listener requires a trusted client certificate. The worker identity is lowercase hexadecimal SHA-256 of the verified leaf certificate DER. Invitations are random 256-bit secrets, bound to that fingerprint, valid for 1 to 600 seconds, and consumed once. SQLite stores only their SHA-256 hashes. Consumption and worker insertion share one transaction. A mismatch does not consume the invitation. Revocation closes an existing worker connection and rejects reconnection.

`POST /worker/v1/enroll` accepts `{"invitation":"<secret>"}` and returns `201` with `worker_id`. `GET /worker/v1/connect` upgrades to WSS. These are the only worker routes. No worker bearer token exists. Owner bearer tokens do not authenticate a worker. Worker certificates and invitations do not authenticate an owner. Worker messages have no approval-grant, device-enrollment, credential-read, or security-policy method.

## Owner methods

These methods use the existing ordinary loopback listener and operation-time owner authorization. JSON mutations reject unknown fields. Worker errors use explicit codes with HTTP `409`. M01 authentication and admission errors retain their existing statuses.

| Method | Request or response |
| --- | --- |
| `POST /v1/worker-invitations` | Accepts `fingerprint` and `lifetime_seconds`. Returns `invitation` once and `expires_at`. |
| `GET /v1/workers` | Returns `id`, `fingerprint`, `revoked`, durable `generation`, and live `phase`. |
| `DELETE /v1/workers/{id}` | Accepts `{}`. Revokes the worker and stops its active socket. |
| `POST /v1/computer-sessions` | Accepts `bot_id` and `worker_id`. Returns the existing or new session. A different worker requires revocation of the previous worker. |
| `GET /v1/computer-sessions/{id}` | Returns `id`, `bot_id`, `worker_id`, `owner`, `revision`, and `status`. |
| `PUT /v1/computer-sessions/{id}/owner` | Accepts `expected_revision` and `owner`, either `bot` or `owner`. Returns the current session. |
| `POST /v1/worker-commands` | Rejects legacy task commands. Use the durable task API. |
| `POST /v1/worker-input` | Accepts `id`, `fence`, and numeric test `value`. The fence must name `owner`. Returns a receipt. |
| `GET /v1/workers/{worker}/receipts/{command}` | Returns connection-local owner input acknowledgment. Durable tasks use `/v1/tasks/{id}`. |

A fence contains `generation`, `session_id`, `revision`, and `owner`. Owner input receipts distinguish accepted intent from applied input. Durable task reports are separately journaled and acknowledged after manager commit. Production refuses legacy approved-operation requests. Opt-in [W01 process work](w01-process.md) uses the durable task path.

The M01 `/v1/status` worker marker remains unchanged for compatibility. Read `/v1/workers` for M02 worker status.

## Negotiation and authority

Every envelope contains `version`, UUID `request_id`, and a typed `message`. Version 3 uses the `stable` channel. The first worker frame must be `hello`. Both sides require `logical_sessions` and `input_fencing`. Required unsupported capabilities, incompatible versions, and incompatible channels fail before a generation is allocated. `test_input` is advertised only by the test worker. Production manager builds refuse test-work admission. A supported, configured worker advertises `process_systemd_v1` after probing its backend.

On connection, the manager increments the persisted worker generation and all its session revisions. `welcome` contains the authoritative snapshot. The worker replaces its registry and sends `snapshot_applied`. Sessions with admitted unfinished process work remain pending until an explicit `control_applied` follows recovery and verified stopping. Other snapshot sessions retain their existing confirmation behavior. Sessions added or changed meanwhile require their own `control_applied`. An old-generation event or disconnect cannot affect the replacement connection.

An owner change compares the expected revision, increments only the selected session's revision, and blocks its input. The worker stops that session's old producer, discards queued stale input, and sends `control_applied`. Only that acknowledgment confirms ownership. Repeating the same desired owner at the current revision is idempotent. An outdated expected revision conflicts.

The manager rechecks queued owner input before sending. The daemon checks immediately before applying it. Durable effects require a worker claim and a current connection permit. A delayed permit under an old fence cannot invoke the adapter. Returning control to a bot never reuses a consumed execution permit.

## Durable commands and recovery

`CommandBody` is immutable semantic intent. `Delivery` adds current fences, an optional approval binding and durable cancellation intent for that execution. The old `Command` task shapes remain decodable for explicit refusal and have no execution path.

After `snapshot_applied`, the daemon sends `recovery_hello` with its persistent journal incarnation and fixed report high-water mark. The manager binds incarnation before dispatch. A replacement journal places potentially delivered unresolved work into attention even if acceptance never reached the manager.

The manager requests ordered `recover` pages. The daemon sends at most 16 reports and 48 kibibytes per page. Report identity is worker, journal incarnation and sequence. The manager commits receipts, reducer changes and events before `report_ack`. Duplicate identities must have identical bodies. Acknowledgments delete transport payloads, while semantic command records and terminal outcome report identities remain.

A complete paged journal inventory follows report replay. The manager checks command bodies and consumed executions. `recovery_ready` admits fresh delivery only after all pages reconcile. Missing consumed execution evidence requires attention. Report and inventory queues have count and byte bounds. The journal accepts at most 128 semantic commands and 16384 lifetime report records. Capacity exhaustion fails closed without evicting duplicate protection.

The worker journals prepared acceptance before reporting. It requests `claim` immediately before execution. The manager verifies the complete owner approval binding, permission revision, current session control and cancellation, then atomically consumes authority. `permit` is specific to the connection and execution. It is never an outbox command. The worker journals admission and rechecks local expiry and fencing. Synthetic work invokes its synchronous adapter. Process work records a one-shot launch intent and rechecks authority before releasing its supervised helper.

The independent debug provider ledger records actual synthetic invocations before the journal result. On restart, a reliable observation recovers the outcome. An unavailable observation or a lost permit requires attention and never invokes replacement work. Cancellation remains sticky until a journaled stopped confirmation. `no_process_group` is scoped evidence for this adapter, which has no external process group.

## Transport bounds

Both peers reject invalid versions, unknown fields and variants, binary application frames, and messages above 64 kibibytes. TLS and HTTP setup, negotiation, and writes have five-second deadlines. The manager admits 16 concurrent worker connections, including upgrades, and 32 queued messages per worker. Idle connections close after 30 seconds. The daemon sends heartbeats every 10 seconds. Shutdown cancels upgraded sockets and drains tasks within five seconds before the manager joins its state owner.

There are at most 64 logical sessions and 128 owner-input correlations per worker connection. Full transport queues close the connection while durable commands remain stored for recovery.

The historical M02 evidence records protocol version one, and the historical M03 record uses version two. Current Rust fixtures and the W01 two-machine runner exercise version three. M02 remote Python runners remain historical version-one reproduction tools and must use the recorded binaries. M03 recovery is reproduced by the commands in [M03 acceptance](m03-acceptance.md).
