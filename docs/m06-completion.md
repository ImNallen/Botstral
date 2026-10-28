# M06 completion evidence

M06 implements reusable owner connections, independent bot grants, recoverable manager and Computer state, and separate Linux packages. The table maps every M06 requirement in `PLAN.md` to implementation and observed behavior.

| Requirement | Evidence |
| --- | --- |
| Install connections once for the owner. | Owner HTTP and WSS operations persist one connection independently of bot grants. See [connector administration](m06-connectors.md#owner-api) and [live acceptance](m06-integrated-acceptance.md#observed-gates). |
| Discover, authenticate, reconnect, rotate, and revoke. | Actual manager API checks and authority tests cover protected entry, revision changes, rediscovery, locked restart, and terminal revocation. Live HTTPS rejects absent and incorrect bearer credentials. |
| Remove revoked tools and reject cached bot and Vibe calls. | The [live report](evidence/m06-integrated-84b6d2fb/report.json) records stale grant and connection rejection. Authority tests cover cached catalogs, approvals, and Vibe policy bindings. |
| Protect remote tokens and restrict local credential release. | Encrypted, identity-bound manager storage supplies remote calls. One-shot local permits release only the selected connection's named secrets. Logs, public views, and ledger results are redacted. See [connector execution](m06-connectors.md#execution-and-recovery). |
| Separate account authorization from bot tool permissions. | Two-bot tests use one connection with independent grant revisions. Live acceptance proves that an ungranted second bot cannot use the connected account. Shared Computer credentials remain a documented shared-machine boundary. |
| Support Streamable HTTP and local stdio. | Live authenticated HTTPS and actual Vibe stdio use the official SDK. A separate pinned official filesystem-server check observes a file effect. These are distinct proofs. |
| Run local plugins inside the Computer VM. | Independent process and cgroup observations place the wrapper, child, and descendant in the existing task scope. Cancellation empties that scope. |
| Record discovery, permission decisions, calls, and results. | Durable audit and effect records survive restart and restore. Unknown effects require attention and are never blindly replayed. [Authority regressions](evidence/m06-parent/review-authority-after.log) cover duplicate and late evidence. |
| Expose permitted tools to Vibe through the manager. | Actual Vibe 2.24.2 completes a local call through the production gateway. Journal counts prove that it does not submit a nested worker job. |
| Define in-flight revocation. | Revocation stops further authority and retains uncertain external effects. Late results do not resume the parent. Tests and the live stale-call gate verify this [documented policy](m06-connectors.md#execution-and-recovery). |
| Install, diagnose, back up, and restore consistent manager state. | Real manager restoration preserves owner identity, history, Markdown, encrypted credentials, and artifact bytes. Startup is locked. Original-passphrase unlock permits a new authenticated connector effect without credential reentry. |
| Define and restore Computer backup contents. | [Recovery policy](m06-recovery.md#select-computer-recovery-roots) selects guest roots explicitly and requires original-path restoration. Worker tests restore selected bytes, executable modes, identity, and the pinned connector interpreter. |
| Package separate Linux services and OCI images. | Native installation stages distinct service types and refuses combined roots. Separate Docker images start production binaries. The worker acceptance exercises the production systemd supervisor and actual journal restoration. |
| Verify Proxmox and another Linux environment. | [Proxmox staging](evidence/m06-parent/proxmox-packaging/report.json) runs on distinct Ubuntu x86_64 guests. Separate Docker Linux arm64 acceptance exercises manager startup, worker supervision, and recovery. |

## Review and verification

`RUST_TEST_THREADS=2 make verify` and `RUST_TEST_THREADS=2 make verify-desktop` passed after the review corrections. The [check manifest](evidence/m06-parent/review-checks.json) records commands, source identities, and retained log digests. Both [review-corrected images](evidence/m06-parent/review-images.json) passed Docker acceptance. Actual updated-manager [local](evidence/m06-parent/review-owner-local.json) and [owner WSS](evidence/m06-parent/review-owner-wss.json) administration checks passed.

[Independent review and corrections](m06-review.md) preserves all three original verdicts, four reproduced defects, focused corrections, and failed-before evidence. The reviewers examined the integrated source before the coordinator corrections. Their original reports are not approvals of later commits.

[Desktop proof](m06-desktop-proof.md) contains before and after renderer captures from actual native serialized approval data. It verifies the new static connector action description. It does not claim a connector-management graphical interface.

The canonical [decision trail](m06-decisions.tsv) records source and evidence identities. [Integrated Linux acceptance](m06-integrated-acceptance.md) and [recovery acceptance](m06-recovery-acceptance.md) retain the live commands, immutable source records, cleanup receipts, and earlier failed attempts.

## Boundaries

Remote authentication supports bearer credentials. Local servers receive explicit named environment secrets. OAuth browser flows remain outside this implementation. MCP schemas use a bounded subset with local JSON Pointer references. Recursive or scope-changing references are refused. Tool and combined provider-request sizes remain bounded.

The live integrated run restores the manager and restarts the retained original worker. Separate worker recovery proves original-path restoration. Neither record claims a replacement-machine cutover or atomic multi-filesystem publication. Retire the old worker before starting its restored identity.

Proxmox installation is isolated staging. Existing installed services, real owner state, system trust stores, and shared runtimes were not replaced. Docker arm64 proves Process supervision and recovery. Computer browser automation remains Linux x86_64. Podman and Docker x86_64 were not exercised. No paid inference ran.

M06 does not close P03, W02, C03, mobile work, the first acceptance milestone, or release readiness. Those retain their separate checklists.
