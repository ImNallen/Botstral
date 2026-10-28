# M06 grounding

The goal is all of PLAN.md M06. This includes owner-installed MCP accounts, remote Streamable HTTP, worker-local stdio, bot permissions, Vibe gateway access, revocation and audit, consistent backups, separate services and OCI images, and installation proof on Proxmox Linux and another Linux environment. A standalone remote HTTP client does not satisfy it.

## Existing ownership

- `manager/src/manager/mod.rs` owns `Manager`, its bounded `Queue`, runtime construction and public authenticated methods. `Manager::start_with_provider` starts one dedicated State thread. `Queue::call` is bounded admission for owner operations. `call_wait` preserves completion ownership for internal settlements. Do not perform network or long-running process work on State.
- `manager/src/manager/state.rs` owns the only SQLite connection, installation/vault state, live worker and owner connections and private manager root. `State::from_connection` configures WAL and FULL sync, migrates, drops local sign-in sessions, starts with the vault locked, reconciles Markdown/artifacts and recovers inferences/runs/coding.
- Current database version is 8. `migrate` explicitly rejects future versions and executes each numbered migration transactionally. Adding version 9 requires updating the future-version test, which currently sets version 9 to test rejection.
- `manager/src/manager/crypto.rs` encrypts the owner identity with an owner passphrase and stores encrypted provider values using that identity. Provider plaintext is namespaced `botstral-provider-v1\0mistral\0`. Connector ciphertext needs its own identity binding and format, rather than masquerading as Mistral provider keys. There is no automatic unlock or escrow.
- `manager/src/http.rs::regular_router` hosts loopback owner administration. `StrictJson` rejects unknown fields and malformed content. `manager/src/public_http.rs` provides one TLS listener with separate owner and worker authority. Worker requests must carry verified client certificates. Owner requests reject certificate-bearing requests.
- `link/src/owner.rs::OwnerOperation` and `manager/src/manager/state/devices.rs` implement paired remote owner operations. Local REST additions alone will not make connection administration available to paired remote devices. `OwnerAuthority` centralizes authenticated local-session versus paired-device authority.

## Durable bot tools

- `link/src/run.rs` contains `SubmitRun`, `RunExecutionPolicy`, `ToolProposal`, `RunToolIntent`, `RunTurn` and lifecycle/accounting. Policies currently hold Computer, Process, Vibe and vision only. Proposals contain model-selected arguments, while the owner policy supplies profiles, accounts and bounds.
- `manager/src/manager/runs/tools.rs::{definitions,decode,function,translate,validate_policy}` is the existing closed registry. `definitions` is derived from run policy. `decode` rejects unknown tools. `translate` maps each proposal to immutable `WorkSpec` and refuses unsupported tools. `provider.rs::PreparedChat` consumes this registry.
- `manager/src/manager/state/runs.rs::claim_run_work` commits the exact provider request and turn before external HTTP. Its ready branch checks verified model authority, key state and budgets before admission. Its waiting branch waits for terminal child plus sealed available/unavailable outputs before collecting untrusted results.
- `manager/src/manager/state/runs/settlement.rs::finish_run_call` settles billed usage even for unsupported proposals. Accepted tool proposals create one ordinary child task using `prepare_task`. The transaction commits child task, command, parent link, intent and events together. External dispatch follows commit.
- `manager/src/manager/runs.rs::start` runs the durable supervisor beyond client lifetime. Model HTTP and result collection happen outside State. Possibly transmitted provider attempts become attention-required on recovery instead of being retried. Pure result collection can repeat.
- `manager/src/manager/state/tasks.rs::{prepare_task,grant_approval,set_permission}` owns exact task authority. Existing `Permission` is only a revisioned boolean for a bot, not per-connector/tool permission. Per-tool grants need a separate data shape from account authorization. Approval binds operation, normalized argument digest, permission revision, expiry and single-use execution.
- Every current durable tool ultimately has `TaskView` and worker execution. A manager-native remote connector cannot simply run inside the existing `tools::translate` path without a deliberate new durable call/intent model or an explicit work variant with manager ownership.

## Worker execution and local servers

- `link/src/lib.rs::WorkSpec` has Unavailable, synthetic test work, Process, Vibe and Computer. Protocol VERSION is 3. `Capability`, schemas and generated bindings also live in link. Schema drift is checked by `link/src/bin/export-schema.rs` and verification scripts.
- `agent/src/worker.rs` owns transport-facing worker state. `agent/src/journal.rs::Journal` persists execution identity, accepted authority, report sequences and outcomes. It retains incarnation identity and fail-closed report bounds.
- `agent/src/journal/process.rs::{configure_processes,permits_work,reserve_process,admit_process,tick_processes,finish_process}` adapts Process, Computer and Vibe to the existing Linux systemd supervisor. Process lifecycle owns crash recovery, cancellation and empty-scope stopping proof. Vibe and Computer reuse it instead of introducing a second task lifecycle.
- Local stdio MCP must execute on this worker, not the manager. Existing Process work is finite task execution, not a reusable bidirectional stdio session. Choose explicitly whether a local MCP request starts an ephemeral initialized server or a worker-owned persistent connection. Persistent state needs reconnect/incarnation/revocation semantics. Required local account credentials may cross the encrypted worker link, but remote connector tokens must not.
- Adding a `WorkSpec` variant is cross-cutting. Exhaustive matches exist in task validation, dispatch capabilities, command normalization/digests, worker journal admission/recovery, output declarations, run results and schema exports. Typed arguments are digested into exact approval authority.

## Vibe integration

- `link/src/vibe.rs` pins Vibe 2.24.2 and `VIBE_TOOLS` to read_file, grep, write_file and edit. `VibePolicy` binds immutable workspace, runtime digest, guest account, model configuration and resources.
- `agent/src/vibe.rs::Launch` lowers coding to Process. Runtime verification hashes the installed pinned Vibe distribution and wrapper. It writes a private launch file and compiled-in `agent/vibe/runtime.py`.
- The Python wrapper currently explicitly sets `mcp_servers = []`, limits enabled tools, disables hooks, telemetry and update checks, and gives Vibe no provider key. It invokes programmatic mode with denial callbacks rather than blanket auto-approval.
- Its loopback `Relay` forwards only `/v1/chat/completions` to `POST /worker/v1/coding/{task}/{execution}/{attempt}` using enrolled worker mutual TLS. This is an inference gateway, not an MCP gateway.
- `manager/src/worker_http.rs::worker_request` routes coding HTTP. `manager/src/manager/state/coding.rs::{admit_coding,coding_authority}` verifies worker certificate, current worker generation, live recovered connection, running approved task, exact execution, non-revoked permission, expiry, input ownership and parent-run eligibility. `coding_authority` is a strong starting point for task-bound connector gateway authorization.
- `manager/src/manager/coding.rs::CodingRequest::prepare` validates every requested tool against the fixed file-tool list. MCP tools must be added to both Vibe discovery/configuration and manager-side admission. Merely adding a Vibe mcp_servers entry will fail existing validation or bypass the desired manager authority if connected directly.
- Vibe native session resume requires an unchanged full policy and successful previous task without held/unknown inference. Any new connector grant snapshots or revisions in that policy need deliberate rotation and resume behavior.

## Backup and packaging

- No non-evidence service units, Dockerfiles, installation scripts, diagnostics or backup implementation were found. `Makefile` currently has dev/build/verify/desktop targets. Manager CLI supports state/listener/TLS flags only. Agent CLI supports identity/enroll/check/run with file-based private setup.
- `manager/src/local_state.rs::LocalState::open` locks `manager.lock` and strictly requires root mode 0700 and immediate files mode 0600, singly linked, owned and regular. Only `artifacts` may be a directory. Adding a `connections/`, backup manifest directory or packaging file inside the live manager state root breaks startup unless this validator is intentionally expanded.
- `manager.sqlite3` contains owner, vault identity ciphertext, provider ciphertext, conversations, tasks, approvals, histories and indices. A backup must capture correct WAL state, not copy only the live main database. Stopping manager and acquiring its state lock is a viable consistency boundary. SQLite backup API is an alternative, but filesystem publication must also be coordinated.
- Markdown `instructions-{bot}.md` and `memory-{bot}.md` are authoritative at root. State startup reconciles them and never recreates missing sources from derived search data. Include sources with database, not only indices.
- Durable outputs are manager `artifacts/{id}.blob`; transfer parts are `.part`. `State::reconcile_artifacts` validates length/digest, promotes valid content and marks missing/corrupt content unavailable. Backup must preserve declared deliverables and their ledger together.
- Worker private root has `agent.sqlite3`, durable execution/process evidence, `config.json`, optional process/computer/vibe configuration, browser profile state and Vibe `coding/{workspace}` homes. Configuration references external certificate and runtime paths. Ordinary user workspaces and installed tools also live outside that root. Define inclusion by explicit roots and restore policy rather than claim whole-VM backup.
- Do not restore a worker execution journal separately from retained process/scope evidence or clone its live identity into a second active worker. `Journal::open` explicitly refuses a missing journal when process evidence remains. Backups should be taken with worker processes stopped or under a documented quiescence protocol.
- Existing `docs/worker-deployment.md` explains mutual TLS, distinct VM services, file-path-only secrets, mode restrictions and manual certificate recovery. It provides reproducible fixture runners for two existing Proxmox VMs. Paths under `/home/botstral/m02-acceptance` are fixture conventions, not a packaging contract.
- Worker production Process execution requires Linux systemd support. An OCI image alone does not guarantee that backend works under ordinary Docker or Podman. Packaging must either provide the required systemd/cgroup environment, explicitly validate the supported container mode or describe the limitation honestly. Keep manager and worker on separate hosts/VMs.

## Critical design decisions

1. Model connection identity, transport configuration, credential epoch, discovery revision, revocation state and bot tool grants separately. Account installation is owner-global; bot rights are independent.
2. Bind cached discovery and tool calls to current connection/grant revisions. Revalidate before dispatch and reject stale bot and Vibe requests after revocation or rotation.
3. Commit call intent before remote effects. A timeout or shutdown after transmission requires unknown/attention status unless the remote protocol supplies real reconciliation. Never treat an HTTP retry as harmless.
4. Define revocation during in-flight work. Blocking new admission is enforceable. Aborting a client does not undo remote effects or prove absence of effects. Retain and audit late results under the original identity.
5. Audit discovery, permission decisions, calls and outcomes without leaking account tokens through endpoint URLs, headers, local environment, results or ordinary logs.
6. Keep schemas and generated Rust/TypeScript contracts in link. Keep manager, agent and packaging under their component directories per AGENTS.md.

These are code-read findings and implementation inferences. I ran no tests and changed no repository code.
