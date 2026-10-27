# M04 durable reasoning and tool loop

An authenticated owner can submit a run, inspect its turns and child tasks, and cancel it. Recorded provider responses exercise the complete loop with synthetic outcomes and an actual production worker on Linux. Live provider credentials, live two-VM acceptance, Vibe, vision, connectors, and daily spending controls remain outside this slice. M04 remains open.

## Ownership and authority

`SubmitRun` contains the bot, computer session, verified configuration revision, prompt, execution policy, and cumulative limits. `RunSummary` tracks lifecycle and accounting. Each `RunTurn` owns an immutable message history, reservation, and local attempt identity. The private ledger also retains the exact serialized provider request, including tool schemas, before HTTP. A normalized `RunToolIntent` links the provider call ID to one immutable worker command and its task, operation, and execution identities.

Migration six stores runs, turns, and child links separately from single-turn inference. Immutable columns and unique constraints preserve request, attempt, ordinal, reservation, and child identity. The State queue remains the sole SQLite writer. The transaction settling an accepted proposal also inserts its child task, command, parent link, and events. Both ordinary submission and run children use the same task validation and insertion functions. Dispatch follows commit.

Creating a child grants no execution authority. The owner grants the existing exact child approval through `PUT /v1/tasks/{id}/approval`. Worker claim checks that binding, permission revision, expiry, worker incarnation, input fence, resource capacity, and parent eligibility. Permit publication checks authority and the parent again. Public task submission cannot supply a parent link.

## Provider transport and accounting

Single-turn inference and runs share the private Mistral transport and its two-call semaphore, including metadata verification. Model metadata must verify function calling for a run. Text-only models remain usable by the existing inference API. Runs acquire transport capacity before durable admission, so waiting for capacity cannot preserve stale model authority.

The adapter requests standard capacity, one completion, and `parallel_tool_calls: false`. It preserves ordered user, assistant, and tool roles and provider call IDs. The function registry derives argument schemas from the typed proposals. Provider wire objects remain private. The [chat API](https://docs.mistral.ai/api/endpoint/chat) and [model metadata API](https://docs.mistral.ai/api/endpoint/models) define the transport fields. Fixture model and price values are synthetic assumptions.

Each new turn atomically reserves the entire verified input context plus the requested output cap. Checked price multiplication rounds each component upward. Admission includes all settled usage and held reservations. Valid usage settles even when a response contains multiple calls, an unknown tool, malformed arguments, a duplicate call ID, or another unsupported proposal. Such responses create no executable child. Missing or invalid usage retains the full reservation and requires attention. Single-turn inference keeps its previous ledger behavior.

Turn, token, cost, operation, deadline, and concurrency limits apply before another operation. A tool response on the final allowed model turn stops after settlement. A child is eligible only while its run can still admit the follow-up model turn. Configuration, capability verification, key epoch, and deadlines are checked for every new model admission.

Text turns retain the provider's `stop` or `length` finish reason. A length-limited final response produces an `output_limit` stop, with its partial answer and settled usage preserved. It is not reported as a completed answer.

## Tool policy and results

`computer` accepts a bounded operation batch and names of owner-declared output slots. Browser actions require a preceding navigation in the same batch because executions do not preserve tabs. File operations accept absolute guest paths, including shared files outside a coding workspace. `process` accepts an executable, arguments, and working directory. The owner supplies accounts, profiles, runtime digest, environment, viewport, resource limits, and output declarations. A proposal cannot increase these limits or change approval authority. Existing `ComputerSpec` and `ProcessSpec` validation remains authoritative.

This is not a guest filesystem sandbox. Arbitrary guest code can have remote effects. Cancellation proves stopping, not reversal of effects.

A completed task can still have pending artifacts. The loop waits until every declared output is available or explicitly unavailable. Result collection checks the task and artifact ownership, declarations, readiness, length, digest, and text encoding. Collection runs outside State and cannot open model-selected manager paths. At most 4,096 bytes of artifact text enter a result. Larger text and non-text files contribute metadata only. Process output is bounded by existing worker evidence limits. Results are labeled as untrusted data and committed before the next call.

## Recovery and bounds

The run supervisor outlives the submitting HTTP client. Completion waits for State queue capacity. Committed changes notify reconciliation; a 250-millisecond deadline timer and bounded startup scans provide a fallback. Deadline reconciliation continues while both provider slots are occupied. A ready run has no admitted provider attempt and may safely resume with current authority. A possibly transmitted turn is never replaced after restart. Worker journals recover the same linked child. Result collection may repeat until its exact result commits because collection has no worker effect.

Cancellation blocks new claims, requests normal sticky child cancellation, and settles valid late provider usage without creating more work. A run becomes cancelled only after known child stop evidence. Provider uncertainty retains its reservation. Worker uncertainty preserves evidence and requires attention. Shutdown aborts local provider waiting, records unresolved outcomes, drains settlements, and releases State queue handles. It does not claim to undo a remote effect.

The initial limits are 16 retained runs, eight turns per run, 96 guest operations, one concurrent operation per run, and a one-hour deadline. A computer batch can contain at most 12 operations under the existing worker schema. Run requests are limited to 16 KiB, transcripts to 32 KiB, assistant text to 4 KiB, tool arguments to 2 KiB, and serialized tool results to 8 KiB. Serialized turn views have a 96 KiB guard, private provider requests have a 64 KiB guard, and a full run response has a 1 MiB guard. Retention is a hard admission bound; records are not silently evicted. Replay snapshots contain run summaries rather than duplicated transcripts. Full history is retrieved through `GET /v1/runs/{id}`.

## Local API

- `POST /v1/runs` submits `SubmitRun` and returns `RunView`. Reusing an ID with identical input returns the retained run. Changed input conflicts.
- `GET /v1/runs/{id}` returns the immutable request, configuration snapshot, summary, and ordered turns.
- `POST /v1/runs/{id}/cancel` requests cancellation with an empty JSON body.
- Existing task approval and artifact download endpoints apply to linked children.

All endpoints require the existing owner session token. The protocol version and local status marker remain unchanged. `link/src/run.rs` and the generated protocol bindings define the exact request and response shapes.
