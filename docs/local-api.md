# Local API reference

M01 accepts HTTP/1.1 on literal loopback addresses. Defaults are `127.0.0.1:43120` for ordinary operations and `127.0.0.1:43121` for first-owner setup. `--listen` and `--setup-listen` accept literal IPv4 or IPv6 loopback socket addresses, including port zero for automatic allocation. Hostnames and non-loopback addresses are rejected.

`--state-dir` defaults to `.state`. The parent directory must already exist. Private `listeners.json` records bound addresses as `{"listen":"127.0.0.1:43120","setup":"127.0.0.1:43121"}`. `setup` becomes `null` after its listener closes. The file is removed on graceful shutdown.

## Request rules

Every request requires exactly one `Host` header matching its literal listener address and port. Every `Origin` header and every `Sec-Fetch-*` header is rejected. Forwarding headers grant no authority. There are no CORS grants. Responses use `Cache-Control: no-store`.

Mutations require exactly `Content-Type: application/json` and a JSON object. Unknown fields, duplicate known fields, malformed JSON, and invalid types are rejected without echoing values. Empty mutations use `{}`. JSON bodies are limited to 16,384 bytes. Headers are limited to 8,192 combined name and value bytes, 64 fields, and a 16,384-byte parser buffer. Header and JSON body reads each have a five-second deadline. Connections close after each response. Each listener admits at most 64 concurrent connections.

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
	"inference": "not_implemented",
	"remote": "disabled"
}
```

`provider` is `null` before entry. `vault` is `locked` or `unlocked`. Metadata makes no claim that a provider connection works.

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
