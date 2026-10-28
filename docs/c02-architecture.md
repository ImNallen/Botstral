# C02 desktop architecture

The desktop has two native authorities. `LocalManager` owns trusted loopback administration. `RemoteService` owns one paired actor, one shared owner client, one socket, complete workspace recovery, and a durable mutation journal. Remote tasks never use local HTTP.

`link/client` contains the extracted M05 transport. An opaque zeroized `SecretStore` record holds credential, invitation, manager trust, and enrollment marker atomically. The optional `probe-files` adapter preserves the probe's `device.json` and `state.json`. The desktop does not enable that feature. It uses macOS Keychain or Linux Secret Service without a file fallback. Windows acceptance is outside this implementation's claim.

Owner version 1, `OwnerSnapshot`, and `Resume` are unchanged. The additive `Workspace` read builds roster, sessions, durable run details, existing snapshot, and application cursor in one manager queue operation. Workspace admission checks counts before allocation. Its serialized and allocation bounds are tested against the existing 80 MiB transfer ceiling and aggregate recovery budget. Existing bot and worker admission behavior remains compatible.

The additive `GrantReviewedApproval` checks the exact immutable `CommandBody`, complete prior approval state, and permission revision before invoking existing grant logic in the same serialized manager operation. The manager creates the execution binding. The client does not claim its own read and write are atomic.

The actor serializes mutation calls and saves stable identities and canonical requests before transmission. It does not replay uncertain calls. Recovery compares saved identities and payloads with complete manager state. Acknowledged cancellation remains pending until a terminal authoritative result. A completed task wins a cancellation race. Unknown operations remain blocked until authoritative reconciliation or an explicit owner choice to retain an attention state. Continuing does not replay the saved mutation.

The actor's idle signal wait can be abandoned only by dropping its socket. No partially consumed socket is reused. Authentication and complete workspace recovery precede new mutation admission. Event signals trigger complete recovery. Native notifications report activity hints only. Reconnect uses capped jittered backoff. Explicit pause and resume commands and a native wall-clock sleep-gap detector discard the old session and recover before actions. Hiding the window keeps the activity listener running for notifications.

Sign-out clears both local administration and paired renderer views immediately. Both native sign-out commands clear local session authority and the paired credential lifecycle. The paired view advances its generation before the actor stops, and admission stays closed during cleanup. A public durable tombstone prevents startup authentication even if native credential deletion fails. Packaged origins, main-window checks, popup rejection, navigation policy, and capabilities apply to every command. Invitation import and artifact export use native choosers. Renderer arguments never contain file paths, device credentials, or invitations. A native file chooser imports protected invitations and authenticated trust replacements. Bounded pairing links stage candidates without navigation or fetching. Every trust change requires public confirmation.

Each composer submission is a separate durable M04 run. The renderer receives projected user prompts, actual assistant turns, linked task identities, exact normalized task actions, and verified artifact declarations. It does not invent conversation context between runs. Voice and attachments remain unavailable.

Native and graphical acceptance boundaries are recorded in [c02-acceptance.md](c02-acceptance.md). Full milestone completion requires the parent's independent review and packaged remote-route walkthrough.


The native journal has a separate 1 MiB reader and at most 128 admitted records. It does not use the 64 KiB secret-file reader. Staging opens reject symlinks, hard links, wrong ownership, and wrong permissions before truncation. Complete workspace images retain their installed cursor across process restart.

The first unit's approval operation compared the approval identity. Implementation tightened this to the complete prior `ApprovalView` after tracing a revoke-between-review-and-grant race. The actual-manager contract proves changed arguments, changed permission revision, and intervening approval revocation all refuse the stale grant.

The macOS debug build supports an explicit `BOTSTRAL_DESKTOP_PROFILE` directory for isolated native acceptance. Release builds always use the application's profile directory. Neither build has a file-backed credential fallback.
