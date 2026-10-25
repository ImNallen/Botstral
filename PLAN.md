# Botstral plan

M01 is implemented and passes local acceptance. Final independent review closure remains pending.

M01 checkboxes remain open until review closes. Later milestones remain planned.

## Product target

Match Grok Bot's documented personal features for one owner, with self-hosted application infrastructure and Mistral models. M01 through M06 establish the foundation. They do not complete the feature-parity target.

The manager, task state, memories, skills, files, and shared computer run on infrastructure the owner controls. Hosted model inference and explicitly connected external services remain outside that infrastructure.

The parity baseline is the personal Grok Bot documentation reviewed on October 25, 2026. Match user-visible capabilities rather than assuming its internal implementation or copying its commercial limits. Track service-dependent limitations openly.

## Agreed design

- Each installation serves one owner and their paired devices.
- One Go manager coordinates bots, conversations, tasks, permissions, and connections.
- One persistent Linux virtual machine provides the bots' shared computer.
- Many bots can work concurrently, each with its own screen, conversation, instructions, and memory.
- Bots share files, installed tools, browser logins, and command-line credentials.
- Bots have access across the guest filesystem and command line under the configured guest account.
- Coding workspaces organize work and reduce conflicts. They do not restrict filesystem access.
- Guest administrator permissions belong to virtual machine setup. Bot access does not extend to the manager host or Proxmox host.
- Desktop and mobile apps use Tauri 2 with a shared React, TypeScript, and Vite frontend.
- A small Rust layer handles native device features. The manager remains an independent server.
- The manager provides secure device pairing, remote access, reconnection, and revocation.
- Mistral provides hosted reasoning. Vibe runs coding jobs inside the shared virtual machine.
- Mistral-developed models are the default for reasoning, coding, helper tasks, and supported speech or vision features. Do not silently substitute another model developer or provider.
- Model Context Protocol connections belong to the owner, with tool permissions configured per bot.
- Tasks continue when a desktop or mobile app closes.

The first deployment targets Linux guests on Proxmox. The manager and worker must also run on other compatible Linux hosts.

Initial allocations are 8 gigabytes of memory for the manager and 32 gigabytes for the shared computer. These are planning budgets, not measured requirements.

## Model and processing choices

Using a European model provider and choosing a processing region are separate decisions. Configure the manager and Vibe explicitly, including utility models and automatic approval classifiers.

- [ ] Discover and verify each selected Mistral model against the configured endpoint.
- [ ] Offer European regional inference and record the selected endpoint for each managed request.
- [ ] Test reasoning, tools, vision, embeddings, transcription, and speech separately before enabling them.
- [ ] Show a blocked capability when the selected model or region cannot support it. Require an explicit owner choice before changing region or provider.
- [ ] Verify Vibe's actual model and endpoint, rather than assuming its account or catalog selects Mistral-developed models.

Mistral documents its European endpoint as covering European Union and European Free Trade Association countries. Regional inference does not make every account or operational record regional. If the owner requires strictly European Union-only processing, verify that separately before claiming support. External services have their own data locations. See [regional inference](https://docs.mistral.ai/inference/regional-inference).

Regional endpoints do not support hosted Agents, Batch, or Files APIs. Keep the corresponding orchestration and storage in Botstral and verify each selected regional capability.

## Manager milestones

Build M01 through M06 in order. The client and computer tracks can progress alongside them where their dependencies allow.

### M01. Start the manager and retain owner data

- [ ] Create the Go service and SQLite migrations.
- [ ] Add local owner setup, sign-in, sign-out, and session revocation.
- [ ] Keep first-owner setup on a dedicated loopback listener that is never exposed through remote access.
- [ ] Store one owner profile. Do not add public registration, organizations, or multiple owner accounts.
- [ ] Create the shared React frontend with a sign-in page, bot list, bot creation form, and connection status.
- [ ] Serve the browser build from the manager.
- [ ] Add development, build, and verification commands.

Complete when the owner can create a bot, restart the manager, and see the same bot after signing in. Unauthenticated requests must not expose bot data.

### M02. Connect the shared computer and track bot sessions

Depends on M01.

- [ ] Register one persistent computer for the installation.
- [ ] Enroll the worker through an expiring, single-use invitation bound to its identity.
- [ ] Define versioned worker commands, capabilities, acknowledgments, and events.
- [ ] Provide a separately authenticated private worker connection that cannot serve owner setup.
- [ ] Track an independent screen session and input owner for each bot.
- [ ] Reject stale input after control changes hands.
- [ ] Allow the owner to take control of one screen while other bot sessions continue.
- [ ] Add a clearly labelled test worker for protocol and failure testing. Exclude it from production builds.

Complete when two test sessions remain distinct and taking control of one leaves the other running. This milestone does not prove real desktop independence.

### M03. Make tasks durable and recoverable

Depends on M02.

- [ ] Persist conversations, task settings, progress, approvals, and outcomes.
- [ ] Establish durable owner preferences and separate bot memory records for P01.
- [ ] Define task states for queued work, active work, approval, cancellation, completion, failure, and attention.
- [ ] Commit task changes, outgoing commands, and events together.
- [ ] Use stable command identifiers and worker journals to recognize repeated delivery.
- [ ] Reconcile existing work after a manager restart or worker reconnect.
- [ ] Require attention when an external action's outcome is uncertain. Do not blindly repeat it.
- [ ] Preserve cancellation requests until the worker confirms that execution stopped.
- [ ] Bind approvals to the exact task, arguments, permission revision, and expiry.
- [ ] Restore progress and missed events when a client reconnects.

Complete when the manager can restart during work and recover the original task without duplicate execution or lost history.

### M04. Connect Mistral and execute work across the virtual machine

Depends on M03.

- [ ] Add the manager's Mistral reasoning and tool loop.
- [ ] Keep orchestration and durable state in Botstral rather than relying on a hosted agent platform.
- [ ] Record model usage, deadlines, budgets, tool requests, and results.
- [ ] Add real worker tools for file operations, shell commands, and processes across the guest computer.
- [ ] Run Vibe coding jobs on the worker with cancellation and separate working directories.
- [ ] Verify the selected Vibe interface before promising command-level approval controls.
- [ ] Copy completed deliverables to manager storage for retrieval from clients.
- [ ] Keep provider credentials out of prompts, ordinary logs, and client applications.
- [ ] Use recorded model responses for repeatable checks and a privately configured, capped credential for live checks.

Complete when a bot creates a file outside its coding workspace and another bot reads it. Also verify a real code change, its relevant tests, and cancellation of a running job.

### M05. Build secure remote access into the manager

Depends on M04.

- [ ] Prototype direct encrypted access and an outbound tunnel using established networking components.
- [ ] Support a reachable relay that can also be self-hosted.
- [ ] Keep the encrypted client connection terminated at the manager.
- [ ] Add expiring device invitations, confirmation, pairing, and separately revocable credentials.
- [ ] Handle browser sessions and packaged Tauri client credentials explicitly.
- [ ] Route authenticated screen streams, input, and task events through the manager.
- [ ] Close existing connections when a device is revoked.
- [ ] Bound stream queues so a slow viewer does not block other clients.
- [ ] Keep first-owner setup absent from public, relay, and worker routes, including requests with misleading forwarded addresses.

Complete when a paired phone connects over cellular networking, reconnects to its session, and loses access after revocation. Confirm that remote clients cannot create the first owner.

### M06. Add plugins and package the manager

Depends on M05.

- [ ] Install Model Context Protocol connections once for the owner.
- [ ] Add connection discovery, authentication, reconnection, credential rotation, and revocation.
- [ ] Remove revoked tools from bot and Vibe discovery and reject further calls, including stale cached requests.
- [ ] Keep remote account tokens in the manager's protected connection store. Give local plugins only the credentials required for their connection.
- [ ] Keep account authorization separate from each bot's tool permissions.
- [ ] Support remote Streamable HTTP servers and local standard-input and standard-output servers.
- [ ] Run local plugin processes inside the shared virtual machine.
- [ ] Record tool discovery, permission decisions, calls, and results.
- [ ] Expose permitted plugin tools to Vibe through the manager's gateway.
- [ ] Add installation, update, diagnostics, backup, and restore procedures.
- [ ] Package the manager and worker as independently deployable Linux services.
- [ ] Verify installation on Proxmox-hosted Linux and another compatible Linux environment.

Complete when two bots reuse one connected account with different tool permissions and a backup restores the owner's bots, histories, and deliverables. Verify that revoking the connection blocks further calls from both bots and Vibe.

Tool permissions govern calls through the gateway. Bots with shared computer access also share its credentials. Per-bot tool settings are not a security boundary between bots.

## Feature-parity milestones

These are product outcomes. Split implementation into smaller changes as needed. Start a milestone when its dependencies are available rather than waiting for every foundation milestone.

### P01. Preserve memory and manage bot identities

Depends on M03 and M04.

- [ ] Store durable preferences, decisions, task summaries, and source references separately for each bot.
- [ ] Retrieve relevant memory in later conversations without sending every historical message to the model.
- [ ] Let the owner inspect, correct, and delete saved memory.
- [ ] Add bot names, avatars, descriptions, pinning, organization, hiding, and duplication.
- [ ] Copy profile settings and avatars when duplicating a bot. Exclude conversation history, learned memory, attachments, and credential values.
- [ ] Add bot deletion with cancellation of active tasks and an explicit policy for retained or deleted data.
- [ ] Export and import bot templates with explicit choices about included configuration. Exclude credentials and private conversation history by default.

Complete when a bot applies a corrected preference in a new conversation after a manager restart. Verify that duplication and template import preserve the intended configuration without copying excluded private state. Verify that deleting a bot stops its active tasks. P02 and P03 extend duplication and deletion to skills and routines when those capabilities exist.

### P02. Save skills and teach workflows

Depends on M04 and P01. Demonstration capture also requires real computer sessions.

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

- [ ] Add durable routines with an owning bot, schedule, time zone, next run, and history.
- [ ] Copy routine definitions when duplicating a bot, keeping copies paused until the owner activates them. Remove routines when their owning bot is deleted.
- [ ] Support creation, editing, testing, pause, resume, and deletion through the app and conversation.
- [ ] Handle daylight-saving changes, missed runs, restarts, duplicate triggers, and overlapping work explicitly.
- [ ] Add authenticated webhooks and supported event subscriptions with narrow matching rules.
- [ ] Let bots request future wakeups and follow-ups within owner-defined budgets and permissions.
- [ ] Deliver results, failures, and requests for decisions without requiring the owner to keep a conversation open.

Complete when a routine runs while clients are closed, survives a manager restart, and handles a repeated event without repeating its external effect. Verify that deleting its bot prevents further triggers and duplicated routines remain paused.

### P04. Let bots coordinate and hand off work

Depends on M03, M04, and P01.

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
- [ ] Provide a searchable library of conversations, bot work, files, skills, and routines.
- [ ] Link results to their source task and retain version and provenance information.
- [ ] Support editable reports and interactive artifacts in isolated views without native client privileges.
- [ ] Keep important outputs in durable storage rather than temporary guest directories.

Complete when the owner finds an earlier result, inspects its source task, edits or exports it, and retrieves it after a restart or restore.

### P06. Complete approval and secret-entry workflows

Depends on M03, M05, and M06. Website sign-in and private browser capture also require real shared computer sessions.

- [ ] Add protected secret entry that excludes literal values from ordinary conversations and model inputs.
- [ ] Support private website sign-in, manual takeover, saved credentials, rotation, and revocation.
- [ ] Add an owner-wide vault for logins, payment cards, and contact information, with explicit hints and permissions for their use.
- [ ] Support protected field filling without placing stored values in model inputs. Test supported sites and approval rules before enabling unattended use.
- [ ] Add allow-once, deny, and remembered approval rules with explicit precedence.
- [ ] Apply current rules consistently across bots, routines, plugins, and paired devices.
- [ ] Preserve exact action arguments and meaningful context in approval requests.
- [ ] Verify redaction and private input using synthetic credentials before enabling the flow.

Complete when the owner can authorize an exact action on another device and complete a private sign-in without exposing the entered secret through normal model-visible capture or logs.

Public documentation differs on credential automation. The x.ai guide describes human sign-in and secret requests. Cursor's Vault also stores reusable logins, cards, and contact information. Track those broader functions separately and verify supported sites and approval behavior before claiming automatic sign-in or checkout parity. See [Cursor Vault](https://cursor.com/help/grok-bot/secrets).

These controls do not establish secrecy from arbitrary code with full guest privileges. Keep remote connection credentials off the guest where possible and document credentials deliberately supplied to local processes.

### P07. Add voice and additional contact methods

Depends on M04, M05, and C02.

- [ ] Add editable dictation and voice messages with transcripts using supported Mistral speech models.
- [ ] Add spoken interaction with interruption, tool execution, cancellation, and continued work after the call ends.
- [ ] Verify the selected speech models and region, supported languages, streaming behavior, and client audio permissions.
- [ ] Add owner-configured messaging entry points where the external service provides supported access.
- [ ] Keep bot identity, relevant memory, permissions, and task history consistent across contact methods.
- [ ] Keep drafts editable and require the applicable authorization before sending externally.

Complete when the owner starts work by voice, changes direction by text, and receives the result after leaving the call. Test each contact method independently.

### P08. Recover the computer and connect an optional personal device

Depends on M02, M05, and the real worker. Client integration uses C02 and C03.

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

Depends on the relevant foundation, client, and parity milestones.

- [ ] Show activity, attention states, routine history, connection health, and failed operations.
- [ ] Add per-bot, per-task, and per-routine usage records and configured spending limits for managed execution.
- [ ] Track Vibe and helper inference paths alongside primary reasoning requests.
- [ ] Add notification preferences, quiet periods, themes, accessibility, and device-specific settings.
- [ ] Choose supported interface languages and verify translations and regional formats on desktop and mobile.
- [ ] Deliver version checks, supported updates, diagnostics, and signed client distribution.
- [ ] Verify that updates and restores preserve owner configuration, bot memory, routines, and credentials according to the documented policy.

Complete when the owner can understand ongoing work, control unattended usage, recover a failed connection, and update the installation without losing its documented persistent state.

## Desktop and mobile client track

### C01. Start the shared Tauri client

Depends on M01.

- [ ] Wrap the shared frontend in Tauri 2.
- [ ] Keep frontend source in `web/` and the native host in `web/src-tauri/`.
- [ ] Launch the same basic interface on a desktop, an Android emulator, and an iOS simulator.
- [ ] Adapt navigation and layout for desktop and mobile screens.

Complete when each target renders the shared interface. This establishes packaging, not authenticated mobile access.

### C02. Connect and pair native clients

Depends on C01 and M05.

- [ ] Connect through the manager's versioned interface and authenticated event channels.
- [ ] Validate manager identity and packaged-client origins.
- [ ] Add protected device credential storage and pairing links.
- [ ] Verify sign-out, revocation, suspension, resumption, and reconnection.
- [ ] Bundle the frontend with the app and keep native capabilities separate from remote content.

Complete when a paired native client can reconnect and a revoked client cannot regain access.

### C03. Verify the daily client experience

Depends on C02 and the real computer sessions.

- [ ] Verify conversations, approvals, files, and switching between bot screens.
- [ ] Add client support for the parity milestones as each manager capability becomes available.
- [ ] Verify screen streams, touch controls, keyboard input, and manual takeover.
- [ ] Verify file import, export, and notifications on each target.
- [ ] Test the selected native integrations on real devices.
- [ ] Prepare signed releases after platform behavior is verified.

## Shared computer track

The manager can develop against the test worker while this track proves actual computer behavior.

- [ ] Implement the real Linux worker service and persistent guest storage.
- [ ] Prototype two independent full desktop sessions with correctly routed capture and input.
- [ ] Establish one shared browser identity without conflicting browser profile processes.
- [ ] Verify that both bots reuse a supported browser login.
- [ ] Verify simultaneous typing and navigation without input crossing between sessions.
- [ ] Verify that manual takeover pauses only the selected bot's input.
- [ ] Preserve files and supported browser sessions across worker restarts.
- [ ] Enforce task deadlines and stop cancelled process groups.
- [ ] Measure concurrent browser and build workloads before setting active-bot limits.

Complete when two real bots can work on separate screens, share files and a browser login, and survive disconnects and restarts.

## Parity tracking

Every listed capability starts as planned. Mark it verified only after its stated behavior works. Record a blocker when a required model, device, or external service is unavailable. A working foundation is not full feature parity.

| Personal capability | Delivery track | Status |
| --- | --- | --- |
| Bot identities, organization, duplication, deletion, and templates | M01 and P01 | Planned |
| Persistent conversations and editable bot memory | M03 and P01 | Planned |
| Shared computer, files, logins, and independent screens | M02, M04, and the shared computer track | Planned |
| Skills, reuse, and teaching by demonstration | P02 | Planned |
| Schedules, event triggers, and proactive follow-ups | P03 | Planned |
| Group conversations, delegation, and bot handoffs | P04 | Planned |
| Attachments, documents, interactive artifacts, and search | P05 | Planned |
| Protected credentials, approval rules, and human takeover | M03, M05, and P06 | Planned |
| Reusable vault entries and automatic field filling | P06 and site-specific checks | Conditional |
| Dictation, voice messages, and spoken interaction | P07 | Planned |
| Plugins, shared account connections, and tool controls | M06 | Planned |
| Recovery, rebuilds, snapshots, personal-computer access, and traffic routing | P08 | Planned |
| Desktop and mobile access, notifications, and updates | C01 through C03 and P09 | Planned |
| Usage visibility, spending controls, and unattended work | M04, P03, and P09 | Planned |
| Service-specific entry points and privileged data access | P07 and external integration checks | Conditional |

Grok-provided public X search and the official `@bot` entry point require capabilities belonging to that service. Track functional equivalents through owner-configured integrations where access is available. Do not claim those capabilities work until access and permissions are verified.

Organization administration, team identities, multi-user hosting, and commercial subscription billing are outside this single-owner product. Shared conversations between the owner and multiple bots remain in scope.

## Verification and remaining decisions

- [ ] Verify each milestone through automated checks and its actual running interface.
- [ ] Inspect real files, code changes, process termination, and restored data.
- [ ] Record the acceptance machine, software versions, workload, errors, and performance measurements.
- [ ] Record the manager and worker addresses before deployment checks.
- [ ] Record the relay, phone, and network used for cellular checks.
- [ ] Choose and validate protected credential storage, notifications, and screen streaming on each client platform.
- [ ] Confirm the guest account's administrator privileges during computer setup.
- [ ] Establish measured memory, latency, and concurrency budgets.
- [ ] Keep a current mapping from documented personal Grok Bot features to implemented behavior, evidence, and unresolved limitations.
- [ ] Verify every managed model path against the chosen Mistral model and processing configuration.
- [ ] Verify remote plugin credentials remain in the manager and explicitly identify credentials needed inside the shared guest.

Test workers do not prove desktop isolation or shared browser authentication. Missing toolchains, devices, or credentials leave the corresponding checks open.

Automated virtual machine provisioning and advanced lifecycle adapters can follow the foundation. Multiple owner accounts, commercial billing, and multi-server manager operation are outside the single-owner target.

## Reference behavior

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
