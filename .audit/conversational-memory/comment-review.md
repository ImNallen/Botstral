# Comment review

Reviewed source changes in `41aec24..dd7f9e5`. This review changes no application code or comments. Two added comments should be deleted. There are no new lint or TypeScript suppressions, public-contract comments, external-rule claims, or HOLD findings.

## Findings

1. Delete `desktop/src-tauri/src/remote.rs:955`. The comment explains the local policy already implemented by the `ClientError::Refused` and `OwnerOperation::SubmitRun` branches followed by `self.intents.pop()`. It does not describe an external constraint. No application-code reshape is needed for this comment deletion.
2. Delete `manager/src/manager/state/memory/entries.rs:337`. The comment explains a precondition owned by this code. `import_legacy_memory` expresses that precondition through `reject_known_secrets(&())`, which serializes a unit value solely to reach provider/connector secret loading and its locked-Vault error. MUST KILL `State::import_legacy_memory`'s unit-value validation call. Replace the indirect precondition with an explicit unlocked-Vault or secret-inventory operation, preserving locked import refusal. This is a naming and ownership finding, not a reported correctness failure. `state/inference.rs::provider_secret` checks unlocked state before loading the key, so the existing call does enforce the stated precondition.

## Scope and outcome

The scan covered added comments and suppressions in changed application code, tests, proof scripts, and HTML. Captured logs, JSON/schema data, generated bindings, Markdown evidence, and deleted editor files were excluded from comment judgment. URL strings, metadata sentinels, Rust attributes, CSS colors, and the verification script's shebang are not explanatory comments.

Actual deletion count is zero because the task is report-only. Recommended deletion count is two. Restored comments, reruns, HOLD comments, external-rule kills, and constraint-encoding offers are zero. The unit-value precondition is the sole reshape flag.

## Parent disposition

The parent removed both comments and replaced the unit-value validation with an explicit unlocked-Vault precondition. Two comments were deleted. No comments were restored, no HOLD or suppression findings remain, and no constraint encoding requires an operator decision. Final source review approved the resulting implementation.
