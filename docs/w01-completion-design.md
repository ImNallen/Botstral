# W01 Computer execution

Owners submit a `ComputerSpec` containing typed browser and file operations, declared outputs, the guest account, runtime identity, deadline, and limits. The existing task approval binds the complete request. Shell commands retain `WorkSpec::Process`.

The manager admits one supervised execution at a time. The worker lowers Computer work into the existing journal and systemd supervisor. One journal writer owns launch, stopping, recovery, and transfer state. Browser observations cannot grant authority. Guest work retains the configured account's filesystem access, including approved absolute paths outside coding workspaces.

Execution outcome and artifact availability are separate durable facts. Transfer failure can make a declared output unavailable without repeating execution. The owner retrieves available files through authenticated manager downloads.

## Design choice

Three architect candidates compared first-class Computer work with a packaged executable convention. Independent judging selected typed operations because approvals, runtime selection, and output authority share one representation. A separate browser service was rejected because it would require another cancellation and recovery owner. Per-bot profiles were rejected because bots share supported login state.

The design retains one shared automation profile and serial admission. Independent desktop screens and manual takeover remain W02 work. [Computer setup](w01-computer.md) describes installation and owner operations.

## Implemented contracts

The implementation uses `ComputerProfile::LinuxPlaywrightV1`, a 2048-byte `ComputerSpec`, at most twelve operations, four output declarations, and a viewport bounded to 1280 by 960 pixels. The old `ProcessSpec` validation and its 256 MiB and 32-task ceilings remain unchanged. There is one worker execution slot and a manager-wide admitted supervised limit of one. This also enforces the initial per-worker and per-bot limit of one.

The selected Computer ceiling is 1 GiB and 128 tasks. Five checked runtime workloads on the Computer VM recorded a cgroup memory peak of 183,066,624 bytes and a task peak of 86 at 800 by 600. Five further checked runs at the maximum 1280 by 960 viewport under the final 1 GiB limit recorded 196,243,456 bytes and 87 tasks. Production acceptance also records live Chromium cgroup counters. These fixtures establish the tested initial ceiling, not a throughput or general website capacity promise.

`ComputerSpec` lowers once into the existing private process launch manifest. The manifest also carries the complete original Computer specification and configured runtime paths. The helper validates the lowering and sealed runtime. The journal rechecks runtime identity before durable release. No separate execution service, lifecycle writer, or owner-supplied executable convention was added.

The runtime contains Node 24.22.1, Playwright 1.64.1, and its matching Chromium 156.0.8078.4 revision 1248. Setup verifies the official Node archive digest and the locked npm package integrity. It seals a sorted SHA-256 file inventory. Startup and launch verify that inventory, the embedded runner, and the package lock. The real browser probe uses a separate disposable probe profile. Execution uses one persistent automation profile under worker state. Runtime setup installs system browser dependencies. Execution performs no runtime download.

Systemd clears `ControlGroup` after its own runtime limit while retaining the original `InvocationID`. The supervisor now accepts that specific stopped state only on the recorded host and boot, with the original invocation, an inactive or failed unit, and a positively empty or removed recorded cgroup. A missing unit or changed invocation still cannot prove stopping. The proof also records systemd timeout or resource termination, so a helper result written during timeout cleanup remains a limit failure. This is a necessary lifecycle correction found by real Chromium hang acceptance. It does not widen any resource ceiling.

Browser storage support includes cleanly closed cookies, local storage, and tested IndexedDB. The two-VM fixture reads them from another bot and again after an idle worker restart. Open tabs, session storage, and unflushed crash-time changes are unsupported. The worker never deletes or resets the shared profile. It refuses malformed profile JSON or corrupt cookie databases. Profile corruption is an explicit capability failure until the operator repairs it. Same-account processes retain their real filesystem and setup privileges. Workspaces do not restrict approved absolute guest paths.

Each declared output has a durable `OutputView` independent of `TaskState`. Output states are pending, transferring, available, and unavailable. Worker offers and snapshots live in the existing journal and separate spool files. The manager reserves opaque artifact identities and output bytes in the same SQLite transaction that consumes approval. Transfer authority comes from that admitted scope and does not expire with launch approval. Current worker identity and journal must still match.

The worker reserves all declared maximum bytes before a claim. An unadmitted denial or cancellation releases that reservation. After positive stopping proof, sealing replaces each maximum reservation with the actual snapshot length, or zero for an unavailable output. The worker retains snapshots until the manager durably acknowledges stored bytes or an unavailable result. Received snapshots are deleted. Unknown stopping keeps the execution reservation and cannot release replacement work. Produced output files are removed only after every declared slot has a persisted sealed offer. An active execution can temporarily hold its bounded produced outputs while the separate spool holds snapshots.

The manager reserves declared maximum bytes before consuming authority. An immutable offer reduces the reservation to its actual length, or zero for a missing or rejected output. Available bytes remain charged to the store. Corrupt or partial retained files remain charged until a future explicit retention operation. Artifact records remain durable for owner retrieval and have a hard bound of 512. The existing task bound is 128. The implementation offers no automatic pruning or configurable larger store.

Sequential chunks carry at most 16 KiB of raw data as hexadecimal within the existing 64 KiB envelope. Only one chunk is outstanding. Chunk bytes are fsynced before their SQLite offset and acknowledgment. Exact duplicates return durable progress. Startup truncates unacknowledged partial suffixes, verifies renamed final files, and reconciles their availability transaction. Missing worker snapshots produce an explicit durable unavailable result. None of these paths re-executes work.

Owner download uses the existing authenticated owner session and immutable task ownership. It verifies length and SHA-256 before serving a bounded attachment stream. Restart requires a new owner login, as before. Partial, corrupt, missing, and unrelated artifacts are refused.

The rejected alternatives remain opaque executable conventions, per-bot browser profiles, a second lifecycle writer, arbitrary page evaluation, and a new graceful-control protocol. No substantive product-scope deviation was needed. Independent source review and refreshed acceptance are recorded in [the completion review](w01-completion-review.md).
