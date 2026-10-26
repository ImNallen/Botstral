# Approved production processes

The first W01 process unit runs one owner-approved guest command through the durable task API. W01 remains open. Browser execution, models, Vibe, artifact transfer, and user interface changes remain outside this unit.

## Authority and ownership

`WorkSpec::Process` contains a fixed `linux_systemd_v1` profile, account identifiers, an absolute executable, conventional arguments, explicit environment, working directory, absolute deadline, and limits. The existing command digest covers the entire specification. The deadline must equal the task deadline. The manager's claim transaction checks current capability, generation, revision, owner, journal incarnation, immutable binding, cancellation, permission revision, expiry, and deadline.

`agent/src/worker.rs` owns the journal, session runtime, and pending control confirmations for the daemon's lifetime. Reconnect replaces the session snapshot inside that owner. It does not create an executor. The journal remains the only durable execution writer. Bounded systemd requests run asynchronously and return observations to the owner. The socket loop can receive cancellation and send heartbeats while the guest runs.

Installing a reconnect snapshot does not confirm control of a session with admitted unfinished process work. The worker sends its control confirmation after recovery and verified stopping. A missing journal cannot supply that proof, so its affected session remains pending.

The worker reserves its single execution slot, eight report records, and 32 KiB of helper storage before claiming authority. Completion retains one report slot for a cancellation racing its terminal report. A denied claim releases the slot. An interrupted claim is uncertain. The 128 retained-task limit bounds execution directories and launch tombstones.

## Launch and stopping

The journal commits admission and a unique launch intent before one transient-service request. The service starts the production binary's gated helper. The helper records its manifest digest, journal and execution identifiers, machine fingerprint, boot identifier, systemd invocation, and cgroup. The backend checks these against the retained service and actual resource-controller files.

Before release, the owner rechecks connection recovery, the current input fence, cancellation, private configuration, approval expiry, and deadline. It commits possible release before writing the gate. The helper checks expiry and deadline immediately before `spawn`. Arguments and environment pass directly to the executable. There is no implicit shell, inherited guest environment, or variable expansion. Approving a path does not pin executable bytes or files that it reads.

The helper drains both streams with fixed buffers. It retains hexadecimal byte prefixes and marks truncation and incomplete capture. It persists a result before the owner can observe completion. A helper remains supervised until the owner stops the service. Ordinary exit also requires scope cleanup before terminal completion.

The service uses `Restart=no`, `KillMode=control-group`, `SendSIGKILL=yes`, a two-second stop grace, bounded startup and runtime, and null service output. The fixed backend requires systemd 255, Linux, and cgroup v2. Startup probes a real transient service and verifies `memory.max`, `memory.swap.max`, and `pids.max`. The [systemd 255 command documentation](https://www.freedesktop.org/software/systemd/man/255/systemd-run.html) defines the explicit expansion setting used for helper startup.

Capability probes never release guest work. After interrupted probing, startup cleans its private partial files and stops any recorded probe service before checking capability again. This recovery path does not apply to real command launch intents, which retain conservative uncertainty.

A private `EmptyScopeProof` is constructed only after matching the recorded service invocation and cgroup, completing its stop request, and reading an empty or causally removed cgroup. Missing units, missing handles, and numeric process identifiers do not provide proof. Cancellation and affected `ControlApplied` confirmations wait for that proof. An unproved stop retains the slot and requires attention.

The service runtime ceiling includes gated startup. Guest execution therefore never receives a longer runtime than requested. The helper also checks the absolute deadline during capture. After an uncatchable daemon crash, systemd independently terminates ordinary descendants, including descendants that call `setsid`. This is cooperative supervision under the configured account. It does not isolate hostile processes owned by that account or bound arbitrary guest file writes.

## Recovery and evidence

Recovery observes and stops known unfinished units. It never repeats a start request or release. A launch intent with no observable service remains uncertain even when no guest probably ran. A service that independently expired before restart also remains uncertain when its removed scope cannot be proved through the recorded stop operation. A replacement journal cannot prove an old scope never launched. The manager blocks fresh process claims while an admitted scope is missing from the worker journal. The worker refuses to create an empty journal over retained process files.

`TaskView.process` and the journal's process projection retain effect knowledge, exit information, and output separately from the latest lifecycle event. Cancellation cannot erase them. Limit expiry becomes a failed process completion after verified stopping. An unknown operating-system exit stays `unobserved`. Interrupted execution remains awaiting attention. Later cancellation can use a previously persisted empty-scope proof while preserving the prior evidence.

Signal shutdown records stop intent, waits up to 18 seconds, and exits unsuccessfully if stopping cannot be proved. The systemd service continues to enforce its independent bound. The fixture deliberately observes this uncertainty instead of claiming that an absent service proves success.

## Bounds and compatibility

The initial ceilings are 2 KiB of serialized specification, 32 arguments, 32 environment entries, 1 KiB retained per stream, 300 seconds, 32 tasks in the service cgroup, and 256 MiB memory. The process count includes the helper and guest threads. Private configuration can lower these ceilings. Requests exceeding them are refused rather than rewritten.

Report variants have explicit serialized bounds. Process-result reports allow 8 KiB. Scope-bearing reports allow 2 KiB. Other reports retain the 1 KiB limit. Report and inventory pages remain at most 48 KiB within a 64 KiB envelope. A complete task record is capped at 24 KiB. Captured output uses one bounded result record, with no chunk protocol. Helper files are each capped at 8 KiB. Operating-system command output is capped at 16 KiB. Systemd requests have three-, five-, or six-second timeouts. The existing transport queues and 64-session limit bound control state.

Protocol version 3 and generated bindings change together. Worker journal version 2 adds report reservations transactionally and preserves populated version 1 records. The manager's existing version 3 storage needs no table migration. Old task JSON reads the new optional process projection as absent. The existing production refusal of synthetic work, legacy command refusal, separate synthetic binary, and both nonproduction release-feature refusals remain enforced.

## Local implementation choices

The selected ownership contract permits the daemon-lifetime owner to run inside the transport's bounded event loop. Only systemd requests use spawned asynchronous tasks. These tasks never write the journal or release a guest. This keeps one lifecycle writer and avoids another execution actor.

The supervisor and owner integration form one verified implementation commit because an unwired backend cannot pass the production command fixture. The earlier commits preserve the actual-binary failure and protocol contract. This changes the verification-unit grouping, not the selected authority or behavior contract.

The helper storage reservation is released immediately before manifest creation. Capture files remain bounded and are retained with the journal's tombstone. They are not deleted before durable import. The fixture cleans only its own units and resources.

The implementation owner does not provide an independent review. The parent owns comment review, adversarial review, acceptance, and any pull request.
