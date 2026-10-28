# M06 implementation workflow

Implement owner-installed connectors, manager-enforced bot permissions, Vibe access, recoverable installation state, and separate Linux packaging.

## Completion predicate

M06 is complete when the production manager and worker demonstrate all of the following.

- One owner installs a connection once. Separate bots receive different tool permissions for that account.
- Remote Streamable HTTP calls execute through the manager. Local standard-input and standard-output servers execute inside the Computer VM.
- Discovery, authentication, reconnect, credential replacement, permission changes, and revocation work with bounded resources.
- Bot and Vibe requests revalidate current authority before dispatch. Cached requests cannot bypass revoked connections or grants.
- Durable call history distinguishes refusal, confirmed completion, and uncertain external effects. Restart never blindly repeats a possibly transmitted call.
- An in-flight revocation has a documented and tested outcome. Aborting transport never claims to undo an external effect.
- Credentials stay out of model inputs and ordinary audit records. Remote account credentials remain on the manager.
- A consistent backup restores history, Markdown memory, encrypted credentials, and verified artifact contents. The documented Computer VM recovery policy is exercised.
- Separate manager and worker services and OCI images preserve the two-VM boundary. Installation is verified on the existing Proxmox Linux guests and another compatible Linux environment.

Implementation and acceptance are separate. Missing external proof leaves the corresponding predicate open.

## Evidence and rigor

Use high rigor for authority, external effects, and restoration. Capture actual-binary behavior, deterministic failure cases, independent review, and source identities. Use isolated temporary owner state and synthetic credentials for automated checks. Use a real standards-compliant connector for milestone acceptance.

The initial checkout is `4483b183671aec5bc36a65fa11c25308fb127423`. Its workspace tests passed before implementation. The baseline log is `/tmp/botstral-m06-baseline.log`.

## Ordered units

1. Ground the current manager, worker, Vibe, and persistent-state ownership. Compare independent architecture candidates and save the selected contract.
2. Implement connection records, protected credentials, revisioned bot grants, discovery, bounded transports, durable call admission, and recovery. Verify a remote connector end to end.
3. Integrate worker-local servers and the Vibe gateway with the same authority and call history. Verify stale requests, worker disconnect, cancellation, and ambiguous external effects.
4. Implement offline backup, restoration, diagnostics, separate Linux services, and OCI images. Verify actual restored state and refuse unsafe or concurrent operations.
5. Integrate the units serially. Run workspace, schema, desktop compatibility, Linux installation, and actual connector acceptance checks. Resolve independent correctness and maintainability findings.
6. Update acceptance records and milestone checkboxes only for observed gates. Open a reviewable pull request without merging.

## Throughput checkpoint

- Blocking first steps are current-code grounding, design comparison, and a frozen connector authority contract.
- The connector implementation has one owner because manager authority, durable calls, worker dispatch, and Vibe admission are coupled. Recovery and packaging are a separate workstream.
- Implementation owners use separate worktrees. Completed commits integrate serially. The coordinator owns the canonical decision trail and acceptance record.
- One connector owner and one recovery owner are the smallest safe decomposition. Verification uses fresh independent reviewers after integration.

## Decision trail

The canonical trail is [m06-decisions.tsv](m06-decisions.tsv). Each unit records its hypothesis, source evidence, verification result, and remaining limitations.
