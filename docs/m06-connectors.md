# M06 connectors

Connections belong to the owner. Installing or authenticating one grants no bot tools. Each bot receives explicit tools from a discovered catalog. A binding captures the connection revision, credential epoch, catalog revision, per-bot/per-connection grant revision, tool name, and schema digest. Run proposals, ordinary task approvals, and Vibe policies retain that binding. Revoking and regranting the same tool does not revive old approvals.

## Execution and recovery

A connector run proposal becomes an ordinary approved `WorkSpec::Connector` task. The worker lowers it into the existing Process supervisor. Its pinned helper calls the manager through the enrolled outbound mutual TLS connection. Remote credentials remain on the manager. Local launch material is released to the assigned worker once. Vibe uses the same helper inside its existing Process scope; it does not submit a second task or reserve a second process slot.

The manager's State queue is the sole authority and effect writer. HTTP and stdio I/O run outside State. Before releasing a permit, State rechecks the exact execution, current worker generation, input fence, consumed approval, permission revision, task and parent deadline, and captured connection/catalog/grant revisions. Remote calls also recheck State immediately before `tools/call`; local helpers request fresh status at that boundary.

`ConnectorCall` records `reserved`, then commits `dispatch_possible` before remote transmission or local permit release. Repeated requests return status, never launch material. The helper persists identity before requesting a permit and persists a redacted outcome before settlement. Worker recovery only resubmits saved evidence from stopped scopes. It cannot launch from that evidence.

An unresolved possible effect becomes `unknown` after authority loss, timeout, disconnect, or restart. A parent run enters `awaiting_attention`; Vibe terminates and refuses further coding or connector calls. Late evidence is retained in `late_result` without resuming a run or changing the unknown classification. An owner must inspect the evidence before starting new work. A committed finished result remains finished when its settlement response is lost.

The declared `connector.json` artifact contains only the invocation identifier. Run result collection reads the authoritative, redacted ledger result and checks any available artifact against that identifier. A forged receipt cannot substitute another invocation. Ordinary run results cap connector payloads at 4096 bytes to leave room within the existing 8192-byte transcript result limit. Direct tasks and Vibe can request up to 8192 bytes.

## Protocol and bounds

The worker protocol is version 4 and the owner protocol is version 2. The database migration is 9. JSON Schema and TypeScript are generated from `link`. Older peers are rejected at negotiation; existing stored policies default their absent connector field to `None`. Owner snapshots include bounded connection summaries. Full catalogs use the explicit connection operations.

Both transports negotiate exactly [MCP 2025-06-18](https://modelcontextprotocol.io/specification/2025-06-18/basic/lifecycle), advertise no sampling, roots, or elicitation, and support tools only. The remote client accepts JSON and incremental SSE POST responses, correlates request identifiers, honors session identifiers, sends the protocol header after initialization, and rejects server capability requests. It uses HTTPS, no redirects, no ambient proxy, and no automatic retries. Sessions are ephemeral; there is no SSE replay or automatic reconnection of a possible call.

Local stdio uses bounded newline JSON framing, nonblocking pipes, a clean environment, closed inherited descriptors, discarded server stderr, a deadline, and process-group cleanup within the existing outer Process scope. It initializes and rediscovers before each invocation and checks the exact captured schema. Only explicit public environment and protected named secrets enter the subprocess environment.

There are at most 32 connections, 32 tools per catalog, 32 selected tools per policy, and 16 calls per Vibe task or connector run policy. Catalogs are bounded to 32768 serialized bytes, eight pages, and schemas of 8192 bytes. Arguments are bounded to 8192 bytes. The existing 2048-byte serialized task-spec limit also applies to Connector and Vibe tasks, so their combined bindings, prompt, arguments, and policy must fit that smaller envelope. JSON nesting is bounded to 24 levels with at most 256 elements per container. Remote bodies and stdio frames are bounded to 128 KiB; stdio has a 256 KiB aggregate read bound. An invocation lasts at most 60 seconds and never exceeds its task deadline. The manager admits at most four concurrent gateway requests.

Schema validation is offline and bounds regular-expression backtracking. Remote schema references are rejected. Tool results and catalog descriptions are untrusted. Stored results redact secret strings in both values and keys. Audit records contain fixed event codes, identifiers, and digests, without credentials or raw tool arguments. Encrypted credentials are bound to the connection identity, transport class, and credential epoch. Historical ciphertext supports redaction of late evidence; public operations never return it. Protected secret entries have a 16 KiB aggregate limit.

Cross-language digests use SHA-256 over a typed encoding of JSON, not serializer-dependent text. Null, booleans, strings, integers, floats, arrays, and objects have distinct tags. Strings and collections include unsigned big-endian 64-bit lengths; floats use big-endian IEEE-754 bytes; object keys are sorted. The exact Rust/Python vectors are in `link/schema/connector-digests.json`.

## Worker configuration

The connector helper is embedded in the agent binary. `connector.json` pins its bytes together with the SHA-256 of the Python executable. Changing either requires a new configuration and fresh task policy. Generate the configuration on the worker for its existing non-root Process account:

```sh
python3 agent/connector/configure.py --python /usr/bin/python3 > /path/to/worker-state/connector.json
chmod 600 /path/to/worker-state/connector.json
```

Run this as the worker account, or pass its numeric `--uid` and `--gid`. The file's `account` must match the existing Process account and its ceilings must fit Process supervision. Copy `runtime`, `account`, and the desired bounded `limits` into `ConnectorRunner`. The command only generates configuration; it does not install or change a service. Restart or reconnect the worker through the normal deployment procedure to advertise `connector_v1`.

Vibe continues to use its separately pinned 2.24.2 distribution and Python runtime. Its connector policy also pins the helper configuration and matches the Vibe account. The wrapper enables only exact granted dynamic names. Existing file permission callbacks remain active, `bypass_tool_permissions` stays false, and denied paths remain denied. Exact connector names use Vibe's `always` setting only after the owner approved their task policy; every actual effect still passes current manager authorization.

## Owner API

All local routes below require the existing owner session bearer token. Their JSON types are defined in `link/src/connector.rs`. Secret entry requires an unlocked vault. The owner WSS `OwnerOperation` variants provide the same operations over the existing encrypted device channel.

| Local route | Owner operation | Input or result |
| --- | --- | --- |
| `POST /v1/connectors` | `install_connection` | `InstallConnection` becomes `ConnectionView`. |
| `GET /v1/connectors` | `connections` | All connection views. |
| `GET /v1/connectors/{id}` | `connection` | One connection view and catalog. |
| `PUT /v1/connectors/{id}/secret` | `replace_connection_secret` | Expected revision and protected secret. |
| `POST /v1/connectors/{id}/reconnect` | `reconnect_connection` | Expected revision. |
| `POST /v1/connectors/{id}/revoke` | `revoke_connection` | Expected revision. Revocation is terminal. |
| `POST /v1/connectors/discover` | `discover_connection` | `DiscoverConnection` becomes an ordinary task. |
| `GET /v1/bots/{bot}/connectors/{id}` | `connector_grants` | Current bot/connection grant revision. |
| `PUT /v1/bots/{bot}/connectors` | `set_connector_grants` | Exact connection/catalog and expected grant revision. |
| `GET /v1/bots/{bot}/connector-tools` | `connector_tools` | Currently granted tool definitions. |
| `GET /v1/tasks/{task}/connector-calls` | `connector_calls` | Durable effect and late-result evidence. |
| `GET /v1/connectors/{id}/audit/{after}` | `connector_audit` | Up to 128 audit entries after a sequence. |

Remote installation uses this transport shape:

```json
{"type":"remote_http","endpoint":"https://connector.example/mcp"}
```

Local installation uses this shape, with actual worker, account, and canonical absolute paths:

```json
{"type":"worker_stdio","worker":"WORKER_UUID","account":{"uid":1000,"gid":1000},"executable":"/usr/bin/node","arguments":["/srv/mcp/server.js"],"cwd":"/srv/mcp","environment":{}}
```

An installation body contains `id`, `label`, and `transport`. Install starts revision 1 and credential epoch 1 with no credential. Authentication is a separate protected request:

```json
{"expected_revision":1,"secret":{"type":"bearer","value":"SECRET_FROM_PROTECTED_INPUT"}}
```

For local secrets use `{"type":"local_environment","values":{"SERVICE_TOKEN":"SECRET_FROM_PROTECTED_INPUT"}}`; to remove credentials use `{"type":"none"}`. Supply real values through protected input, never command arguments or metadata. OAuth browser flows are outside this implementation. Rotation and reconnect advance connection and credential revisions and require fresh discovery and grants. A revoked connection must be installed under a new identity.

Discovery takes `task` and `session` UUIDs, the current `expected` connection version, `runner`, and a Unix `deadline`. Review and approve the returned task through the existing exact task approval API. On success, fetch the connection again because discovery advances its revision and catalog revision. Set grants with its exact `connection` version, `catalog_revision`, the prior grant's `expected_revision` (initially zero), and tool `tools` names. An empty list revokes that bot's grant. Obtain bindings from `connector-tools`; do not synthesize them.

A run's optional `execution_policy.connectors` contains `tools` bindings, `max_calls`, `max_result_bytes`, and `runner`. A Vibe policy uses the same shape in `policy.connectors`. The runner field is a small addition to the selected design: ordinary connector tasks need an explicit approved helper identity, account, and resource ceilings when lowered from model proposals.

Remote discovery deliberately retains the selected ordinary-task implementation and therefore also needs an available worker and Computer session. It does not start the remote server on that worker. This is the documented discovery coupling allowed by the design verdict.

## Verification and remaining acceptance

Connector tests use recorded provider responses and temporary owner/worker state. They do not use paid inference or change installed services. `manager/src/manager/state/connectors/tests` exercises current authority, independent bot grants, stale discovery and calls, exact approval, atomic release, SQLite reopening, lost replies, uncertain effects, late evidence, parent-run stopping, dynamic model tools, and result collection. Remote transport tests cover JSON, SSE, server requests, protocol mismatch, schema changes, authentication failure, and an authority change after discovery.

`agent/tests/m06_connector_contract.py` drives real stdio subprocesses and checks clean environment, redaction, bounds, deadlines, revocation, one-shot launch recovery, saved settlement replay, and shared digests. Rust worker tests check runtime pin changes and recovery of captured outcomes without another launch.

`agent/tests/m06_vibe_contract.py` runs the actual pinned Vibe binary with dynamic MCP tools and denial callbacks. `agent/tests/m06_vibe_local.py` runs its production relay with a real local stdio child, checks the common parent process, and verifies that an unknown effect prevents another inference. On macOS this does not prove Linux systemd cgroup membership; that acceptance remains open.

`manager/tests/m06_owner_connector.py` and `m06_owner_wss.py` run the actual manager binary and exercise local and owner WSS administration, encrypted storage, restart, and redacted audit. `agent/tests/m06_official_stdio.py` uses the real official `@modelcontextprotocol/server-filesystem@2.0.1` server without external secrets, discovers its tools, and observes a temporary file effect. Install it in a temporary directory and pass its `dist/index.js` and an absolute Node executable.

The result report records the original implementation commands and captured evidence. [Integrated Linux acceptance](m06-integrated-acceptance.md) subsequently verified authenticated remote use after backup restoration, exact approvals, distinct bot grants, stale-call rejection, actual Vibe scope membership, and production cancellation across the Proxmox pair. [Recovery acceptance](m06-recovery-acceptance.md) covers native packaging and Docker. Final whole-branch review and milestone closure remain separate gates.
