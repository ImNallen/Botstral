# Independent correctness review

Reviewed `dd7f9e5` against `41aec24`. The reviewer did not implement the change and made no application edits. The user needs conversation-based individual and shared memory, durable truthful outcomes, no memory editor, manager-only chat, and value-free Vault handoff.

Verdict at the reviewed revision is changes required. Two defects need correction. The entry model, publication intent ordering, immutable snapshot fallback, scoped access, no-worker policy, typed outcomes, and honest Vault limitation otherwise match the selected contract in the inspected source and evidence.

## 1. Protected run views fail open or redact only part of the request

Severity is high because a known stored secret can cross back into ordinary public run responses.

At the reviewed revision, `manager/src/manager/state/runs.rs:212` redacts only `request.prompt` and only when `reject_known_secrets(&view.request)` returns `InvalidInput`. A locked Vault returns `Locked`, so the historical prompt is returned unchanged. The same behavior affects GET, identical submit retries, and cancel responses because they all use `public_run`. Even while unlocked, a protected value in another request field remains present after the prompt alone is replaced. `RunExecutionPolicy.process.environment` is one reachable example. A formerly ordinary environment value can become a known secret after credential rotation.

The live and archived turn guards at lines 222 and 233 also fail open for inventory errors unless the turn has a memory-specific marker. The full request and all unscannable turn content need a consistent boundary.

The actual built manager was exercised over its authenticated loopback HTTP API. The probe starts an isolated synthetic installation, installs a synthetic provider key, seeds completed historical run rows representing content that is now sensitive, and calls the public run endpoints. It does not run provider inference or use real credentials. The result was:

- Unlocked GET withheld a prompt equal to the provider key.
- Unlocked GET still exposed that key in `execution_policy.process.environment.APP_MODE`.
- After Vault lock, GET, identical POST retry, and cancel each exposed the key as the prompt.

The reproducible probe is `review/probe_locked_views.py`. Its output is `review/locked-views.json`, under this directory. It uses the manager binary produced by the implementation checkout's verification. Parent should retain focused regression tests against the final compiled code rather than treat this probe as final verification.

The smallest strong correction is to refuse the full public request when inventory validation fails, rather than fabricate a partially sanitized `SubmitRun`. Parent's proposed `WorkspaceRun.request: Option<SubmitRun>` preserves workspace availability with a withheld summary-only projection. Normal runs carry `Some(request)` and exact native reconciliation requires `Some(expected)`. Only explicit protected-content and locked-inventory errors should become that placeholder. Other errors should propagate. Withhold live and archived content whenever its inventory guard cannot succeed.

## 2. One publication attention state permanently stops all dependent work

Severity is medium because a recoverable file or transient storage problem becomes a permanent installation-wide outage for run admission and dispatch.

`manager/src/manager/state/memory/publication.rs:97` returns `Conflict` immediately for every journal row whose state is `attention`. That happens before examining the current canonical file, expected digest, intended bytes, or secret inventory. Lines 119 through 133 put a pending operation into attention after any non-locked publication failure, including an unexpected external file or a transient I/O error. No implementation path returns that journal row to pending or otherwise resolves it.

The existing `admitted_publication_survives_cancellation_but_never_overwrites_an_unexpected_digest` test demonstrates the first half of the path. A committed create intent encounters an unexpected canonical file, preserves it, marks the run for memory-publication attention, blocks new memory context and admissions, and prevents model work. The missing recovery step is deterministic from the early branch. Remove the conflicting file to restore the exact expected prior state, or replace it with the exact intended bytes. Every subsequent recovery still returns at line 97 without checking those repaired bytes. Restart retains the same journal state.

The impact is wider than the affected entry. `memory_context` always invokes recovery, and `claim_run_work` at `state/runs.rs:608` returns no work for this conflict before considering any run. The selected contract says matching intended bytes finalize the receipt and matching prior bytes permit publication. The unconditional attention branch violates that contract after a reliable observation becomes available. Existing generic attention limitations do not justify disregarding these exact digest observations.

The smallest correction is to re-evaluate attention rows under the same expected/intended-digest and secret checks, finishing only the already admitted operation when its state becomes provable again. Continue blocking while the mismatch remains. Preserve cancellation semantics and do not invent a new operation or resend the provider. Extend the existing crash test to restore both expected-prior and intended states and prove one revision, one receipt, and resumed admission/dispatch.

## Additional integration correction

The parent identified a separate active-phase omission in `state/routines/reconcile.rs:29`. The overlap query excludes `waiting_for_memory`. A later admission can recover the earlier publication to Ready after the overlap check has already passed. Include the phase in the query and retain the parent's regression test. This is attributed to the parent, not a new reviewer finding.

The current `docs/local-api.md` reference still describes owner and generated Memory document writes as working methods. These writes are intentionally rejected now. Update the current reference and link the conversational tool contract. Historical P01 evidence can remain unchanged.

## Review limits

This review inspected the selected design, public types, storage and recovery implementation, run settlement and dispatch, secret inventory and public projections, native/editor cutover, scoped tests, and the acceptance evidence. It ran the isolated HTTP projection probe above. It did not run Linux workers, paid inference, a packaged Tauri session, or repeat both full verification scripts. Parent is replaying focused checks independently. The two corrections and parent overlap fix still need final tests and reconciliation before a clean verdict.
