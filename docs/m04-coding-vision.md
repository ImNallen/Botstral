# M04 coding and vision

The manager can dispatch an approved Vibe file-edit task and send authorized PNG artifacts to a verified vision model. Recorded-provider tests, real Vibe tests and observed live two-VM checks cover these paths. [The completion record](m04-completion.md) maps each gate to its evidence and explains the failed campaigns and focused follow-up.

## Accepted design

`State` owns the durable coding ledger and all authority decisions. Vibe lowers to the existing Process supervisor, which owns execution, cancellation, resource enforcement, stop evidence, and recovery. Migration seven adds coding leases, immutable provider attempts, and verified vision capability. It does not add a second task lifecycle.

`WorkSpec::Vibe` contains the prompt, an explicit new/resume choice, limits, and an owner-selected policy. The policy binds the bot's configuration revision and model, workspace identity and canonical path, installed runtime digest, guest account, Process resources, file-edit authority, and output declarations. Runs can offer the same operation through `RunExecutionPolicy.vibe`; proposals supply only the prompt and session choice. Both paths require the existing exact task approval before execution.

Each workspace belongs to one bot, worker, and immutable policy. Overlapping coding workspaces are refused. Resume names the latest task and exact native session. It requires a confirmed successful process exit, complete structured output, no denied callbacks or tool errors, and no held or unknown provider calls. Changed ownership, model configuration, runtime, workspace, or native identity conflicts. The worker also takes a nonblocking session writer lock. Failed or uncertain tasks cannot be resumed automatically; their evidence remains available.

## Pinned execution

The worker requires unmodified Vibe 2.24.2. `agent/vibe/seal.py` writes a separate manifest containing dependency versions and runtime file hashes. Startup and permit release verify that manifest and the compiled-in wrapper. `vibe.json` selects the installed directory, manifest, digest, non-root account, and ceilings. Existing installations need no modification.

The wrapper invokes the supported CLI with a separate `VIBE_HOME` per workspace and the generic OpenAI backend. It exposes only `read_file`, `grep`, `write_file`, and `edit`. The owner-approved workspace gets explicit file-tool allowlists. Approval-needed actions use Vibe's programmatic denial callback. Bash, subagents, MCP, hooks, telemetry, update checks, and project trust are disabled. No blanket approval or ambiguous continuation flag is passed. Tests execute as separate exact approved Process tasks.

Vibe can exit zero after a denied callback. The wrapper parses the bounded JSON transcript, reports denial and failed/cancelled-effect counts, and returns failure when any occur. The task therefore fails rather than claiming a successful edit. A declared `transcript` slot retains up to one MiB; other text outputs name files directly inside the workspace. Native session identity and outcome fit the existing one KiB process output cap.

The configuration is file-edit authority under the guest account, not a filesystem sandbox. The working directory and allowlists do not isolate a hostile process with that account. Existing unrestricted Process tasks can have external effects. Cancellation proves descendants stopped; it does not undo an effect.

## Transport and accounting

Vibe 2.24.2 cannot configure an mTLS client certificate through GenericBackend. A small loopback adapter therefore forwards bounded requests to `POST /worker/v1/coding/{task}/{execution}/{attempt}` using the enrolled worker identity. Its local bearer is random and private. The provider key remains in the manager vault and private permit. The Vibe environment contains no provider credentials.

The existing worker listener retains five-second TLS, header, and body bounds. An authenticated coding request has a bounded inference lifetime, so the old five-second connection timeout cannot terminate normal inference. The adapter performs no replay; after an upstream error it refuses further requests from that invocation.

Every request, including compaction and a distinct retry, reserves the full verified input context plus capped output before upstream HTTP. Model, endpoint, output cap, one completion, and standard tier are manager-owned. Requests are limited to 256 KiB, provider responses to 64 KiB, and buffered SSE output to 128 KiB. Shared envelope validation checks model, usage totals, tier, credential reflection, and documented cached/reasoning subset counters. Ordinary bot tools and Vibe tools use separate completion parsers.

A task admits only one unsettled provider attempt. Reserved, sent, settled, unknown, and not-sent states are durable. An identical settled attempt returns its cached response. Changed input conflicts. Unknown outcomes retain reservations and block new calls and resume; restart closes old leases and never substitutes another attempt. Coding jobs register with the shared inference runtime before State admission. Caller cancellation before sending releases the known-unsent reservation; after sending, the job keeps settlement ownership. Shutdown signals and drains these jobs through State within the existing bound. Valid late usage can settle after cancellation, without granting another call. A malformed completion with valid usage is charged but remains unknown for execution.

Coding calls consume their task limits and, when linked, the parent run's aggregate turn, token, and cost limits. `GET /v1/tasks/{id}/coding-calls` exposes owner-readable accounting. This is per-task/run admission, not an account-wide daily spending control.

## Vision

`SubmitRun.images` contains artifact references, never local paths or remote URLs. `RunExecutionPolicy.vision` declares allowed output slots and bounds. Matching screenshot outputs from prior run children can also enter the retained transcript. Every image must be a sealed manager artifact owned by the same owner and bot, with the exact slot, artifact identity, digest, PNG media type, and length.

The manager decodes the PNG and checks dimensions, pixel count, animation refusal, and allocation bounds before admission. Limits are two images, one MiB per image, 1,280 pixels per dimension, 1,638,400 pixels per image, 3,276,800 total pixels, and three MiB after wire expansion. Exact model metadata must verify vision, and the owner must acknowledge `context_tokens_including_images_at_acknowledged_ceilings` billing.

Retained history stores authorized artifact identities and dimensions. Immediately before HTTP, the manager rereads and verifies the bytes and sends `data:image/png;base64,...` in multimodal user content. The same conservative context reservation and usage validation apply. Old text-only requests and histories remain decodable. A vision-only run can omit all execution tools.

## Verification and setup

See [the reproducible runners and evidence](m04-coding-vision-acceptance.md). The provider usage-detail correction from commit `685800647ca11577630941a994da9f9208065aa6` is included with its authenticated HTTP regression. The completion record retains source provenance and independent review.
