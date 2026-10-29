# Local API reference

M01 accepts HTTP/1.1 on literal loopback addresses. Defaults are `127.0.0.1:43120` for ordinary operations and `127.0.0.1:43121` for first-owner setup. `--listen` and `--setup-listen` accept literal IPv4 or IPv6 loopback socket addresses, including port zero for automatic allocation. Hostnames and non-loopback addresses are rejected.

`--state-dir` defaults to `.state`. The parent directory must already exist. Private `listeners.json` records bound addresses as `{"listen":"127.0.0.1:43120","setup":"127.0.0.1:43121"}`. `setup` becomes `null` after its listener closes. M02 adds `worker`, which is `null` unless the separate TLS listener is configured. The file is removed on graceful shutdown.

## Request rules

Every request requires exactly one `Host` header matching its literal listener address and port. Every `Origin` header and every `Sec-Fetch-*` header is rejected. Forwarding headers grant no authority. There are no CORS grants. Responses use `Cache-Control: no-store`.

Mutations require exactly `Content-Type: application/json` and a JSON object. Unknown fields, duplicate known fields, malformed JSON, and invalid types are rejected without echoing values. Empty mutations use `{}`. JSON bodies are limited to 16,384 bytes. Headers are limited to 8,192 combined name and value bytes, 64 fields, and a 16,384-byte parser buffer. Header and JSON body reads each have a five-second deadline. Connections close after each response. Each listener admits at most 64 concurrent connections.

Shutdown stops accepting connections and gives existing connections five seconds to finish. It then cancels remaining connections, including clients that stop reading responses. Admitted state operations finish before the installation lock is released.

Protected routes require exactly one `Authorization: Bearer TOKEN` header. Tokens are 43-character base64url strings issued by sign-in. Setup uses the same syntax with the capability read from `bootstrap.token`. Credentials never belong in URLs.

Owner and bot names contain 1 to 100 Unicode characters without control characters. Passwords and vault passphrases contain 16 to 1,024 UTF-8 bytes. Provider values contain 1 to 8,192 UTF-8 bytes and are opaque strings. Identifiers are UUIDs. Timestamps are integer Unix seconds.

## Setup listener

Only `POST /v1/owner` exists here. Its request has these fields:

```json
{
	"name": "Local owner",
	"password": "SYNTHETIC login password",
	"vault_passphrase": "SYNTHETIC separate vault passphrase",
	"acknowledge_no_escrow": true
}
```

The strings above are examples, never production credentials. `acknowledge_no_escrow` must be `true`. Success returns `201` with `{"name":"Local owner"}`. Owner and vault initialization commit together and leave the vault locked. The capability file is removed and setup closes. A configured installation never opens setup on restart. Unconfigured restarts rotate the capability.

`POST /v1/owner` returns `404` on the regular listener, including before configuration. Setup has no ordinary manager routes. Duplicate setup cannot change the owner. A concurrent request can receive `503` while a credential operation holds admission, `409` after commit, or connection refusal after setup closes.

## Regular listener

All routes below require a session except health and sign-in.

| Method and path | JSON request | Success |
| --- | --- | --- |
| `GET /health` | None. | `200`, `{"status":"ok"}`. This reports HTTP liveness only. |
| `POST /v1/sessions` | `{"password":"SYNTHETIC login password"}`. | `201`, a new token and session metadata. |
| `GET /v1/owner` | None. | `200`, `{"name":"Local owner"}`. |
| `GET /v1/sessions` | None. | `200`, an array of unexpired session metadata. |
| `DELETE /v1/session` | `{}`. | `204`, the caller is signed out. |
| `DELETE /v1/sessions/{id}` | `{}`. | `204`, the target is revoked. An absent target also succeeds. |
| `PUT /v1/bots/{id}` | `{"name":"Research bot"}`. | `200`, the full bot. An identical retry returns the same record. Different fields conflict. |
| `GET /v1/bots/{id}` | None. | `200`, the full bot. |
| `GET /v1/bots` | None. | `200`, all bots ordered by creation time and identifier. |
| `GET /v1/status` | None. | `200`, vault and provider metadata plus observed implementation status. |
| `POST /v1/vault/unlock` | `{"passphrase":"SYNTHETIC separate vault passphrase"}`. | `204`, the vault is unlocked. Already unlocked conflicts. |
| `POST /v1/vault/lock` | `{}`. | `204`, the vault is locked. Repetition succeeds. |
| `PUT /v1/provider-keys/mistral` | `{"secret":"SYNTHETIC provider key"}`. | `200`, provider metadata. Requires an unlocked vault. |

Sign-in returns the token once:

```json
{
	"token": "<issued 43-character bearer>",
	"session": {"id": "<UUID>", "created_at": 1792960000, "expires_at": 1793046400}
}
```

Session lists contain only `id`, `created_at`, and `expires_at`. The lifetime is 24 hours from issuance and does not slide. Restart invalidates every session. Sign-out and revocation apply immediately when their operation completes. They leave vault state available to other valid sessions.

Bot records contain exactly `id`, `name`, and `created_at`. Creation uses a caller-supplied UUID for safe retries. An authenticated unknown identifier returns `404`. Invalid sessions return `401` before any record lookup.

Provider metadata contains exactly `provider` and `updated_at`. There is no value, ciphertext, identity, suffix, read route, or export route. Status has this shape:

```json
{
	"vault": "locked",
	"provider": {"provider": "mistral", "updated_at": 1792960000},
	"worker": "not_implemented",
	"inference": "single_turn_text",
	"remote": "disabled"
}
```

`provider` is `null` before entry. `vault` is `locked` or `unlocked`. Metadata makes no claim that a provider connection works. The worker marker is retained for M01 compatibility. Use [the M02 worker methods](worker-protocol.md#owner-methods) for current worker state and session operations.

`inference` reports the implemented API, not configuration readiness or a working provider connection. Admission requires a current verified configuration and an unlocked vault.

## Errors and admission

Application errors have one fixed code, such as `{"error":"unauthorized"}`. They omit submitted values and diagnostics. The HTTP parser can close malformed connections before routing.

| Status | Codes |
| --- | --- |
| `400` | `invalid_input`, `invalid_json`. |
| `401` | `unauthorized`, `unlock_failed`. Missing, invalid, expired, revoked, and signed-out tokens use `unauthorized`. |
| `403` | `forbidden`. |
| `404` | `not_found`. Unsupported methods also use this result. |
| `408` | `request_timeout`. |
| `409` | `conflict`, `vault_locked`. |
| `413` | `body_too_large`. |
| `415` | `json_required`. |
| `429` | `rate_limited`. |
| `431` | `headers_too_large`. |
| `500` | `internal_error`. |
| `503` | `busy`. |

Sign-in, unlock, and bootstrap proofs each have a separate budget of five admitted attempts per 60-second window. Failed checks count. One expensive operation can be queued or running across setup, sign-in, and unlock. The queue holds 32 pending operations. Admission fails immediately when a bound is reached. A disconnected request can still commit. Retrying bot creation uses the same UUID and name.

## Protected input example

This example runs in an interactive Python session on the manager host. `getpass` reads credentials without echo. The token remains in memory. Submitted credentials never appear in arguments, environment variables, or output.

```python
import getpass, json, pathlib, urllib.request, uuid

state = pathlib.Path(".state")
addresses = json.loads((state / "listeners.json").read_text())

def call(address, method, path, body=None, bearer=None):
	data = None if body is None else json.dumps(body).encode()
	headers = {"Content-Type": "application/json"}
	if bearer is not None:
		headers["Authorization"] = "Bearer " + bearer
	request = urllib.request.Request("http://" + address + path, data, headers, method=method)
	with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(request) as response:
		data = response.read()
		return json.loads(data) if data else None

if addresses["setup"] is not None:
	ack = input("Do you keep an offline passphrase copy and accept no escrow? Type yes: ") == "yes"
	call(addresses["setup"], "POST", "/v1/owner", {
		"name": input("Owner name: "),
		"password": getpass.getpass("New sign-in password: "),
		"vault_passphrase": getpass.getpass("Separate vault passphrase: "),
		"acknowledge_no_escrow": ack,
	}, (state / "bootstrap.token").read_text())

address = addresses["listen"]
grant = call(address, "POST", "/v1/sessions", {"password": getpass.getpass("Sign-in password: ")})
token = grant.pop("token")
bot = call(address, "PUT", "/v1/bots/" + str(uuid.uuid4()), {"name": input("Bot name: ")}, token)
print(bot)
call(address, "POST", "/v1/vault/unlock", {"passphrase": getpass.getpass("Vault passphrase: ")}, token)
call(address, "PUT", "/v1/provider-keys/mistral", {"secret": getpass.getpass("Mistral key: ")}, token)
print(call(address, "GET", "/v1/status", bearer=token))
call(address, "POST", "/v1/vault/lock", {}, token)
call(address, "DELETE", "/v1/session", {}, token)
del token
```

## Durable task and memory methods

M03 adds these owner-authenticated methods. All mutations use strict JSON and the existing loopback protections. Types come from `link/src/durable.rs` and the generated bindings.

| Method | Request | Result |
| --- | --- | --- |
| `POST /v1/tasks` | `SubmitTask` with client task ID, session, optional conversation, immutable settings and typed work. | `TaskView`. Identical retries return the original task. Changed intent returns `409`. |
| `GET /v1/tasks/{id}` | None. | Persisted task, command identity, approval, progress, outcome, prior effect evidence, and optional bounded process result. |
| `POST /v1/tasks/{id}/cancel` | `{}`. | Durable cancelling intent. Terminal tasks remain terminal. |
| `PUT /v1/tasks/{id}/approval` | `{"expires_at": UNIX_SECONDS}`. | Approval derived from the recorded operation. Expiry must be future, within one hour and before the task deadline. |
| `DELETE /v1/tasks/{id}/approval` | `{}`. | Revoked approval. Admission already consumed cannot be undone. |
| `PUT /v1/bots/{id}/permission` | `{"expected_revision":1,"allow":true}`. | Advanced permission revision and invalidated pending approvals. |
| `POST /v1/conversations` | `Conversation` with `id` and `bot`. | Idempotent conversation. |
| `POST /v1/messages` | `ConversationMessage` with `id`, `conversation` and `text`. | Idempotent immutable message. Snapshots preserve append order, including after restart. A retry keeps the original position. |
| `GET /v1/events/{cursor}/{limit}` | None. Limit is 1 through 64. | Retained events, or a snapshot with a replacement cursor. Zero requests a snapshot. Future cursors return `400`. |
| `PUT /v1/documents` | `WriteDocument` whose kind is `instructions`. | Owner instructions with validated source references and revision. Memory writes return `400`. |
| `PUT /v1/generated-memory` | Legacy `WriteDocument`. | Returns `400`. Use conversational memory tools. |
| `POST /v1/documents/rebuild` | `{}`. | Instruction documents rebuilt from authoritative Markdown sources. |
| `POST /v1/documents/search/{bot}` | `{"query":"text"}`. | Bot-scoped exact instruction matches with source references. Sources reconcile before search. |

[Conversational memory](conversational-memory.md) defines entry operations, scope, revision checks, and migration from legacy Memory documents. Markdown cannot change typed permission or approval policy.

Task states are queued, running, awaiting approval, cancelling, completed, failed, cancelled and awaiting attention. Attention has no generic retry or acknowledgment-to-resume API. Unknown effects cannot become safe through an owner acknowledgment alone.

The first W01 process unit adds `work.type` equal to `process`, with the immutable request in `work.spec`. [The process guide](w01-process-acceptance.md) describes its opt-in configuration and execution limits. Its `TaskView.process` retains exit information, bounded byte output, and effect knowledge independently of later cancellation. Revoking consumed process approval or denying its permission requests stopping; it cannot undo earlier effects.

M03 admits 128 retained tasks and 256 records each for conversations, messages, documents and explicit permissions. Message text and Markdown bodies are limited to 4096 UTF-8 bytes. Documents require one through eight valid task, message or HTTPS source references. Task and message references must belong to that bot. Document paths cannot be supplied by callers.

`POST /v1/worker-commands` now rejects legacy task commands with `unsupported_execution` after checking current fences. Use `/v1/tasks`. Connection-local owner input receipts remain test-only transport diagnostics. Durable task outcomes use `/v1/tasks/{id}`.

## Download a declared output

`GET /v1/tasks/{task}/artifacts/{artifact}` requires the existing owner bearer session and task ownership. The task's `outputs` list reports pending, transferring, available, or unavailable delivery separately from execution completion. Only an available output has a downloadable artifact. Responses use attachment disposition, a fixed declared media type, `nosniff`, and `no-store`. Length and SHA-256 are verified before streaming. Sign in again after a manager restart. Durable artifacts remain retrievable.

## Single-turn text inference

These methods require the owner bearer session and existing strict JSON and loopback protections. Shared types are generated from `link/src/inference.rs`.

| Method | Request | Result |
| --- | --- | --- |
| `PUT /v1/inference-configurations` | `ConfigureInference`. | A new immutable policy revision, initially `unverified`. `expected_revision` is zero for first configuration and the current revision thereafter. |
| `GET /v1/inference-configurations/{bot}` | None. | The latest configuration and verification timestamps. |
| `POST /v1/inference-configurations/verify` | `VerifyInference` with `bot` and `revision`. | Verifies the exact model ID, chat capability, and context ceiling against the selected endpoint. Requires the unlocked vault and stored key. |
| `POST /v1/inferences` | `SubmitInference`. | An admitted durable `InferenceView`. Identical identifiers return the original record, including after restart. Changed input returns `409`. |
| `GET /v1/inferences/{id}` | None. | The persisted request, configuration snapshot, phase, reservation, accounting, timestamps, usage, provider request identifier, and bounded output. |
| `POST /v1/inferences/{id}/cancel` | `{}`. | Cancels and releases an unsent admission. After sending, records intent and keeps the reservation until valid completion evidence arrives. |

`InferencePolicy` requires an endpoint enum of `global`, `eu`, or `us`, an explicit pinned `model_id`, and `context_tokens`. There is no default or arbitrary URL. Its `acknowledgment` records `model_developer`, `developer_terms_url`, `privacy_terms_url`, `acknowledged_at`, and `release`. Release is `{"type":"stable"}` or `{"type":"preview","terms_url":"https://..."}`. The owner attests to the developer and terms. API catalog metadata does not establish legal or privacy compliance.

`prices` records `input_microusd_per_million_tokens`, `output_microusd_per_million_tokens`, `effective_at`, `expires_at`, and `billing`. The only supported billing value is `text_tokens_only_at_acknowledged_ceilings`. Prices use USD millionths and must include any applicable regional surcharge. Other billing dimensions are unsupported. These are owner-configured ceilings, not a promise about invoices. Verification lasts at most one hour and expires with the price ceilings. Replacing the provider key invalidates verification. Failed verification leaves admission blocked.

`SubmitInference` contains `id`, `bot`, `configuration_revision`, `prompt`, `deadline`, `max_output_tokens`, `max_total_tokens`, and `max_cost_microusd`. The prompt is one user text message, without conversation history or tool execution. Its limit is 4,096 UTF-8 bytes. Deadlines must be future, within 120 seconds, and covered by the price expiry. Output caps range from 1 through 16,384 tokens and must be below the verified context ceiling. Context ceilings are at most 2,000,000 tokens.

Admission reserves the full verified context ceiling at the input price plus the output cap at the output price. Each component rounds upward to one USD millionth with checked integer arithmetic. The total token budget must cover `context_tokens + max_output_tokens`. A short prompt can therefore require a much larger budget than its actual usage. There is no local tokenizer estimate. Limits apply per inference, with at most two admitted or sending calls manager-wide. Metadata verification shares the two-call HTTP limit. The ledger retains at most 128 inferences and 256 configuration revisions.

Phases are `admitted`, `sending`, `completed`, `cancelled`, `not_sent`, and `awaiting_attention`. Accounting is independently `held`, `settled`, or `released`. Usage settles once against the recorded prices. A deadline before HTTP, including during the local capacity wait, produces `not_sent` and releases the reservation. Valid completion after a cancellation retains both cancellation intent and measured usage. Cancellation after sending does not abort HTTP or prove that the provider stopped billing.

Unknown responses retain the entire reservation without replacement calls. These include transport failures, provider rejections, redirects, missing or contradictory usage, unsupported usage dimensions, unexpected models, malformed content, tool calls, and oversized responses. Usage metadata accepts an absent or `standard` service tier. Priority and unknown tiers are unsupported. Output is limited to 32,768 bytes and the provider body to 65,536 bytes. Provider errors and reflected credentials are not returned or persisted. There are no retries, redirects, or ambient proxies.

Restart marks sending attempts `awaiting_attention` and retains accounting. Admitted but unsent runs become `not_sent` and release their reservations. Shutdown interrupts HTTP and persists uncertainty before closing the State queue. There is no generic retry, attention acknowledgment, or reservation-release API. Replay snapshots include all inference records and configuration revisions. Configuration and lifecycle events commit with their authoritative records.

`409` adds `inference_unverified` and `inference_budget`. The first also covers stale configuration authority. `503 busy` covers concurrency and retained-ledger limits. The [architecture](m04-inference.md) describes execution ownership and acceptance limits.

## Durable reasoning runs

`POST /v1/runs` admits an authenticated `SubmitRun`. `GET /v1/runs/{id}` returns its request, accounting, ordered turns, and child task identities. `POST /v1/runs/{id}/cancel` accepts an empty JSON body and records cancellation. Child operations use the existing exact task approval and artifact download methods.

The [run contract](m04-tool-loop.md) describes execution policy, cumulative limits, tool results, and recovery. Shared request and response types are generated from `link/src/run.rs`. The [acceptance record](m04-tool-loop-acceptance.md) distinguishes synthetic worker outcomes from the observed production Linux lane. Both use recorded provider responses.
