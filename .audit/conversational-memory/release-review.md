# Independent release source review

Reviewed `bace80bed72fe5aca55e9f258ab68827178bb1d2`, its changes since `1af15d0`, the four preceding reviews, selected design, and parent decision trail. No application edits, subagents, or new HTTP probes. Only this report was written.

**Source verdict: approved within the documented feature boundaries. No remaining correctness or security blocker was proven. Release verification remains pending.** The incomplete aggregate verification log is not a source defect and is not a completed passing gate.

## Public protection and regressions

`State::public_run` now validates the entire serialized `RunView`. GET, identical submit retries, cancellation replies, and initial submit replies all use that boundary. This closes the two reproduced omissions: configuration strings and archived `tool_call_id`. Inventory errors propagate instead of permitting partial disclosure. Admission independently checks configuration before committing a new run.

`workspace_run` converts only protected-content and locked-inventory refusals to the typed summary with no request or turns. Other errors propagate. The summary contains identifiers, bounded state enums, counters, and timestamps rather than conversational strings. Native reconciliation still fetches the full run and compares the exact submitted request; an unavailable full view cannot falsely confirm a submission.

The new configuration test rotates a real admitted configuration acknowledgment into the provider key. The archived test uses an actual settled memory call, compacts it, then rotates its identifier into the key. Neither needs fabricated historical rows. Both now require `InvalidInput`. The retained `configuration-before.log` and `archived-before.log` show the precise missing-refusal failures. `manager-final.log` records both passing and the complete manager library result: 137 passed, 0 failed, 5 ignored.

The request protection regression covers prompt and execution-environment rotation, summary-only workspace results, locked and unlocked inventory, GET, retry, and cancel. The two new rotation cases directly assert GET refusal; their workspace coverage follows the same source boundary rather than separate assertions for each field. This is an evidence distinction, not a demonstrated defect.

All four changed restart tests first assert HTTP 409 `vault_locked`, explicitly unlock, and retain their recovery/content assertions. The change does not restore locked access or weaken the persisted-content checks.

## Publication and original design

The publication path still commits the response, accounting, and durable intent before file publication, withholding the success receipt until publication is durable. Recovery verifies operation identity, secret inventory, and expected or intended bytes, then commits the same receipt and publication event. Attention rows are re-evaluated after repair; unexplained bytes remain protected. Cancellation after admission preserves the committed operation and receipt. The earlier recovery fix matches the original expected/intended-digest contract.

Scoped CRUD resolves individual ownership from the admitted run, checks revisions, preserves unrelated entries, and bounds snapshots. Frozen context and outbound provider bodies are checked against the current secret inventory. Response and request identifiers are screened before settlement; the complete public-view guard also protects historical content that becomes sensitive later. The routine overlap fix includes `waiting_for_memory`.

Manager-only admission retains the no-session path only when worker policies and images do not require a session. The Vault proposal and outcome remain closed, value-free types; reusable login storage is explicitly unsupported. Durable typed outcomes remain separate from proposed arguments. These fixes preserve the selected conversational-entry design rather than reintroducing an editor or inventing credential storage.

## Evidence reconciliation still required

The parent trail honestly records that imported history replaced two earlier rows and points to the surviving design and judgment artifacts. It does not recover the lost rows. Acceptance must not describe the complete historical trail as append-only.

At inspection, `docs/evidence/conversational-memory/acceptance.md` still names the implementation checkout and old head, says parent integration is pending, and calls earlier gate logs final. Replace those statements with the final integrated revision and completed gate results after the current run finishes. Retain the first failed repository run and explain that its four failures were outdated locked-history expectations.

Name or link exact ignored and skipped tests. The current manager log identifies five environment-dependent cases: pinned Vibe production, M06 isolated VMs, C02 graphical fixture, production browser/file/process worker, and C02 two-VM production fixture. Do not infer execution of these from the 137 passing tests. Reconcile the frontend opt-in skip and worker test exclusions from the final logs as well.

Regenerated captures and video must have matching final metadata and hashes. Keep the component replay, actual native/manager captures, and separate no-worker tests distinct. The captured native workspace contains a fixture worker; that recording alone does not establish no-worker admission.

Known-secret exclusion covers the current provider key and retained connector epochs, not arbitrary unknown text, arbitrary encodings, or overwritten provider keys. It does not erase historical files or backups. No packaged application session, real keychain exercise, Linux worker execution, or paid inference is established by this review. Full repository and desktop results must be reconciled separately before final acceptance.
