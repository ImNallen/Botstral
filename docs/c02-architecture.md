# C02 desktop architecture

The desktop has two native authorities. `LocalManager` owns trusted loopback administration. `RemoteService` owns one paired actor, one shared owner client, one socket, complete workspace recovery, and a durable mutation journal. Remote tasks never use local HTTP.

`link/client` contains the extracted M05 transport. An opaque zeroized `SecretStore` record holds credential, invitation, manager trust, and enrollment marker atomically. The optional `probe-files` adapter preserves the probe's `device.json` and `state.json`. The desktop does not enable that feature. It uses macOS Keychain or Linux Secret Service without a file fallback. Windows acceptance is outside this implementation's claim.

Owner version 1, `OwnerSnapshot`, and `Resume` are unchanged. The additive `Workspace` read builds roster, sessions, durable run details, existing snapshot, and application cursor in one manager queue operation. Workspace admission checks counts before allocation. Its serialized and allocation bounds are tested against the existing 80 MiB transfer ceiling and aggregate recovery budget. Existing bot and worker admission behavior remains compatible.

The additive `GrantReviewedApproval` checks the exact immutable `CommandBody`, approval identity, and permission revision before invoking existing grant logic in the same serialized manager operation. The manager creates the execution binding. The client does not claim its own read and write are atomic.

The actor serializes mutation calls and saves stable identities and canonical requests before transmission. It does not replay uncertain calls. Recovery compares saved identities and payloads with complete manager state. Acknowledged cancellation remains pending until a terminal authoritative result. A completed task wins a cancellation race.

The actor's idle signal wait can be abandoned only by dropping its socket. No partially consumed socket is reused. Authentication and complete workspace recovery precede new mutation admission. Event signals trigger complete recovery. Native notifications report activity hints only.

Sign-out clears the protected view in a new generation before aborting the actor. A public durable tombstone prevents startup authentication even if native credential deletion fails. Packaged origins, main-window checks, popup rejection, navigation policy, and capabilities apply to every command. Invitation import and artifact export use native choosers. Renderer arguments never contain file paths, device credentials, or invitations.

Each composer submission is a separate durable M04 run. The renderer receives projected user prompts, actual assistant turns, linked task identities, exact normalized task actions, and verified artifact declarations. It does not invent conversation context between runs. Voice and attachments remain unavailable.

Native and graphical acceptance boundaries are recorded in [c02-acceptance.md](c02-acceptance.md). Full milestone completion requires the parent's independent review and packaged remote-route walkthrough.
