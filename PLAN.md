# Botstral plan

Botstral has a local manager, durable tasks, a production worker, and a desktop implementation for local owner sessions and bots. M04 provides a durable model and tool loop, approved Vibe coding, bounded vision and observed live two-VM acceptance. M05 provides owner-device pairing and remote access. C02 implements the paired macOS workflow.

M01 has local acceptance evidence and independent review. C01 has a verified native desktop interface. M02 two-VM transport and synthetic session acceptance passed. Later checkboxes distinguish implementation from their milestone acceptance. [DESIGN.md](DESIGN.md) defines the architecture and release scope.

## Product target

Build a self-hosted Grok Bot that uses Mistral-developed models instead of US models. Match its documented personal features for one owner. M01 through M06 establish the foundation. They do not complete the feature-parity target.

The manager, task state, memories, skills, files, and shared computer run on infrastructure the owner controls. Hosted model inference and explicitly connected external services remain outside that infrastructure.

The parity baseline is the personal Grok Bot documentation reviewed on October 25, 2026. Match user-visible capabilities rather than assuming its internal implementation or copying its commercial limits. Track service-dependent limitations openly.

## Agreed design

- Each installation serves one owner and their paired devices.
- One Rust manager coordinates bots, conversations, tasks, permissions, and connections using tokio, axum, and SQLite through rusqlite.
- The manager and Computer VM run in two separate virtual machines from the first deployment. There is no combined installation mode.
- The Rust `botstral-agent` daemon runs on the persistent Linux Computer VM. Both services belong to one Rust workspace and remain independently deployable.
- A versioned `botstral-link` protocol uses JSON over secure WebSockets, with shared schemas and Rust and TypeScript bindings.
- Many bots can work concurrently within configured resource and spending limits. Each has its own conversation, instructions, and memory. Independent screens and manual takeover follow initial headless browser execution.
- Bots share files, installed tools, browser logins, and command-line credentials.
- Bots have access across the guest filesystem and command line under the configured guest account.
- Coding workspaces organize work and reduce conflicts. They do not restrict filesystem access.
- Guest administrator permissions belong to virtual machine setup. Bot access does not extend to the manager host or Proxmox host.
- Build the desktop app first using Tauri with React and TypeScript. Add React Native apps for iOS and Android after the desktop workflow is stable.
- Keep the TypeScript protocol library reusable by the later mobile apps. Mobile implementation and native checks do not block the desktop release.
- Markdown files are the source of truth for bot memory and owner instructions. A rebuildable local SQLite index provides text search, with semantic search added later.
- The manager owns an encrypted credential store, approval authority, schedules, and durable task state. Guest code cannot administer the manager or hypervisor.
- The manager provides secure device pairing, remote access, reconnection, and revocation.
- Mistral provides hosted reasoning. Vibe runs coding jobs inside the shared virtual machine.
- Mistral-developed models are the default for reasoning, coding, helper tasks, and supported speech or vision features. Do not silently substitute another model developer or provider.
- Model Context Protocol connections belong to the owner, with tool permissions configured per bot.
- Tasks continue when a desktop or mobile app closes.

### Delivery stages

Complete a stage only when its acceptance evidence exists. Dependencies that name a scoped check require that check, not later work in the same milestone. A milestone that spans stages remains open until all its checks pass.

| Stage | Planned work |
| --- | --- |
| First acceptance milestone | Core checks from M01 through M06, W01, the C01 and C02 desktop workflow, and one P03 routine support one owner, one bot, and one worker. Verify browser and file work, one connector, approvals, cancellation, reconnection, uncertain outcomes, and backup restoration. |
| v1 | Complete the initial multi-bot roster, Vibe coding, protected credentials in P06, core P05 file workflows, and C03 desktop checks. Run initial routines through P03. |
| v1.x | Complete W02 computer sessions and manual takeover, P01 advanced management and semantic retrieval, P02 skills and demonstrations, and expanded connectors, files, routines, usage controls, and recovery. |
| v2 | Deliver P04 bot coordination and P07 in-app voice. |
| Later | Add the C04 mobile companions after the desktop workflow is stable, telephone calling, other contact methods, optional personal-computer access, and traffic routing. |
| Public release | Complete R01 distribution and R02 safe updates for the released capabilities. Publish the license, supported behavior, installation, and recovery instructions. |

The first deployment targets Linux guests on Proxmox. The manager and worker must also run on other compatible Linux hosts.

Initial allocations are 8 gigabytes of memory for the manager and 32 gigabytes for the shared computer. These are planning budgets, not measured requirements.

## Model and processing choices

Using a European model provider and choosing a processing region are separate decisions. Configure the manager and Vibe explicitly, including utility models and automatic approval classifiers.

- [ ] Discover and verify each selected Mistral model against the configured endpoint.
- [ ] Store a configuration table per task role with model identifier, developer, endpoint, capabilities, privacy terms, and usage limits. Use the candidates in [the design](DESIGN.md#3-model-routing), subject to verification.
- [ ] Require explicit owner consent before sending private data to a preview model whose terms permit training on inputs or outputs. Select a verified alternative when consent is absent.
- [ ] Offer European regional inference and record the selected endpoint for each managed request.
- [ ] Test reasoning, tools, vision, OCR, embeddings, moderation, transcription, and speech separately before enabling them.
- [ ] Show a blocked capability when the selected model or region cannot support it. Require an explicit owner choice or configured policy before changing the model, provider, region, or privacy policy.
- [x] Verify Vibe's actual model and endpoint, rather than assuming its account or catalog selects Mistral-developed models.
- [ ] Verify subscription allowance, key scope, and pay-as-you-go settings for the selected service. Do not present subscription and API access as incompatible modes.
- [ ] Maintain dated price estimates and record actual usage. Enforce configured budgets across primary models, utility models, and coding jobs.

Mistral documents its European endpoint as covering European Union and European Free Trade Association countries. Regional inference does not make every account or operational record regional. If the owner requires strictly European Union-only processing, verify that separately before claiming support. External services have their own data locations. See [regional inference](https://docs.mistral.ai/inference/regional-inference).

Regional endpoints do not support hosted Agents, Batch, or Files APIs. Keep the corresponding orchestration and storage in Botstral and verify each selected regional capability.

Run scheduled inference through the selected regional endpoint. Do not switch to global Batch silently. Optional local inference requires separate license, hardware, and quality checks and is outside v1.

## Manager milestones

Build M01 through M04 in order. M05 can proceed after M03 alongside M04. M06 requires both M04 and M05. W01 supplies the real worker for M04. Client work can start after M01 and follow its stated dependencies.

### M01. Start the manager and retain owner data

- [x] Create the Rust workspace, manager service, and SQLite migrations using tokio, axum, and rusqlite.
- [x] Add local owner setup, sign-in, sign-out, and session revocation.
- [x] Keep first-owner setup on a dedicated loopback listener that is never exposed through remote access.
- [x] Store one owner profile. Do not add public registration, organizations, or multiple owner accounts.
- [x] Add interfaces for owner authentication, bot records, and connection status that C01 and C02 can use.
- [x] Keep provider-key setup in protected secret entry outside ordinary conversations and logs. Apps must not retain provider keys or retrieve stored vault values.
- [x] Select an established encryption implementation for the credential store. Add locked and unlocked states, owner-provisioned unlock credentials, and a key recovery policy before storing provider keys.
- [x] Add development, build, and verification commands.

Complete when a local authenticated client can create a bot, restart the manager, and retrieve the same bot after signing in. Unauthenticated requests must not expose bot data. C01 and C02 provide the native client experience.

Local evidence comes from `make verify` on Rust 1.93.0. Seven focused tests and [the actual-binary TCP test](manager/tests/m01_local.rs) pass. TCP acceptance compares the full bot after forced restart, rejects missing and revoked authentication, verifies locked startup and protected key replacement, and scans generated files and logs for synthetic secrets. Formatting, clippy with warnings denied, and the release build pass. [The M01 architecture](docs/m01-architecture.md) records reconciliation and remaining integration checks. [The Linux workflow](.github/workflows/verify.yml) runs the same command.

### M02. Connect the shared computer and track bot sessions

Depends on M01.

- [x] Register the separate Computer VM and add the Rust `botstral-agent` daemon to the workspace.
- [x] Enroll the worker through an expiring, single-use invitation bound to its identity.
- [x] Define `botstral-link` schemas, Rust bindings, protocol versions, correlated requests, capabilities, acknowledgments, and events.
- [x] Establish an outbound authenticated WSS connection from the daemon to the manager. Reserve SSH for bootstrap and recovery.
- [x] Restrict worker credentials to worker methods. Reject attempts to approve actions, enroll owner devices, retrieve unrelated credentials, or change security policy.
- [x] Negotiate protocol versions and capabilities before admitting work. Test visibly rejected incompatible combinations.
- [x] Define commands for starting work, status, cancellation, approved operations, and results. Include stable command and task identifiers.
- [x] Define events for acceptance, progress, redacted logs, approval requests, screenshots, artifacts, completion, failure, and confirmed cancellation.
- [x] Track logical computer sessions and input ownership for each bot. W02 proves independent full desktops later.
- [x] Reject stale input after control changes hands.
- [x] Model manual takeover so the selected bot stops providing input while other sessions continue. Verify the contract with the test worker.
- [x] Add a clearly labelled test worker for protocol and failure testing. Exclude it from production builds.

Complete when the daemon connects across the two VMs, worker-only authorization is enforced, and two test sessions retain distinct ownership. Verify rejection of incompatible protocol versions and stale input. This milestone does not prove real desktop independence.

Local implementation and verification cover the daemon, mutual TLS enrollment, protocol, session ownership, confirmed takeover, cancellation, and production separation. The daemon now passed production transport and synthetic session checks across the actual manager and Computer VMs. Registration and enrollment in this acceptance are fixture-scoped. [The two-VM acceptance record](docs/m02-two-vm-acceptance.md) preserves the results and coverage boundaries. `make verify` runs M01 TCP acceptance, real-binary M02 TLS/WSS acceptance, test-worker session checks, schema drift detection, and clean production builds. [The M02 architecture](docs/m02-architecture.md) records the design and remaining gaps.

### M03. Make tasks durable and recoverable

Depends on M02.

- [x] Persist conversations, task settings, progress, approvals, and outcomes.
- [x] Store owner instructions and per-bot memory as Markdown with source references. Serialize writes through the manager and keep the local search index rebuildable.
- [x] Keep model-generated memory separate from owner-controlled instructions, permissions, and approval policy.
- [x] Define queued, running, awaiting-approval, cancelling, completed, failed, cancelled, and awaiting-attention task states with explicit transitions.
- [x] Commit task changes, outgoing commands, and events together.
- [x] Journal command acceptance and outcomes on the worker before acknowledging them. Repeated delivery reconnects to existing execution or returns its recorded outcome.
- [x] Reconcile existing work after a manager restart or worker reconnect.
- [x] Reconcile external effects through provider operation identifiers or reliable observation. Require attention when an outcome remains uncertain. Do not blindly repeat it.
- [x] Preserve cancellation requests until the worker confirms that execution and its process group stopped. Record effects that happened before cancellation.
- [x] Bind approvals to the owner, bot, task, operation, normalized arguments, permission revision, expiry, and single-use execution identifier.
- [x] Revalidate authority immediately before execution. Changed arguments, expiry, revocation, or policy changes invalidate affected authority.
- [x] Persist authoritative events and bound replay retention. Provide a snapshot and new cursor when a reconnecting client has missed retained history.

Completion requires that a manager restart or duplicate command delivery recovers the original task without starting replacement work. Disconnect the worker after an external effect and before its result is recorded. Verify reconciliation or an attention state. Also verify confirmed cancellation, invalidated approvals, and snapshot recovery beyond event retention.

Local actual-binary acceptance passed. [M03 acceptance](docs/m03-acceptance.md) maps every requirement to commands and saved evidence. Cancellation uses the synthetic adapter's explicit `NoProcessGroup` evidence. It does not establish real process-group termination. The separate synthetic provider ledger proves effect reconciliation and uncertainty without replacement calls. Production provider execution and real process-group supervision remain open under M04 and W01. [Independent implementation review](docs/m03-review.md) verified the corrected scoped implementation.

### M04. Connect Mistral and execute work across the virtual machine

Depends on M03 and W01 for live execution.

Completed on 2026-10-27. [M04 completion acceptance](docs/m04-completion.md) maps live EU Mistral reasoning, browser/file work, second-bot sharing, bounded vision, Vibe edit/test/resume/cancellation, budget refusal and durable artifacts to observed evidence. Independent review approved aggregate closure after the Process stop-proof correction. Failed full-run reports remain failed; a focused two-VM lifecycle follow-up and final-source recorded regressions close their missing gates. Daily account budgets and invoice reconciliation remain later work.

- [x] Add the manager's Mistral reasoning and tool loop.
- [x] Keep orchestration and durable state in Botstral rather than relying on a hosted agent platform.
- [x] Record model usage, deadlines, budgets, tool requests, and results. Enforce turn, token, time, spending, and concurrency limits before admitting each operation.
- [x] Add Playwright browser execution through the supported runtime selected in W01. Treat screenshots and page content as untrusted inputs.
- [x] Add real worker tools for file operations, shell commands, and processes across the guest computer.
- [x] Run Vibe coding jobs with explicit task ownership, resumable sessions, cancellation, separate working directories, and captured output.
- [x] Verify the selected Vibe interface before promising command-level approval controls. Keep unsupported approval operations blocked instead of enabling blanket auto-approval.
- [x] Prefer manager-mediated Vibe inference where compatible. Identify any provider credentials supplied to guest processes and document their exposure.
- [x] Enforce manager permissions before browser, file, shell, connector, and coding operations. Model moderation cannot grant authority.
- [x] Record where unrestricted guest execution prevents enforcing a semantic action rule, including complete send or purchase prevention.
- [x] Copy completed deliverables to manager storage for retrieval from clients.
- [x] Keep provider credentials out of prompts, ordinary logs, and client applications.
- [x] Use recorded model responses for repeatable checks and a privately configured, capped credential for live checks.

Complete when a bot performs a real browser task, creates a file outside its coding workspace, and retrieves its durable artifact. Verify a real Vibe code change, its relevant tests, cancellation, and budget exhaustion. Verify a second bot can read the shared file before v1 multi-bot acceptance.

### M05. Build secure remote access into the manager

Depends on M03. Desktop acceptance is completed in C02. Mobile and cellular checks follow in C04.

- [x] Expose a single TLS listener for the WSS control protocol, with separate method authority for owner devices and workers.
- [x] Verify direct LAN access and select a configured mesh or reachable outbound relay for remote access without a forwarded home-router port.
- [x] Keep the encrypted client connection terminated at the manager.
- [x] Add expiring single-use invitations bound to the manager identity and device role. Require trusted confirmation and attempt limits for short codes.
- [x] Pin manager identity at pairing and define certificate rotation and recovery without silently dropping the pin.
- [x] Issue separately revocable credentials for owner devices and workers. Handle desktop credential storage through C02 and later mobile storage through C04.
- [x] Route authenticated task events and supported computer input through the manager. W02 adds full desktop streams.
- [x] Close existing connections when a device is revoked.
- [x] Bound stream queues so a slow viewer does not block other clients.
- [x] Add heartbeats, dead-peer detection, exponential-backoff reconnect, event resumption, and snapshot recovery through M03.
- [x] Deliver notification hints to the native owner client for desktop display integration in C02. Notifications cannot grant approval authority or replace the durable task queue. C04 adds mobile push later.
- [x] Keep first-owner setup absent from public, relay, and worker routes, including requests with misleading forwarded addresses.

Complete when an authenticated protocol client connects through the configured remote route, resumes after disconnection, and loses its current and future connections after revocation. Confirm that remote and worker clients cannot create the first owner. C02 verifies the desktop app on that route.

M05 protocol acceptance passed with the reusable native owner client on the LAN and through a temporary public bore relay. The relay and fixture processes are stopped. C02 retains graphical pairing, native keychain storage, and platform notification display. See [M05 acceptance](docs/m05-acceptance.md).

### M06. Add plugins and package the manager

Depends on M04 and M05.

- [x] Install Model Context Protocol connections once for the owner.
- [x] Add connection discovery, authentication, reconnection, credential rotation, and revocation.
- [x] Remove revoked tools from bot and Vibe discovery and reject further calls, including stale cached requests.
- [x] Keep remote account tokens in the manager's protected connection store. Give local plugins only the credentials required for their connection.
- [x] Keep account authorization separate from each bot's tool permissions.
- [x] Support remote Streamable HTTP servers and local standard-input and standard-output servers.
- [x] Run local plugin processes inside the shared virtual machine.
- [x] Record tool discovery, permission decisions, calls, and results.
- [x] Expose permitted plugin tools to Vibe through the manager's gateway.
- [x] Define the handling of in-flight connector calls during revocation.
- [x] Add installation, diagnostics, and consistent backup and restore procedures for SQLite state, Markdown memory, the encrypted credential store, and durable artifacts.
- [x] Define which Computer VM files, browser sessions, and installed tools each backup preserves. Restore them according to that policy.
- [x] Package the manager and worker as separate Linux services and OCI images for Docker or Podman. Installation must preserve the two-VM boundary.
- [x] Verify installation on Proxmox-hosted Linux and another compatible Linux environment.

Complete when one bot uses a real connector and a backup restores its history, memory, credentials, and deliverables. Verify that connection revocation blocks further calls from the bot and Vibe. Before v1 multi-bot acceptance, verify two bots reuse one connected account with different tool permissions and revocation blocks both.

Tool permissions govern calls through the gateway. Bots with shared computer access also share its credentials. Per-bot tool settings are not a security boundary between bots.

M06 acceptance is recorded in the [completion index](docs/m06-completion.md). The coordinator reproduced and corrected all four final review findings and added regression checks. Proxmox staging and Docker Linux arm64 passed. Podman, Docker x86_64, OAuth browser flows, and replacement-machine cutover remain unverified.

### First acceptance milestone

Run these checks on one owner, one bot, one worker, and the desktop app after the required foundation work is available. Later multi-bot and mobile checks remain open.

- [x] Start separate manager and Computer VMs, pair the worker, and verify that its credentials cannot approve actions, enroll owner devices, or access unrelated credentials.
- [x] Complete a browser and file task from the client, approve its exact proposed action, retrieve the result, and verify cancellation of a running job.
- [x] Use one real connector and run one durable P03 routine while clients are closed. Repeat its trigger and verify reconciliation rather than a repeated external effect.
- [x] Restart the manager during execution and reconnect to the original task without starting replacement work.
- [x] Disconnect the worker after an external effect and before completion is recorded. Reconcile the outcome or show that attention is required before retrying.
- [x] Disconnect the client beyond event replay retention and restore current state through a snapshot.
- [x] Deny, alter, expire, consume, and revoke approval authority. Verify that stale requests cannot execute.
- [x] Restore a backup and inspect history, memory, credentials, artifacts, and the documented Computer VM state.
- [x] Inspect model and helper requests for the configured Mistral-developed model and endpoint. Verify that privacy and unsupported-capability blockers remain visible.

[First acceptance evidence](docs/first-acceptance.md) records all nine checks. The original eight checks are retained unchanged. The [helper supplement](docs/helper-acceptance.md) verifies live Vibe compaction, configured EU inference, policy refusals, and accounting. Strict aggregation preserves exact production binary identities. Automatic headless CLI compaction remains outside this explicit helper check.

## Feature-parity milestones

These are product outcomes. Split implementation into smaller changes as needed. Start a milestone when its dependencies are available rather than waiting for every foundation milestone.

### P01. Preserve memory and manage bot identities

Depends on M03 and M04.

Recorded provider checks verify that a new run receives corrected per-bot Memory after a manager restart. See [memory snapshot scope and proof](docs/p01-memory.md). The remaining P01 checklist stays open.

- [ ] Store durable preferences, decisions, task summaries, and source references separately for each bot.
- [ ] Build local FTS5 search from the Markdown source and verify that deleting and rebuilding the index preserves retrievable memory.
- [ ] Retrieve relevant memory in later conversations without sending every historical message to the model.
- [ ] Add semantic retrieval with Mistral Embed in v1.x after selecting and verifying a local vector index. Verify its endpoint independently of the main model.
- [ ] Let the owner inspect, correct, and delete saved memory.
- [ ] Add bot names, avatars, descriptions, pinning, organization, hiding, and duplication.
- [ ] Copy profile settings and avatars when duplicating a bot. Exclude conversation history, learned memory, attachments, and credential values.
- [ ] Add bot deletion with cancellation of active tasks and an explicit policy for retained or deleted data.
- [ ] Export and import bot templates with explicit choices about included configuration. Exclude credentials and private conversation history by default.

Complete when a bot applies a corrected preference in a new conversation after a manager restart. Verify that duplication and template import preserve the intended configuration without copying excluded private state. Verify that deleting a bot stops its active tasks. P02 and P03 extend duplication and deletion to skills and routines when those capabilities exist.

### P02. Save skills and teach workflows

Depends on M04 and P01. Deliver in v1.x. Demonstration capture also requires W02 and protected capture from P06.

- [ ] Create an owner-wide skill library with per-bot enablement and versions.
- [ ] Preserve enabled skills when duplicating a bot without duplicating the owner-wide library.
- [ ] Save instructions, inputs, steps, expected outputs, validation, and approval boundaries from a completed task.
- [ ] Add skill editing, search, import, export, and explicit invocation.
- [ ] Record an owner demonstration while that screen is under human control.
- [ ] Turn the demonstration into an editable draft and test it before enabling unattended use.
- [ ] Keep private sign-in steps and secret values outside recordings and model-visible instructions.

Complete when two bots reuse a reviewed skill and a demonstrated workflow succeeds on a fresh example.

### P03. Schedule work and react to events

Depends on M03 and M04. External event sources also need M05 and the relevant M06 connections.

The supported event adapter is an authenticated publisher with named-topic subscriptions. Bot duplication and tombstone deletion include the lifecycle behavior required by routines. [Acceptance evidence](docs/p03-acceptance.md) records the checks and environment limits.

- [x] Add durable routines with an owning bot, schedule, time zone, next run, and history.
- [x] Copy routine definitions when duplicating a bot, keeping copies paused until the owner activates them. Remove routines when their owning bot is deleted.
- [x] Support creation, editing, testing, pause, resume, and deletion through the app and conversation.
- [x] Handle daylight-saving changes, missed runs, restarts, duplicate triggers, and overlapping work explicitly.
- [x] Run inference through the configured regional model endpoint. Keep schedules and run history in the manager instead of using unsupported regional Batch or hosted agent APIs.
- [x] Add authenticated webhooks and supported event subscriptions with narrow matching rules.
- [x] Let bots request future wakeups and follow-ups within owner-defined budgets and permissions.
- [x] Deliver results, failures, and requests for decisions without requiring the owner to keep a conversation open.

Complete when a routine runs while clients are closed, survives a manager restart, and handles a repeated event without repeating its external effect. Verify that deleting its bot prevents further triggers and duplicated routines remain paused.

### P04. Let bots coordinate and hand off work

Depends on M03, M04, and P01. Deliver in v2. Delegated full desktop work also requires W02.

- [ ] Add conversations containing the owner and multiple bots.
- [ ] Support bot mentions, replies, reactions, and explicit context sharing.
- [ ] Allow one bot to delegate a tracked task or send a message that wakes another bot.
- [ ] Record parent and child tasks, task ownership, progress, results, cancellation, and handoffs.
- [ ] Keep the main conversation responsive while delegated work runs.
- [ ] Apply per-session input ownership and resource admission to delegated computer work.

Complete when one bot delegates a task to another, both work independently, and the owner can inspect, redirect, or cancel the task chain.

### P05. Organize files and retrieve prior work

Depends on M03 and M04. External information sources use M06.

- [ ] Add attachments, uploads, downloads, previews, and generated documents.
- [ ] Add document ingestion through the verified Mistral OCR capability where required. Keep the selected endpoint and unsupported states visible.
- [ ] Provide a searchable library of conversations, bot work, files, skills, and routines.
- [ ] Link results to their source task and retain version and provenance information.
- [ ] Support editable reports and interactive artifacts in isolated views without native client privileges.
- [ ] Keep important outputs in durable storage rather than temporary guest directories.

Complete when the owner finds an earlier result, inspects its source task, edits or exports it, and retrieves it after a restart or restore.

### P06. Complete approval and secret-entry workflows

Depends on M03, M04, and C02. Connector-specific flows also require M06. Build protected Playwright fills for v1 on W01. Manual takeover extends those flows through W02 in v1.x.

- [ ] Add protected secret entry that excludes literal values from ordinary conversations and model inputs.
- [ ] Support private website sign-in, saved credentials, rotation, and revocation. Add manual takeover through W02 in v1.x.
- [ ] Extend the encrypted store from M01 with individual owner-created login items, metadata, and per-item usage policy. Do not import an entire password vault automatically.
- [ ] Make items available to the owner's bots under the item's policy. Explain that shared guest access does not isolate bots from one another.
- [ ] Add approval for every fill and automatic fill on explicitly allowed origins. Require approval when page classification is uncertain. A login approval cannot authorize a purchase or send.
- [ ] Keep TOTP seeds on the manager and send only the current approved code. Require approval for every TOTP fill in v1.
- [ ] Bind fill requests to the task, browser session, page, frame origin, and target fields. Revalidate the destination immediately before filling and reject changed destinations.
- [ ] Exclude secret values from prompts, screenshots, recordings, transcripts, logs, memory, and artifacts. Pause capture during protected entry and verify supported sites before unattended use.
- [ ] Verify locked-store behavior after reboot. Automatic unlock requires an explicitly provisioned key and a documented protection, backup, and recovery policy.
- [ ] Prevent clients from retrieving stored secret values. Expose only metadata and protected entry or approval controls.
- [ ] Block future fills when an item is removed. Treat existing website sessions, copied credentials, cookies, and remote grants as separate revocation or rotation work.
- [ ] Add allow-once, deny, and remembered approval rules with explicit precedence.
- [ ] Apply current rules consistently across bots, routines, plugins, and paired devices.
- [ ] Preserve exact action arguments and meaningful context in approval requests.
- [ ] Test denied, modified, expired, previously consumed, and revoked approval requests. None may authorize stale execution.
- [ ] Verify redaction and private input using synthetic credentials before enabling the flow.

Complete when the owner authorizes an exact action on another device and completes a supported private sign-in without exposing the entered secret through normal model-visible capture or logs. Verify origin-change rejection, locked-store recovery, per-fill TOTP approval, and future-fill revocation. Full desktop takeover additionally requires W02.

The initial vault supports credentials required for agent work. Track broader payment-card and contact-information parity separately, with site-specific permissions and checks before claiming checkout support. See [Cursor Vault](https://cursor.com/help/grok-bot/secrets).

These controls do not establish secrecy from arbitrary code with full guest privileges. Keep remote connection credentials off the guest where possible and document credentials deliberately supplied to local processes.

### P07. Add voice and additional contact methods

Depends on M04, M05, and C02.

Deliver desktop in-app voice in v2. Mobile voice requires C04. Telephone calling and additional contact methods follow later.

- [ ] Add editable dictation and voice messages with transcripts using supported Mistral speech models.
- [ ] Add spoken interaction with interruption, tool execution, cancellation, and continued work after the call ends.
- [ ] Verify the proposed Voxtral Mini Transcribe Realtime 3, Mistral reasoning, and Voxtral Mini TTS 3 pipeline. Use the same durable task and approval system as text requests.
- [ ] Choose the LiveKit deployment, WebRTC media path, signaling, relays, and authentication. Document connections beyond the WSS control protocol and where audio is processed.
- [ ] Verify the selected speech models and region, supported languages, streaming behavior, and client audio permissions.
- [ ] Add transcript retention controls with inspection and deletion. Do not save every call automatically to durable memory.
- [ ] Require an authenticated in-app approval card for sensitive actions unless a current owner rule already authorizes them.
- [ ] Add owner-configured messaging entry points where the external service provides supported access.
- [ ] Keep bot identity, relevant memory, permissions, and task history consistent across contact methods.
- [ ] Keep drafts editable and require the applicable authorization before sending externally.
- [ ] Verify telephone-provider geography, access rights, number availability, and all charges before adding calling. Keep telephone costs separate from model and media-hosting estimates.

Complete in-app voice when the owner starts work by voice, interrupts or changes direction by text, and receives an authorized continuing task's result after leaving the call. Verify retention controls and sensitive-action approvals. Test each later contact method independently.

### P08. Recover the computer and connect an optional personal device

Depends on M02, M05, M06, and W01. Client integration uses C02 and C03. M06 supplies first-acceptance backup restoration. Expand recovery in v1.x and add personal-device access later.

- [ ] Distinguish application updates, worker software updates, computer rebuilds, recovery, and snapshot resets.
- [ ] Define and display which files, browser sessions, installed packages, and recent changes each operation preserves.
- [ ] Keep manager conversations and task history available when the shared computer is replaced.
- [ ] Add narrowly scoped, owner-configured lifecycle adapters where image or virtual machine operations require host control.
- [ ] Keep host-management credentials outside bot execution and preserve deployment portability.
- [ ] Add an optional personal-computer connector with separate enrollment, permissions, visibility, and revocation.
- [ ] Show which computer executes each task. Keep access to the personal computer disabled until the owner enables it.
- [ ] Add optional routing of guest web traffic through a paired personal computer, with consent separate from permission to run commands there.
- [ ] Show the active traffic route and reachable networks. On disconnection or revocation, follow the owner's explicit stop-or-fallback policy.

Complete when a supported recovery restores the documented state and a reset requires explicit owner action. Verify that command execution and traffic routing can be enabled independently. Revocation must stop both uses of the personal computer.

### P09. Complete daily management and distribution

Depends on M04, M05, and C02. Integrate later capabilities as their milestones become available. Baseline usage enforcement is required in M04. Complete the richer daily controls in v1.x. R01 and R02 own release and update acceptance.

- [ ] Show activity, attention states, routine history, connection health, and failed operations.
- [ ] Add per-bot, per-task, and per-routine usage records and configured spending limits for managed execution.
- [ ] Track Vibe and helper inference paths alongside primary reasoning requests.
- [ ] Add notification preferences, quiet periods, themes, accessibility, and device-specific settings.
- [ ] Choose supported interface languages and verify translations and regional formats on desktop. Extend the checks to mobile in C04.
- [ ] Expose version checks, update approvals, release-channel choice, and diagnostics through R01 and R02.
- [ ] Verify that updates and restores preserve owner configuration, bot memory, routines, and credentials according to the documented policy.

Complete when the owner can understand ongoing work, control unattended usage, recover a failed connection, and update the installation without losing its documented persistent state.

## Client track

### C01. Start the desktop app

Depends on M01 for app shells and M02 for protocol bindings.

- [x] Create the Tauri desktop app with React and TypeScript, including native tray and notification integration.
- [x] Add the TypeScript `botstral-link` bindings for the M02 protocol before C02 integration.
- [x] Provide desktop sign-in, bot list, bot creation, and connection status as their manager capabilities become available.
- [x] Keep desktop-native features separate from the protocol library and reusable application logic.
- [x] Launch the desktop app and verify its initial navigation and layout.

Implementation, automated checks, and native acceptance are recorded in [the desktop architecture](docs/c01-architecture.md). The [native walkthrough](docs/assets/c01-desktop.mp4) shows the supplied screenshot's layout with synthetic manager data. Sign-in, search, creation, details, themes, sign-out, relaunch, and offline state were verified. Tray restoration and operating-system notification delivery remain manual checks.

Complete when the desktop app renders its initial interface. C02 verifies authenticated access and the complete first-acceptance workflow.

### C02. Connect and pair the desktop app

Depends on C01, M04, and M05.

- [x] Connect the desktop app through `botstral-link` using the manager's versioned WSS interface and authenticated event channels.
- [x] Validate manager identity, certificate pinning, device roles, and packaged-client origins.
- [x] Add protected device credential storage and pairing links.
- [x] Verify sign-out, revocation, suspension, resumption, and reconnection.
- [x] Restore a current snapshot when the app reconnects beyond event replay retention.
- [x] Bundle the desktop interface and keep remote content separate from native capabilities.
- [x] Complete chat, task progress, an exact-action approval, cancellation, and result retrieval in the desktop app for first acceptance.
- [x] Verify the paired desktop app over the configured remote route, including reconnect and immediate loss of access after revocation.

The packaged macOS workflow passed over the configured public relay with the production worker, real Keychain, exact approval, browser/file work, cancellation, app and manager runtime restart, retention-gap recovery, lost-reply reconciliation and current and future revocation. All reproduced independent review findings were corrected and independently rechecked. See [C02 acceptance](docs/c02-acceptance.md).

Complete when the desktop app completes the first-acceptance workflow, pairs and reconnects over the configured route, and loses current and future access after revocation.

### C03. Verify the daily desktop experience

Depends on C02, M04, P05 core file workflows, and P06's v1 protected-fill checks. Full desktop streams and manual takeover also require W02 in v1.x.

- [ ] Verify desktop conversations, approvals, files, and switching between bots.
- [ ] Add client support for the parity milestones as each manager capability becomes available.
- [ ] Verify headless browser results and task progress in v1. Verify full screen streams, pointer and keyboard input, and manual takeover with W02 in v1.x.
- [ ] Verify desktop file import, export, tray behavior, and native notifications.
- [ ] Test native integrations on the supported desktop platforms.
- [ ] Verify native packaging and signing integration for R01 after platform behavior is verified.

Complete the v1 checks when the desktop app supports the released bot, approval, file, and notification flows. Complete the screen and takeover checks only after W02 works on real computer sessions.

### C04. Add mobile companions after desktop is stable

Depends on C03's desktop release checks and M05. Mobile does not block first acceptance or the desktop release.

- [ ] Create React Native apps for iOS and Android using the existing TypeScript protocol library and selected application logic.
- [ ] Adapt chat, activity, approvals, files, and bot navigation to mobile layouts.
- [ ] Add protected device credential storage, pairing links, certificate validation, and revocation on both platforms.
- [ ] Verify suspension, backgrounding, resumption, reconnection, and snapshot recovery beyond event retention.
- [ ] Select mobile notification delivery, such as ntfy with the required platform integration. Document the APNs-connected upstream needed for instant iOS push.
- [ ] Keep push payloads minimal. Fetch authoritative task and approval state from the manager after a notification.
- [ ] Verify a paired phone over cellular networking, including reconnect and immediate loss of access after revocation.
- [ ] Test the released workflows on an iOS simulator, Android emulator, and real phones. Include file import, export, native notifications, and regional formats.
- [ ] Extend screen streaming and touch input after W02, and voice after P07, when those capabilities are available.
- [ ] Verify mobile signing, distribution, and update compatibility through R01 and R02 before releasing the companions.

Complete when both mobile platforms support the released workflows, reconnect reliably, receive notifications under documented conditions, and lose access on revocation. Verify native behavior on real phones.

## Shared computer track

The manager can develop against the test worker while this track proves actual computer behavior. W01 enables initial browser execution. W02 establishes the later shared-desktop experience.

### W01. Run browser and file work on the separate Computer VM

Depends on M02. Integrate the durable worker journal through M03 before M04 live acceptance.

The first process unit runs one explicitly approved guest command with bounded capture, descendant cancellation, and conservative crash recovery. [The verification record](docs/w01-verification.md) includes production acceptance across both VMs. Execution is opt-in and requires systemd 255. [The Computer guide](docs/w01-computer.md) adds the pinned browser runtime, typed file operations, and durable artifacts. [Completion acceptance](docs/w01-completion-acceptance.md) and [independent review](docs/w01-completion-review.md) record the completed milestone and tested limits.

- [x] Implement the Rust Linux daemon on the separate Computer VM with persistent guest storage and its outbound WSS connection.
- [x] Select and package a supported Playwright runtime and its Rust integration or subprocess interface. Drive Chromium through that runtime.
- [x] Run browser, file, and shell operations using the configured guest account. Keep manager and hypervisor administration inaccessible.
- [x] Enforce global and per-bot browser and coding concurrency. Measure representative workloads before setting active limits.
- [x] Apply timeouts and stop cancelled process groups. Reconcile hangs and uncertain effects before restarting work.
- [x] Stage artifacts with transfer size limits and integrity checks, then copy durable deliverables to manager storage.
- [x] Preserve documented files and supported browser state across ordinary worker restarts.

Complete when the real daemon executes browser and file commands across the two VMs, returns verified artifacts, and confirms process-group cancellation. Headless contexts do not establish independent desktop screens.

### W02. Provide independent screens and manual takeover

Depends on W01 and M04. Protected desktop capture uses P06. Deliver in v1.x.

- [ ] Prototype two independent full desktop sessions with correctly routed capture and input.
- [ ] Establish one shared browser identity without conflicting browser profile processes.
- [ ] Verify that both bots reuse a supported browser login.
- [ ] Verify simultaneous typing and navigation without input crossing between sessions.
- [ ] Verify that manual takeover pauses only the selected bot's input.
- [ ] Preserve files and supported browser sessions across worker restarts.
- [ ] Enforce the M02 ownership protocol on real input and capture. Reject stale input after takeover or ownership changes.
- [ ] Verify resource limits under concurrent browser and build workloads without starving manager connections or another session.

Complete when two real bots can work on separate screens, share files and a browser login, and survive disconnects and restarts.

## Release and update track

### R01. Publish stable and nightly releases

Depends on M06. Client distribution also requires C03 checks for the released features.

- [ ] Publish nightly GitHub prereleases from green merges to the main branch with version `X.Y.Z-nightly.<date>.<build>` and OCI images tagged `nightly`.
- [ ] Promote a tested nightly commit manually to stable. Generate release notes against the preceding release in the same channel.
- [ ] Publish SHA256 checksums and authenticated release metadata. Verify publisher authenticity before installation.
- [ ] Support `BOTSTRAL_CHANNEL`, separate app update feeds, and channel-pinned container tags. Record installed versions and image digests.
- [ ] Enforce compatible protocol ranges and release channels. Reject a nightly daemon when paired with a stable manager.
- [ ] Select MIT or Apache 2.0, publish the independent-project disclaimer, and verify applicable naming and distribution rights.
- [ ] Verify Vibe integration permissions separately from its documented headless support before public distribution.
- [ ] Publish supported capabilities, installation instructions, limitations, and the tested backup and recovery procedures.

Complete when an owner can install a verified release on two compatible Linux VMs, pair a supported app, identify the installed channel and versions, and verify artifact authenticity. Verify that incompatible channel or protocol combinations fail before execution.

### R02. Apply updates without losing owner state

Depends on R01, M03, and C02. Add the update approval controls here. P09 expands their daily management interface.

- [ ] Present an owner approval card for a manager update from its selected channel.
- [ ] Stop admitting new work and drain active execution before applying an update. Do not terminate active tasks silently.
- [ ] Back up persistent state and check schema migrations and manager, daemon, and client compatibility before installation.
- [ ] Apply service updates through an external supervisor or container tooling. Make the daemon follow the manager's channel and compatible protocol range.
- [ ] Define and test failed-update recovery, including incompatible database migrations. Binary rollback alone cannot reverse state changes.
- [ ] Verify that updates and recovery preserve owner configuration, bot memory, routines, credentials, artifacts, and the documented Computer VM state.

Complete when an approved update drains work, preserves documented state, and resumes normal execution. Simulate a failed update and restore through the documented recovery path. Verify that an unapproved update and an unauthenticated artifact cannot be installed.

## Parity tracking

Every listed capability starts as planned. Mark it verified only after its stated behavior works. Record a blocker when a required model, device, or external service is unavailable. A working foundation is not full feature parity.

| Personal capability | Delivery track | Status |
| --- | --- | --- |
| Bot identities, organization, duplication, deletion, and templates | M01 and P01 | Planned |
| Persistent conversations and editable bot memory | M03 and P01 | Planned |
| Shared computer, files, logins, and independent screens | M02, M04, W01, and W02 | Planned |
| Skills, reuse, and teaching by demonstration | P02 | Planned |
| Schedules, event triggers, and proactive follow-ups | P03 | Implemented with [acceptance limits](docs/p03-acceptance.md#verification-limits) |
| Group conversations, delegation, and bot handoffs | P04 | Planned |
| Attachments, documents, interactive artifacts, and search | P05 | Planned |
| Protected credentials, approval rules, and human takeover | M03, M05, and P06 | Planned |
| Reusable vault entries and automatic field filling | P06 and site-specific checks | Conditional |
| Dictation, voice messages, and spoken interaction | P07 | Planned |
| Plugins, shared account connections, and tool controls | M06 | Planned |
| Recovery, rebuilds, snapshots, personal-computer access, and traffic routing | P08 | Planned |
| Desktop access, notifications, and updates | C01 through C03, P09, R01, and R02 | Planned |
| Later mobile access and notifications | C04 | Planned |
| Usage visibility, spending controls, and unattended work | M04, P03, and P09 | Planned |
| Service-specific entry points and privileged data access | P07 and external integration checks | Conditional |

Grok-provided public X search and the official `@bot` entry point require capabilities belonging to that service. Track functional equivalents through owner-configured integrations where access is available. Do not claim those capabilities work until access and permissions are verified.

Organization administration, team identities, multi-user hosting, and commercial subscription billing are outside this single-owner product. Shared conversations between the owner and multiple bots remain in scope.

## Verification and remaining decisions

- [ ] Verify each milestone through automated checks and its actual running interface.
- [ ] Inspect real files, code changes, process termination, and restored data.
- [ ] Record the acceptance machine, software versions, workload, errors, and performance measurements.
- [ ] Record the manager and worker addresses before deployment checks.
- [ ] Record the route and network used for desktop remote-access checks. Record the relay, phone, and cellular network when C04 starts.
- [ ] Choose and validate protected credential storage, notifications, and screen streaming on each client platform.
- [ ] Resolve the Playwright runtime, encrypted store and key recovery, remote-access route, and certificate lifecycle before their implementation milestones.
- [ ] Resolve speech endpoints, retention, licensing, and LiveKit deployment before P07. Resolve signing and migration recovery before R01 and R02.
- [ ] Confirm the guest account's administrator privileges during computer setup.
- [ ] Establish measured memory, latency, and concurrency budgets.
- [ ] Keep a current mapping from documented personal Grok Bot features to implemented behavior, evidence, and unresolved limitations.
- [ ] Verify every managed model path against the chosen Mistral model and processing configuration.
- [ ] Verify remote plugin credentials remain in the manager and explicitly identify credentials needed inside the shared guest.
- [ ] Verify synthetic credential redaction across model inputs, screenshots, recordings, transcripts, logs, memory, and artifacts before enabling P06 fills.
- [ ] Verify one-owner enrollment and worker-role rejection across local, relay, and native client paths.
- [ ] Verify ordinary Computer VM restarts preserve the stated files and browser sessions. Show a separate explicit policy for resets and rebuilds.

Test workers do not prove desktop isolation or shared browser authentication. Missing toolchains, devices, or credentials leave the corresponding checks open.

Automated virtual machine provisioning and advanced lifecycle adapters can follow the foundation. Multiple owner accounts, commercial billing, and multi-server manager operation are outside the single-owner target.

## Reference behavior

- [Botstral design](DESIGN.md).
- [Grok Bot overview](https://docs.x.ai/grok-bot/overview).
- [Bots and templates](https://docs.x.ai/grok-bot/bots).
- [Shared computer and apps](https://docs.x.ai/grok-bot/computer-and-apps).
- [Computer settings and traffic routing](https://docs.x.ai/grok-bot/settings-and-notifications).
- [Conversations and collaboration](https://docs.x.ai/grok-bot/chat-and-collaboration).
- [Skills and routines](https://docs.x.ai/grok-bot/skills-routines-and-automations).
- [Files and results](https://docs.x.ai/grok-bot/files-and-results).
- [Approvals and privacy](https://docs.x.ai/grok-bot/approvals-security-and-privacy).
- [Reusable credentials and personal information](https://cursor.com/help/grok-bot/secrets).
- [Mobile behavior](https://docs.x.ai/grok-bot/mobile).
- [Mistral regional inference](https://docs.mistral.ai/inference/regional-inference).
- [Vibe model and provider configuration](https://docs.mistral.ai/vibe/code/cli/configuration-reference).
