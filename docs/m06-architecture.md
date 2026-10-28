# M06 architecture

## Problem

Implement every M06 requirement through the existing durable task and Linux Process supervisor. Keep account credentials in the manager, local standard-input servers in the Computer VM, and bot permissions independent of account installation. Bot runs and Vibe must use one connector admission authority. Preserve the two-VM installation and prove consistent restoration.

The load-bearing repository constraints are `manager/state/tasks.rs::prepare_task`, `runs/settlement.rs::finish_run_call`, `state/coding.rs::coding_authority`, and `agent/journal/process.rs`. The worker currently has exactly one `process_slot`. A local connector invoked by Vibe cannot be another ordinary worker task: its Vibe parent already occupies that slot. Local connector subprocesses therefore run inside the invoking task's existing systemd scope. The gateway records connector suboperations, as it already records coding inference suboperations, without introducing a second task lifecycle.

## Usage (caller's view)

An owner installs an account once, discovers its tools, and grants selected tools to bots. Installation never implies a grant. Public connection views contain no readable credential value.

```rust
let connection = manager.install_connection(owner.clone(), InstallConnection {
    id: ConnectionId::new(),
    label: "Owner calendar".into(),
    transport: ConnectionTransport::RemoteHttp {
        endpoint: HttpsEndpoint::parse("https://calendar.example/mcp")?,
    },
    authentication: ConnectionSecretInput::Bearer(secret_entry),
}).await?;

// Discovery uses the ordinary approved task path. This works for both transports.
let discovery = manager.discover_connection(
    owner.clone(), connection.id, computer_session,
).await?;
manager.grant_approval(owner.clone(), discovery.id, expires_at).await?;
// Existing task events announce completion; refresh the connection's catalog view.
let tools = manager.connection(owner.clone(), connection.id).await?.tools;
manager.set_connector_grants(owner.clone(), bot, SetConnectorGrants {
    expected_revision: grants.revision,
    tools: vec![tools.named("calendar_search")?.binding.clone()],
}).await?;
```

The owner selects a bounded immutable connector snapshot when submitting a durable run. The manager resolves names, validates schemas, checks current authority, creates the ordinary child task and handles external execution. The model supplies only the tool choice and arguments.

```rust
let permitted = manager.connector_tools(owner.clone(), bot).await?;
request.execution_policy.connectors = Some(ConnectorToolPolicy {
    tools: permitted.bindings(),
    max_calls: 4,
    max_result_bytes: 8192,
});
let run = manager.submit_run(owner.clone(), request).await?;
// Desktop lifetime does not control execution. Existing TaskView approvals apply.
```

Vibe's wrapper sees one loopback MCP server, the manager gateway relay. It never sees remote endpoint tokens and never directly configures owner-installed servers. Gateway discovery filters current grants and the approved Vibe policy snapshot. A call rechecks authority even if Vibe retains a stale definition.

```rust
// agent/vibe.rs::Launch::prepare, inside the already supervised Vibe task.
let relay = ConnectorRelay::for_task(task, execution, policy.connectors.clone());
// Pseudocode: wrapper config installs only this generated loopback endpoint.
vibe_config.mcp_servers = [relay.loopback_mcp_server()];

// Owner rotation/revocation is one authenticated operation, locally or remotely.
manager.revoke_connection(owner, connection.id, connection.revision).await?;
```

## Shape

### Domain first

Add `link/src/connector.rs`. All public types use existing serialization/schema derives and reject unknown fields. Omitted bounds below are validated constructors, not arbitrary strings or vectors. Endpoint types prohibit user information, fragments and credentials in queries. Arguments are canonical bounded JSON objects; schemas are bounded validated JSON Schema. Validation happens at owner, worker and MCP boundaries.

```rust
identifiers!(ConnectionId, ConnectorCallId);

pub struct ConnectionVersion {
    pub connection: ConnectionId,
    pub revision: u64,          // Configuration and lifecycle changes.
    pub credential_epoch: u64, // Never reused, including rotation back.
}
pub struct ConnectorToolBinding {
    pub connection: ConnectionVersion,
    pub catalog_revision: u64,
    pub grant_revision: u64,
    pub name: ToolName,
    pub schema_digest: ArgumentDigest,
}
pub enum ConnectionTransport {
    RemoteHttp { endpoint: HttpsEndpoint },
    WorkerStdio {
        worker: WorkerId,
        account: ProcessAccount,
        executable: GuestExecutable,
        arguments: BoundedArguments,
        cwd: GuestPath,
        environment: PublicEnvironment,
        limits: ProcessLimits,
    },
}
// SecretInput deliberately has no Debug. It is write-only, never in a view.
pub enum ConnectionSecretInput {
    None,
    Bearer(SecretEntry),
    LocalEnvironment(SecretEnvironment),
}
pub enum ConnectionAvailability {
    NeedsDiscovery,
    Ready,
    AuthenticationRequired,
    Unavailable { code: ConnectorFailure },
    Revoked,
}
pub struct ConnectionView {
    pub id: ConnectionId,
    pub label: String,
    pub version: ConnectionVersion,
    pub availability: ConnectionAvailability,
    pub tools: Vec<ConnectorToolDefinition>,
    // No secret values, raw response bodies or credentials in diagnostics.
}
pub struct ConnectorToolDefinition {
    pub binding: ConnectorToolBinding,
    pub public_name: ConnectorFunctionName,
    pub description: UntrustedDescription,
    pub input_schema: BoundedJsonSchema,
}
pub struct BotConnectorGrants {
    pub bot: BotId,
    pub revision: u64,
    pub tools: Vec<ConnectorToolBinding>,
}
pub struct ConnectorToolPolicy {
    pub tools: Vec<ConnectorToolBinding>,
    pub max_calls: u32,
    pub max_result_bytes: u32,
}
pub struct ConnectorProposal {
    pub tool: ConnectorToolBinding,
    pub arguments: CanonicalArguments,
}
pub enum ConnectorOperation {
    Discover { connection: ConnectionVersion },
    Call(ConnectorProposal),
}
pub struct ConnectorSpec {
    pub operation: ConnectorOperation,
    pub runner_runtime: CertificateFingerprint,
    pub account: ProcessAccount,
    pub limits: ProcessLimits,
    pub deadline: i64,
    pub result: OutputDeclaration,
}

// Existing enums acquire these variants/fields.
WorkSpec::Connector { spec: Box<ConnectorSpec> }
ToolProposal::Connector(ConnectorProposal)
RunExecutionPolicy { /* existing fields */ connectors: Option<ConnectorToolPolicy> }
VibePolicy { /* existing fields */ connectors: Option<ConnectorToolPolicy> }
Capability::ConnectorV1
```

The WorkSpec carries no credential or owner-installed command environment. The approved spec binds the connection version, tool schema and normalized argument digest. The manager supplies executable configuration and the required local secrets only after task-bound admission. Remote credentials never leave the manager. The runner digest is verified like the existing Vibe runtime digest.

Internally, `manager/state/connectors.rs` owns the only credential, catalog, grant and call record writers. Discovery definitions are keyed by `(connection, catalog_revision, name)`; grants by `(bot, connection, tool)`, with one revision per bot grant set. Call records are keyed by stable `ConnectorCallId`, unique `(task, execution, caller_request_id)`. There is one record per invocation, including discovery.

```rust
enum ConnectorSource {
    Task { task: TaskId, execution: ExecutionId },
    Vibe { task: TaskId, execution: ExecutionId, caller_request_id: String },
}
struct ConnectorAuthority {
    source: ConnectorSource,
    connection: ConnectionVersion,
    tool: Option<ConnectorToolBinding>, // None only for owner-approved discovery.
    grant_revision: Option<u64>,
    argument_digest: ArgumentDigest,
    deadline: i64,
}
enum ConnectorOutcome {
    NotSent { reason: ConnectorFailure },
    Finished { result: ConnectorResult },
    Unknown { reason: ConnectorFailure },
}
enum ConnectorCallState {
    Reserved,
    DispatchPossible,
    Resolved(ConnectorOutcome),
}
struct ConnectorCallRecord {
    id: ConnectorCallId,
    authority: ConnectorAuthority,
    state: ConnectorCallState,
    revoked_during_call_at: Option<i64>,
    late_result: Option<ConnectorResult>,
}
```

These are effect-admission and evidence records, not tasks. Ordinary `TaskView`, approval consumption, run parentage, cancellation, systemd supervision and artifact publication remain authoritative. The record answers whether an external connector effect may have happened. Task state answers whether the supervised job has stopped. Neither substitutes for the other.

### Exact module and signature integration

`manager/src/manager/connectors.rs` is the single deep facade. It owns bounded async scheduling outside State, HTTP sessions, cancellation handles and reconciliation of gateway requests. State owns durable records; no async task writes SQLite directly.

```rust
impl Manager {
    pub async fn install_connection(&self, owner: impl Into<OwnerAuthority>,
        input: InstallConnection) -> Result<ConnectionView, OperationError>;
    pub async fn connection(&self, owner: impl Into<OwnerAuthority>,
        id: ConnectionId) -> Result<ConnectionView, OperationError>;
    pub async fn discover_connection(&self, owner: impl Into<OwnerAuthority>,
        id: ConnectionId, session: ComputerSessionId) -> Result<TaskView, OperationError>;
    pub async fn replace_connection_secret(&self, owner: impl Into<OwnerAuthority>,
        id: ConnectionId, expected: u64, value: ConnectionSecretInput)
        -> Result<ConnectionView, OperationError>;
    pub async fn revoke_connection(&self, owner: impl Into<OwnerAuthority>,
        id: ConnectionId, expected: u64) -> Result<ConnectionView, OperationError>;
    pub async fn set_connector_grants(&self, owner: impl Into<OwnerAuthority>,
        bot: BotId, input: SetConnectorGrants) -> Result<BotConnectorGrants, OperationError>;
    pub async fn connector_tools(&self, owner: impl Into<OwnerAuthority>,
        bot: BotId) -> Result<ConnectorCatalog, OperationError>;

    // Worker gateway. Authentication is verified by worker_http, never JSON.
    pub(crate) async fn connector_request(&self, worker: VerifiedWorker,
        source: TaskExecution, request: ConnectorGatewayRequest)
        -> Result<ConnectorGatewayReply, OperationError>;
}

impl State {
    fn admit_connector(&mut self, worker: VerifiedWorker, source: TaskExecution,
        request: ConnectorGatewayRequest) -> Result<ConnectorAdmission, OperationError>;
    fn dispatch_connector(&mut self, call: ConnectorCallId)
        -> Result<ConnectorDispatch, OperationError>;
    fn finish_connector(&mut self, call: ConnectorCallId, outcome: ConnectorOutcome)
        -> Result<(), OperationError>;
    fn permitted_connector_catalog(&self, bot: BotId,
        snapshot: &ConnectorToolPolicy) -> Result<ConnectorCatalog, OperationError>;
}
```

`ConnectorDispatch` and secret-bearing `ConnectorGatewayReply` are private. Their externally serialized worker replies are specifically tagged remote result versus local launch material; neither accepts an arbitrary URL, command or environment from a caller.

| Existing boundary | Required integration |
| --- | --- |
| `manager/mod.rs::start_with_provider` | Construct one bounded connector executor next to the durable run/coding executors. Stop admission on shutdown, settle through `call_wait`. |
| `manager/state.rs::migrate` | Version 9 adds connections, encrypted connector secrets, immutable catalogs, bot grants, connector calls and audit. Update the test that currently treats version 9 as future. Recover `DispatchPossible` as Unknown. |
| `manager/crypto.rs` | Add connector encryption with explicit `botstral-connector-v1`, owner identity, connection identifier, transport class and credential epoch binding. Reject ciphertext swapping. Preserve locked startup. |
| `http/connectors.rs`, `http.rs` | Protected owner install, list/view, discover, grants, rotate and revoke routes using StrictJson. Secret routes are write-only. |
| `link/owner.rs`, `state/devices.rs` | Matching OwnerOperation variants and dispatch. Add bounded connector metadata to owner snapshot; never include secrets. Paired remote owner administration is required. |
| `runs/tools.rs` | Build dynamic provider definitions from the manager-resolved catalog and policy intersection; decode against that exact advertised catalog. Add Connector translation to WorkSpec. Persist the advertised definitions with the exact prepared provider request. |
| `provider.rs`, `state/runs.rs` | `PreparedChat::run` receives the resolved catalog rather than independently discovering tools. Revalidate current grants on each turn and before child dispatch. |
| `runs/settlement.rs` | Connector proposal consumes one operation; create normal child task and RunToolIntent atomically using existing prepare/insert path. No direct network call from settlement. |
| `state/tasks.rs`, `state/workers/dispatch.rs` | Validate immutable connector spec, current connection/catalog/grant revisions, worker capability and worker match for stdio at prepare, approval and Permit dispatch. Discovery requires authenticated owner action; it grants no bot tool permission. |
| `agent/connector.rs`, `agent/connector/runtime.py` | Verified launch manifest and lowering to ProcessSpec, matching the Vibe adapter. Use the existing Journal lifecycle and scope stop proof. |
| `agent/journal/process.rs` | Add Connector to permits/reserve/admit/result handling, without adding a slot or second process supervisor. |
| `worker_http.rs` | Route task/execution/call-bound connector gateway requests only after mutual TLS verification. |
| `state/coding.rs` | Factor the existing task/generation/approval/permission/fence/deadline checks into one private task-authority function used by coding and connector admission. Keep provider-specific checks in coding. |
| `manager/coding.rs` | Validate names and canonical schemas against fixed file tools plus the permitted connector catalog. Change both input and completion tool-call validation; expand the bounded count accordingly. |
| `agent/vibe.rs`, `agent/vibe/runtime.py` | Extend the existing loopback relay with MCP; configure Vibe with only this server. A local server is an ephemeral child within Vibe's existing scope. Runtime verification covers new code/configuration. |
| `runs/results.rs`, artifact collectors | Read a fixed declared bounded connector result artifact; verify result settlement against manager call ledger. Unknown outcome overrides a helper exit code of zero. Treat results as untrusted data. |
| `link/export-schema`, schemas, generated TypeScript | Export the entire new domain contract. Increment worker VERSION from 3 to 4 and OWNER_VERSION from 1 to 2 deliberately; update native owner client consumers and compatibility acceptance together. |

### One transport contract

Pin MCP **2025-06-18**, tools-only, deliberately. Send `initialize`, validate the same negotiated version and capabilities, then `notifications/initialized`. Do not silently downgrade. Both external transports and the loopback Vibe MCP server implement this pinned contract. The [versioned lifecycle](https://modelcontextprotocol.io/specification/2025-06-18/basic/lifecycle) defines initialization/version agreement. The [versioned transport specification](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports) defines stdio and Streamable HTTP, including both JSON and SSE HTTP responses.

`manager/connectors/http.rs` implements Streamable HTTP with bounded JSON/SSE parsing, correlated identifiers, `Accept: application/json, text/event-stream`, negotiated version headers and session identifiers. Handle 202 notifications, JSON-RPC errors, authentication failures, session expiration, pagination and tool list changes. No redirect forwarding of authorization. Endpoint TLS verification remains required. Unsupported server-originated sampling, elicitation and roots requests receive protocol errors; advertise none of those capabilities. Never inject server instructions into system prompts.

Use ephemeral sessions for discovery and each tool invocation. Reconnection means establishing and validating a new initialized session for a *new* call. Before a call, list tools in that session and verify the catalog/schema binding. A changed schema invalidates discovery and requires a fresh owner grant. Never replay a possibly transmitted `tools/call` after a broken connection, authentication refresh or HTTP 404. MCP request identifiers are correlation, not remote idempotency keys. GET stream resumption can be a later optimization; no acceptance claim depends on it.

`agent/connector/runtime.py` implements bounded newline-framed stdio, exact request correlation, initialization, paginated discovery, one invocation and shutdown. Run with a clean environment plus only public connection environment and that connection's explicitly installed secret values. No host environment inheritance and no captured raw stderr. Shut down by closing stdin, then terminate/kill under the existing scope deadline. Discovery and calls use fresh processes, so no persistent connector daemon incarnation/reconnection state exists.

Authentication in M06 is explicit protected bearer entry for remote accounts and named environment secrets for local accounts, including credential replacement and visible AuthenticationRequired status. This meets authentication without pretending to implement every OAuth service. OAuth-only connectors remain explicitly unsupported until their flow is implemented; real acceptance must choose an authenticated server supported by this contract.

### Durable call sequence

For bot calls, `finish_run_call` commits an ordinary Connector child task and its exact work. Owner approval and the worker Journal operate unchanged. The Connector helper requests gateway execution under that task and execution identity. For Vibe, the loopback MCP call uses the Vibe task and execution identity, and a stable caller request identifier. Vibe connector policy and grant snapshot are part of the approved Vibe work digest.

Gateway admission checks worker certificate role, worker generation, recovered connection, current running task, exact execution, consumed unrevoked approval, bot permission, input fence, parent-run eligibility, deadline and connector policy. It additionally checks connection active/version/credential epoch, exact tool/schema/catalog binding and current bot grant revision. The tool must appear in the approved snapshot and current grant intersection. Reserve the call and audit the decision transactionally. Duplicate IDs with equal authority/arguments return the existing outcome; different arguments conflict.

Remote dispatch revalidates immediately before committing `DispatchPossible`, then decrypts credentials and makes HTTP outside State. Local dispatch commits `DispatchPossible` *before* handing the helper launch material. This conservative boundary includes a crash after handoff but before actual server transmission. A local receipt or subprocess spawn does not prove absence of effects.

The helper must durably write its call identifier, argument digest and phase marker in the task's private working directory before local launch, and write the complete bounded result atomically before returning it for settlement. No secret is written into that evidence. Once dispatch is possible, recovery never launches the same invocation again. It may re-submit already captured results to the manager. This matches the existing no-replay Process recovery rule.

`finish_connector` is idempotent and uses `call_wait`; it records the result, updates discovery catalog when relevant and emits redacted audit. A bounded timeout, shutdown or broken stream after dispatch yields Unknown, never a safe retry. Run result collection sees Unknown and produces AwaitingAttention. Vibe's relay stops accepting subsequent connector calls after an Unknown and fails the coding job visibly; it does not tell Vibe that the tool simply failed and invite a replacement effect.

### Revocation and stale requests

Revocation/rotation increments connection revision and credential epoch in one transaction; connection revocation clears the effective catalog immediately. Grant edits increment the bot grant revision. Discovery is always computed from live authority intersected with the immutable approved snapshot; no separately synchronized per-bot catalog exists. A revoked tool disappears from future provider/Vibe discovery. Current Vibe inference admission also rejects stale tools in its advertised request. Already in-flight provider completions may mention old tools; settlement still checks current authority and refuses dispatch.

The revocation transaction denies new admission, resolves reserved unsent calls as NotSent, marks DispatchPossible calls revoked-during-call, and schedules best-effort cancellation. Local revocation asks the wrapper to stop the active connector; if stop cannot be confirmed, cancel the entire supervising Vibe/Connector task through the existing scope mechanism. Remote cancellation closes/aborts the session and can send a cancellation notification. Neither action undoes effects. Already admitted work can have effects after the revocation timestamp; this is an explicit in-flight outcome, not authorization for another call.

Old identities and epochs remain in immutable call records. Late results can settle or be attached as late evidence under that identity, even after current credentials change. They never reenable discovery. A settled result remains known; an interrupted call remains Unknown until actual evidence supports reconciliation. Gateway duplicate requests after revocation cannot re-dispatch. The owner can inspect prior results through authenticated history; worker access still requires a live valid task lease.

Audit includes installation/configuration revisions, discovery digest and names, grant decisions, admission/denial codes, normalized argument digest, call identity, result digest/length, transport status class, revocation and late settlement. Ordinary logs exclude endpoints containing secrets, headers, environments, raw JSON-RPC, secret input and raw results. Store bounded untrusted result content in the durable result/artifact path, with installed-secret redaction; do not put it in audit. Tool descriptions/results and schemas have independent count/byte/depth limits.

### Backup, services and images

Keep packaging under `manager/packaging/` and `agent/packaging/`; shared acceptance scripts live under `scripts/`. Add private-root-aware backup code in `manager/src/backup.rs` and `agent/src/backup.rs`, not in the connector executor. Backup manifests live in the backup destination, never as new directories inside LocalState's strict manager root.

Provide offline commands `botstral-manager backup create`, `backup verify`, `backup restore`, and corresponding agent commands. They must acquire the same state locks as the services. The service is stopped first. Manager backup performs SQLite integrity validation and a consistent checkpoint/backup snapshot with no live writer, then copies Markdown sources, encrypted vault/connector records, artifact blobs and required install identity/configuration metadata into a private temporary destination. Publish a digest manifest and rename atomically on the same filesystem. Exclude stale transfer `.part` content; verify ledger-backed complete blobs. Fail backup if any declared available artifact is missing/corrupt rather than silently claiming full deliverables.

Restore validates digest manifest, schema/format version, ownership, symlinks/hardlinks and SQLite integrity before publishing to an empty private destination. Credentials remain encrypted and require the original owner passphrase. Do not regenerate vault or installation identity. Start locked, invalidate old transient sessions/connections and run existing reconciliation. Retained uncertain calls stay uncertain after restore.

The worker backup policy names explicit roots: private identity/configuration/journal/process evidence; selected guest workspaces and coding homes; selected browser profile roots; configured certificate files; installed-tools inventory and pinned runtime/install manifests. Include external paths only through explicit allowlisted roots. Browser sessions and workspaces are preserved only if those roots were selected and all owning guest processes stopped. Installed binaries may be included explicitly or reproducibly reinstalled from pinned manifests; inventory alone is never called tool preservation. Exclude operating-system files and Proxmox host state.

Stop and verify all worker systemd scopes empty before backup, and stop browser processes before copying profiles. Restore the worker journal and execution evidence together on the original logical worker with the old instance stopped. Restoring to a replacement host requires identity/certificate replacement or explicit retained-identity cutover; never run two active copies. A coordinated recovery bundle records manager and worker backup identifiers and quiescence timestamps. The installation script freezes new admission, stops worker tasks/scopes, stops worker and manager, then takes both backups; it restarts only after successful publication. If remote calls cannot be proven finished, record them Unknown rather than promise an application-consistent remote snapshot.

Supply separate systemd units, configuration examples, install scripts, pinned multi-stage OCI builds and diagnostic subcommands. Manager service uses its private nonroot account and state volume. Worker service preserves existing systemd execution privilege requirements while launching tasks under the configured guest account. No combined compose deployment, shared manager state mount, or manager host socket in worker configuration.

The manager OCI image runs conventionally with persistent state and certificate mounts. The worker OCI image must run a real systemd PID 1 with the required delegated cgroup/system-bus support on a separate Linux Computer host. Ship explicit Docker and Podman launch profiles and verify their stop/scope behavior. Do not label an ordinary unprivileged container supported if Backend::probe fails. Provide native services as the preferred Proxmox guest mode; state the worker container's required host permissions exactly. Diagnostics must test TLS paths, permissions, connectivity, vault state, migrations, supervisor/cgroup capability, pinned Vibe/connector runtime hashes, browser roots and backup coverage without printing secrets.

Acceptance requires actual installation and restore evidence on Proxmox Linux and one independently installed compatible Linux host, plus the documented Docker/Podman modes. Missing live hosts or credentials leave those PLAN checkboxes open. Local mocks prove contract behavior, not external installation.

## Synthesis decision

The coordinator selected the Sol task-bound gateway design. The Astra design independently converged on the same task lifecycle and in-scope Vibe subprocesses. Its explicit late-result reconciliation and local permit release boundary reinforce the selected contract. The Luna alternative adds a separate run lifecycle and assumes an inbound worker route, so it is rejected against the outbound-only worker and existing task ownership.

Every bot proposal stays on the existing ordinary task path. Vibe-local subprocesses stay inside the existing parent scope. The coding-call ledger pattern supplies connector effect evidence. There is no new manager task lifecycle or persistent worker connector daemon. The [independent design verdict](m06-design-verdict.md) requires captured grant revisions in each immutable binding and strictly one-shot local launch permits. Duplicate requests return status or already saved results, never launch material again.

## Tradeoffs accepted

- We accept a short supervised worker runner for remote bot calls in exchange for retaining the exact existing task, approval, cancellation and run-intent architecture.
- We accept one initialized MCP session/process per operation in exchange for eliminating persistent plugin connection recovery and revocation state.
- We accept conservative Unknown outcomes after local launch-material handoff in exchange for never replaying a possibly executed external effect.
- We accept cancelling an entire Vibe task when local connector stop is unproved in exchange for relying on one existing stop authority.
- We accept owner-approved discovery using a selected computer session in exchange for avoiding a parallel worker maintenance execution lifecycle. Discovery remains owner-global and creates no bot tool grant.
- We accept bearer and explicit environment authentication initially in exchange for implementing and proving a real bounded authentication contract rather than claiming generic OAuth compatibility.

## Alternatives considered

1. **Manager-native task executor.** Add a target enum to CommandBody, nullable worker/session ownership, manager task admission, manager cancellation and manager effect evidence. Remote calls become more direct, but task recovery and approval policy split across two executors and every current worker-assuming TaskView consumer changes. It hides HTTP but exposes execution placement throughout existing durable APIs. This is a plausible later architecture, but larger than M06 needs.
2. **Persistent worker MCP broker.** Maintain owner-installed stdio processes outside task scopes and multiplex bot/Vibe calls. It hides initialization cost but creates another supervisor, journal, incarnation fence and credential lifecycle. A stale broker lease is a new revocation hole. It loses on interface depth because callers must understand both task and broker lifetimes.
3. **Standalone manager connector calls outside TaskView.** Add RunPhase::WaitingForConnector and connector-specific approval/cancellation/results. The transport facade looks small, but durable runs now coordinate two kinds of child and Vibe needs another nested authority. It duplicates the lifecycle the repository already has.

The chosen design passes the red-flag screen by keeping one task lifecycle, one connector call ledger writer, one current grant intersection, private transport representations and one gateway path. The deliberate two local placements are the same ephemeral connector helper under either an ordinary Connector task or an already running Vibe task, rather than independent lifecycles.

## Implementation reconciliation

This document is the implementation contract. Do not replace task-bound stdio execution with an unsupervised tokio subprocess, execute remote HTTP on State, introduce a second worker process slot, or return Unknown as a normal retryable tool error. Record accepted deviations here before dependent work continues.

The coordinator accepted standalone Python standard-library maintenance commands in place of main-binary backup subcommands. They acquire the same service locks and use SQLite's backup API. The actual manager-state restoration test proves lock exclusion, locked startup, original credential decryption, history, Markdown, and artifact retrieval. This keeps maintenance separate from runtime entry points and adds no Rust dependencies. The recovery implementation and its packaging remain under their component directories.

The coordinator accepted `ConnectorToolPolicy.runner` to bind the helper identity, guest account, and resource limits when an ordinary connector task is created. Remote discovery retains the ordinary-task path and requires an available worker, as allowed by the independent verdict. Shared typed JSON digest vectors replace serializer-dependent hashing across Rust and Python.

Integrated HTTPS acceptance required explicit private-CA trust for a self-hosted connector. The manager enables system certificate roots alongside bundled WebPKI roots. A process-scoped `SSL_CERT_FILE` can name public CA material before startup. Certificate and hostname validation remain enabled. This applies to the manager's reqwest clients and adds no per-connection trust bypass. The [live acceptance record](m06-integrated-acceptance.md) proves authenticated use after a real manager backup and restoration, plus Vibe execution and cancellation in one production worker scope.

The coordinator accepted the independent judge's two corrections before implementation. `ConnectorToolBinding.grant_revision` is included in the exact provider turn, Connector work, and Vibe policy approval digest. Revoke and regrant cannot revive an older snapshot. Local execution material is released once per durable invocation, including across response loss and worker reconnection. The pinned Vibe adapter must demonstrate local stdio execution within its occupied supervising scope.

## Open questions and risks

- Can the pinned Vibe 2.24.2 MCP integration use the loopback Streamable HTTP gateway and preserve stable request IDs, or should the wrapper expose an equivalent local stdio gateway adapter? Inspect and run the installed pinned runtime before choosing the wrapper surface; both must retain the same manager contract.
- Existing Proxmox Ubuntu manager and Computer VMs are reachable through the repository's SSH aliases. Local Docker provides an additional Linux environment. Real connector acceptance still requires a pinned standards-compliant server distinct from the synthetic contract fixtures.
- Can both published worker container profiles prove existing systemd scope cancellation under their documented cgroup configuration? If one fails, fix or narrow the claimed mode before marking packaging complete.

## Next implementation step

Implement link connector types and migration 9 with focused authority/epoch/grant and unknown-recovery tests, then add Connector WorkSpec lowering through the existing Process adapter before adding either external transport.
