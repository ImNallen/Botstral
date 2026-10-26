# Worker protocol reference

The source contract is [`link/src/lib.rs`](../link/src/lib.rs). [`link/schema/protocol.json`](../link/schema/protocol.json) is generated JSON Schema. Regenerate it with `cargo run -p botstral-link --bin export-schema --locked > link/schema/protocol.json`. Workspace tests reject drift. Rust consumers use `botstral-link`. TypeScript bindings remain C01 scope.

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
| `POST /v1/worker-commands` | Accepts a typed `Command` with `id`, `task_id`, `fence`, and `action`. Returns a receipt. |
| `POST /v1/worker-input` | Accepts `id`, `fence`, and numeric test `value`. The fence must name `owner`. Returns a receipt. |
| `GET /v1/workers/{worker}/receipts/{command}` | Returns connection-local acknowledgment, latest task event, and count of admitted bot input intents. |

A fence contains `generation`, `session_id`, `revision`, and `owner`. A receipt's `accepted` acknowledgment means admission, not an applied input or completed task. `bot_inputs` counts manager-admitted intentions. Task progress reports the test worker's applied effects. Only `applied` confirms owner input. Only a `cancelled` event confirms cancellation. Production rejects input and work execution with `unsupported_execution`, including approved-operation requests.

The M01 `/v1/status` worker marker remains unchanged for compatibility. Read `/v1/workers` for M02 worker status.

## Negotiation and authority

Every envelope contains `version`, UUID `request_id`, and a typed `message`. Version 1 uses the `stable` channel. The first worker frame must be `hello`. Both sides require `logical_sessions` and `input_fencing`. Required unsupported capabilities, incompatible versions, and incompatible channels fail before a generation is allocated. `test_input` is advertised only by the test worker. Production manager builds refuse test-work admission.

On connection, the manager increments the persisted worker generation and all its session revisions. `welcome` contains the authoritative snapshot. The worker replaces its registry and sends `snapshot_applied`. The manager confirms exactly that snapshot. Sessions added or changed meanwhile require their own `control_applied`. An old-generation event or disconnect cannot affect the replacement connection.

An owner change compares the expected revision, increments only the selected session's revision, and blocks its input. The worker stops that session's old producer, discards queued stale input, and sends `control_applied`. Only that acknowledgment confirms ownership. Repeating the same desired owner at the current revision is idempotent. An outdated expected revision conflicts.

The manager rechecks queued input before sending. The daemon checks immediately before applying input. Test bot input uses a correlated manager receipt followed by a final daemon fence check. A delayed approval of old input cannot resume a suspended producer. Returning control to a bot permits new work but does not replay the old producer.

## Commands, events, and bounds

Commands define start, status, cancel, approved operation, and result. Approved operations name an approval, operation, argument digest, and expiry. M02 gives those fields no execution authority. Events define acceptance, progress, redacted log codes, approval requests, screenshot and artifact references, completion, failure, status, and confirmed cancellation. Logs contain fixed codes without free-text payloads. Artifact references contain an ID, digest, length, and media type. M02 fetches no artifact URLs or contents.

Both peers reject invalid versions, unknown fields and variants, binary application frames, and messages above 64 KiB. TLS and HTTP setup, negotiation, and writes have five-second deadlines. The manager admits 16 concurrent worker connections, including upgrades, and 32 queued messages per worker. Idle connections close after 30 seconds. The daemon sends heartbeats every 10 seconds. Shutdown cancels upgraded sockets and drains tasks within five seconds before the manager joins its state owner.

There are at most 64 logical sessions and 128 command correlations per worker connection. The test worker keeps at most 16 tasks. Exhaustion returns `queue_full` or closes a connection whose delivery queue is full. Repeated command IDs with identical content reuse their receipt. Changed content conflicts. Correlations and test outcomes disappear on reconnect. No work is replayed automatically. Durable execution, replay, approval consumption, and crash recovery remain M03 work.
