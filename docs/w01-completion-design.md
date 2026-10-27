# W01 completion design


Three independent read-only architect candidates ran on gpt-6-astra, gpt-6.1-sol and gpt-6-luna. Each traced the existing code and supplied a caller usage, data shape, runtime/lifecycle/transfer design and verification contract. The gpt-6.1-sol cross-judge received all complete reports through inherited context, read the rubric and checked code. The coordinator read every report and agreed with its base selection.

| Criterion | Astra | Sol | Luna |
| --- | --- | --- | --- |
| Authority | 4 | 3 | 2 |
| Lifecycle | 4 | 4 | 3 |
| Artifacts | 4 | 4 | 2 |
| Runtime and admission | 4 | 4 | 2 |
| Structure | 4 | 4 | 3 |
| Verification | 4 | 4 | 3 |

Choose Astra's first-class Computer work lowered through the existing supervisor. Its approved shape includes account, runtime, limits, typed operations and declared outputs. Graft Sol's bounded viewport and smaller action bound, plus its explicit separation of execution completion and artifact readiness. Reject Luna's opaque executable conventions and per-bot profiles. Preserve one shared profile with serial admission. Preserve the old process profile's ceilings.

The judge required corrections now included in the coordinator contract. Preserve approved absolute guest filesystem access and setup privileges without promising same-account isolation. Transfer authority comes from the admitted immutable output scope and survives ordinary execution-approval expiry. Revocation cannot create new declarations. The manager acknowledges immutable offers before chunks, and duplicate offers/chunks return durable progress. Reserve byte/record capacity before consuming authority and document reservation release. Missing outputs remain explicitly unavailable. Runtime identity belongs in approval and launch manifests and is rechecked before release. Clean context closure preserves completed browser state. Resource ceilings remain experimental until measured.

This is design exploration only. Independent implementation, comment and evidence-trail reviews remain required after code exists.

## Accepted implementation contract


The user authorized completion of W01. Base production commit is e7404fd956f2d7377d74d28d0dbe26f11ee719d7. Read `.audit/w01-completion/grounding.md` in the coordinator checkout before coding. This contract remains coordinator-owned. Copy the accepted design into a tracked `docs/w01-completion-design.md` in the implementation checkout and keep it reconciled with accepted deviations.

## Data shape and ownership

Use `WorkSpec::Computer { spec: Box<ComputerSpec> }` with typed browser steps, file operations and declared output slots. Preserve existing Process and synthetic wire forms. Shell commands continue through Process. Computer work lowers once into a private validated launch shared with the existing journal and systemd supervisor. Do not add another execution service, parallel lifecycle writer, or an opaque script/executable convention at the owner API.

The approved ComputerSpec includes the configured non-root account, pinned runtime identity, deadline, a separate measured resource profile, typed operation and output declarations. The existing ProcessSpec profile and its 256 MiB/32-process ceilings must remain unchanged. Keep complete command specifications at a proved bounded serialized size, preferably the existing 2048-byte bound. Limit browser steps, selectors, URLs, inline text, outputs and metadata. No arbitrary page evaluation operation is needed. Initial operations should support navigation, ordinary click/fill, text capture, screenshot, and bounded file write/read/export. Real shell work already exists. A subsequent approved file export can retrieve a shell-created file.

The worker journal remains the only durable execution-state writer. Extend all supervised-work matching sites coherently, including manager claims, capabilities, recovery inventories, missing-scope safeguards, report validation, cancellation and input control. Retain immutable work identity and exact approval binding. Possible effects precede launch release in durable state. Uncertainty never launches replacement work. Positive empty-scope proof remains mandatory for completion/cancellation/control handoff. A separate Computer capability requires a real packaged browser/runtime probe and working supervision/storage. Production builds still refuse synthetic adapters and debug fault features.

## Runtime, files and admission

Package a locked supported Playwright runtime and matching Chromium under `agent/`, with an explicit reproducible setup command, version and digest verification. Use a pinned supported Node runtime or a verified explicit deployment prerequisite. No downloads during task execution. Probe identity and browser execution before advertising capability. Bind identity to approved work and recheck before release. Read official Playwright docs already cited in grounding, and verify exact current runtime requirements from primary sources.

Use one dedicated shared persistent automation profile, serialized by the existing worker execution slot. All bots reuse supported login state. Do not use a personal Chrome default profile. Document completed cleanly flushed cookies/local storage and any tested IndexedDB as supported state. Open tabs, session storage and unflushed crash-time changes may remain unsupported. Preserve ordinary guest files and completed browser state across worker restarts. Profile corruption must not silently erase state. Idle ordinary daemon restart acceptance is sufficient for previously completed state; active crash recovery must remain truthful. Do not add a new graceful-control protocol unless actual evidence shows it necessary.

Honor the configured guest account's real filesystem access, including approved absolute paths outside coding workspaces. Workspaces organize work and do not restrict filesystem access. The existing non-root account has setup administrative rights. Do not alter its privileges, promise isolation between same-account processes, or claim that the runner prevents all semantic sends/purchases. Manager and hypervisor administration remain outside guest worker authority and guest mounts/credentials. Browser observations are untrusted data and cannot approve operations.

Enforce serial global/per-bot admission across supervised work, including Process shell work. Keep one durable executor. Add real manager admission checks and test that occupied capacity leaves further approval unconsumed. Explicit initial global/per-worker/per-bot active limits of one are acceptable. Avoid building an unused multi-execution executor. Bound retained tasks and queued work using the existing limits or a justified smaller scoped bound.

Before enabling a final Computer resource profile, measure representative real navigation/text/screenshot/login-state/file workloads on the Computer VM. Record cgroup memory peak, task/process peak, successful output checks, startup and cleanup, tool versions and machine. Start experiments with a separate finite 1 to 1.5 GiB and 128-task profile, then select conservative tested ceilings with headroom. These are candidate values, not measured conclusions. Keep the old profile unchanged. Follow benchmark-checklist for any reported comparison or performance conclusion; do not invent capacity promises from a tiny fixture.

## Artifacts

Execution outcome and artifact availability are separate durable facts. A successful execution may have pending or unavailable deliverables, which must be explicit. No missing/corrupt artifact may be presented as available. Output copying/transfer failures never rerun guest effects.

Only declared output slots become artifacts. After positive stopping proof, validate and copy atomically published regular outputs into bounded worker staging. Use no-follow checks and streaming reads, bound bytes before allocation, hash and fsync the snapshot, and persist immutable offers. Treat same-account tampering honestly. Store bytes separately from metadata and snapshots. Metadata names/media types are bounded, and manager paths derive from opaque identities rather than worker paths.

The manager must durably acknowledge an immutable offer before accepting chunks. Repeated identical offers return the same transfer identity and progress. Persist an explicit unavailable state for each missing/rejected requested output. State exactly when unused byte and record reservations are released. Unknown execution outcomes retain their execution reservation until resolved.

Use resumable sequential chunks within the existing 64 KiB WSS envelope, preferably at most 16 KiB raw data with one outstanding chunk and bounded control traffic. Key transfer by worker, journal, execution, declared slot, digest and length. The current authenticated worker connection must match the recorded admitted execution and declared output reservation. An unrelated worker/session/journal/task, extra slot, altered digest, mismatched duplicate or gap is rejected. Transfer of a sealed admitted output uses that recorded scope, not a new launch approval, so normal approval expiry during delayed byte delivery cannot force re-execution. Revocation/cancellation still prevent new launch or invented outputs.

Keep finite per-artifact/per-execution/worker-spool/manager-store byte and record budgets. Candidate bounds are 8 MiB per artifact, 16 MiB per execution, 64 MiB worker spool, 256 MiB manager store, at most four slots and bounded pending records. Validate the arithmetic and reserve capacity before consuming execution authority. Do not claim configurable larger values work without verification. Exhaustion refuses work or marks output unavailable explicitly; it never silently drops bytes.

Manager acknowledges only durable chunk bytes and offsets. Exact duplicate chunks are idempotent. Lost acknowledgments resume from the persisted offset. Verify full length and SHA-256 before atomically promoting storage and committing availability. Reconcile crashes between fsync, rename and SQLite commit. Retain worker snapshots until durable receipt. Bound partial transfers and clean only recorded fixture/staging resources. Owner download requires existing owner session authentication and task ownership, verifies ready bytes, and serves attachment content with safe headers. Partial, corrupt or unrelated blobs remain unavailable.

## Verification and delivery

Create rerunnable actual-binary W01 tests and extend the existing two-VM fixtures. Both strict SSH aliases are reachable, with systemd 255 on the worker. Use a fresh isolated base under `/home/botstral/`, unique service names and no permanent owner state. Prior evidence lives at `/home/botstral/w01-570e76c4` and must not be overwritten. Computer has no Node/npm/Chromium on PATH. Setup sudo is available for required runtime dependencies, but avoid changes outside explicit runtime setup.

Prove real browser navigation/text/screenshot, guest file write/read/export outside coding workspace, existing shell work, exact owner-downloaded artifact bytes, shared login reuse between bot tasks, cancellation of real Chromium descendants, deadlines/hangs, active-slot backpressure with unconsumed approval, mutation/revocation refusal, unavailable capability rejection, duplicate delivery without another effect, manager restart during transfer, lost chunk/final receipt, corruption and partial-data rejection, wrong-worker and unauthenticated download denial, retrieval after manager restart, and guest/browser-state preservation after ordinary worker restart. Preserve truthful unknown-stop and daemon-crash non-replay behavior. Ensure malformed/maximal specs/reports/artifact chunks fit retained inventory/snapshot/envelope bounds.

Run relevant targeted tests and full `make verify`, generated binding drift checks and desktop compatibility verification on macOS and Linux as applicable. Prove unsupported-host behavior. Retain exact commands, source/runtime/binary hashes, raw machine-readable outcomes, cleanup proof, and current scope limits. Do not mark W01 complete unless every W01 checkbox has evidence. If a correctness gap appears, fix it within scope and track it explicitly; unavailable external prerequisites leave affected acceptance open.

Keep ordered verified commits. Apply deslop before commits. Do not merge, deploy, force-push shared branches, open PRs, or start watchers. The coordinator owns independent comment/correctness/audit review, final integration and PR creation. Return the implementation branch, checkout path, commit SHAs, test commands and evidence, accepted design deviations, and unresolved limits. Do the work yourself without spawning agents.

## Implementation deviations

None yet. Validation and independent coordinator review remain open.
