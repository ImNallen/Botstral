# M04 single-turn inference

This is a historical slice record. Its pending-work statements describe that checkpoint. See [M04 completion acceptance](m04-completion.md) for the current milestone result.

This first slice lets an authenticated owner configure, verify, submit, retrieve, and cancel one manager-owned text inference. It does not execute model-selected worker operations. M04 remains open. The [durable tool loop](m04-tool-loop.md) extends M04 separately. Vibe, vision, live provider checks, and live two-VM acceptance remain future work.

## Separate authority and execution

Worker `TaskView` embeds one immutable worker command and requires a worker. Inference has no worker command, so it uses a separate SQLite ledger and shared `InferenceView` schema. Existing worker task, approval, artifact, and recovery rules remain unchanged. Conversation messages do not become provider roles.

The State queue owns configuration, admission, reservation, claim, cancellation, settlement, and restart recovery. Migration five adds configuration revisions, provider-key authority epochs, and inference records. SQL triggers prevent changes to configuration policy and request identity. Ledger changes and authoritative events share each transaction. Replay snapshots include both ledgers and all inference configurations.

A runtime-owned supervisor claims at most two calls and runs HTTP outside the State thread. A private permit contains the decrypted key, the immutable request and configuration, and one durable attempt identifier. HTTP does not receive a Manager clone. Completion waits for queue capacity. A disconnected submitting client cannot cancel execution or discard its result.

Shutdown signals active calls, waits for their completion records, and recovers any remaining admissions. It does not wait for the full provider timeout. Unexpected execution or settlement failures stop further inference admission. Restart never dispatches retained records. Unsent admissions release their reservations; sending attempts retain uncertainty and their full reservation.

## Conditional price and model authority

There are no model, region, provider, or privacy fallbacks. The owner supplies a pinned model, an endpoint enum, its context ceiling, developer and privacy acknowledgments, release consent, and dated USD price ceilings. API metadata verifies exact model availability, chat capability, and the context ceiling. That check does not verify developer identity or legal terms. The owner remains responsible for those attestations, including identifying previews whose model IDs do not contain `preview`.

The adapter calls the selected [official endpoint](https://docs.mistral.ai/inference/regional-inference), checks [model metadata](https://docs.mistral.ai/api/endpoint/models), and sends one [chat completion](https://docs.mistral.ai/api). Regional processing and control-plane data location are separate concerns. Owner price ceilings must include regional surcharges and every supported text-token charge.

Without a tokenizer, input reservation uses the verified context ceiling. Output reserves its entire requested cap separately. Both price products round upward with checked integer arithmetic. The provider can still reject a prompt that does not fit its context. A rejection does not prove zero usage, so its reservation remains held. Admission is a per-request ceiling; daily spending controls and invoice reconciliation are not implemented.

The adapter requests one non-streaming completion at standard capacity with tools disabled. It accepts one assistant string with valid bounded usage and an exact response model. Provider usage can include the documented `standard` service tier. Priority tiers, unknown billing fields, content blocks, and tool responses remain attention. A deadline during the local capacity wait releases the reservation because no HTTP request was sent. Later transport uncertainty retains it. No provider errors are stored or logged. Keys stay in the encrypted vault and private transport permits, outside clients and guests.

The [local API reference](local-api.md#single-turn-text-inference) specifies fields, limits, errors, and cancellation semantics. Fixtures exercise the real adapter through the authenticated HTTP API with a private test-only loopback endpoint. Production accepts only the three official HTTPS origins. Fixture model and price values are synthetic test assumptions, not live availability or pricing claims.
