# Independent final review

Reviewed `1af15d0577cec96940068cc53a1041fd57f96ebd` against the selected contract and prior reviews. No application edits or subagents. Verdict: changes required for the remaining public-view protection defect and the failed required verification gate.

## 1. Public run protection still leaves two known-secret channels

`manager/src/manager/state/runs.rs::public_run` now correctly refuses unsafe requests, including execution policy fields, and refuses requests when the Vault is locked. The workspace fallback preserves only typed summaries. However:

- Archived-turn redaction leaves `ArchivedRunTurn.tool_call_id` unchanged. This is an arbitrary provider string copied by `routines/reconcile.rs::compact_run_transcripts`. A formerly ordinary call identifier that becomes a known key remains visible even when the rest of that archived turn is withheld.
- `RunView.configuration` is never scanned. Its model and acknowledgment fields contain strings. A formerly ordinary acknowledgment value that becomes a known key remains visible through the full run endpoint.

Both were reproduced against this checkout's `target/debug/botstral-manager`, through authenticated loopback HTTP, using synthetic credentials and seeded historical rows. The probes reused the setup from `review/probe_locked_views.py` inline, changed its binary path to this checkout, and used a safe request prompt. They did not perform paid inference or modify application files.

For the archived case, insert a `run_turn_receipts` row with `tool_call_id` equal to the installed synthetic provider key and otherwise safe fields, then GET the run. Actual output:

```json
{"probe":"archived identifier becomes protected after rotation","tool_call_id_exposed":true,"text":"Protected content withheld."}
```

For the configuration case, seed the historical run with `configuration.policy.acknowledgment.model_developer` equal to that key, then GET the run. Actual output:

```json
{"probe":"historical inference acknowledgment becomes protected","configuration_exposed":true}
```

These probes model historical values that become sensitive after rotation; they do not claim normal admission currently accepts known secrets. Clear the archived identifier on withholding and guard the configuration before returning a full run. Add focused rotation regressions, including the summary-only workspace behavior. The parent was notified of both results.

## 2. Required repository verification fails

The aggregate `parent/final-verification.log` now records `repository: exit 101`. Its repository log reports 132 manager tests passed, four failed, and five ignored. The failures are:

- `m04_run_capacity_backpressure_and_bounded_shutdown`
- `m04_run_late_usage_cancellation_and_crash`
- `m04_run_truncated_final_is_stopped_and_persisted`
- `m04_run_recorded_browser_file_approval_result_artifact_and_followup`

The inspected recorded-browser test restarts the manager and immediately expects a full run before unlocking. The new boundary correctly returns `vault_locked`. Update affected restart expectations to assert locked refusal and unlock before inspecting full content, then rerun the required gate. This is verification debt from the intentional boundary change, not evidence that returning locked content should be restored.

No final desktop pass is inferred from its existing log. At review time the aggregate has no completed desktop result, and the visible desktop log still refers to the implementation checkout. Acceptance must use the final run's completed results.

## Corrections independently checked

The publication recovery change rechecks both expected and intended digests even after attention. It still preserves unexpected files, does not resend the provider, and commits the same receipt and event transactionally. The added regression exercises both repair paths across reopening, checks one revision and publication event, and checks unchanged conflicts do not add events. Its completed focused log reports 25 memory tests passed.

Routine overlap now includes `waiting_for_memory`. The failing-before overlap log and completed 15-test routine log substantiate the correction. The source retains exact native submit-request comparison against a separately fetched full public `RunView`; nullable workspace summaries do not weaken reconciliation.

Legacy import now explicitly requires an unlocked Vault and still scans imported documents and intended bytes. The two reported comments are removed. Current local API documentation correctly rejects obsolete Memory document writes. Visible notices remove storage identifiers, revisions, and milestone jargon while retaining saved content and scope.

## Parent trail and evidence limits

The separate parent trail explicitly records that the imported trail replaced two earlier parent rows. It links preserved design and judgment artifacts. That correction is honest; it does not reconstruct the lost rows or make the complete historical trail append-only. The current acceptance text should not imply otherwise.

The parent trail's three failing-before logs contain actual assertion failures, and the passing focused logs substantiate the 25 memory and 15 routine counts. The design score tables are judgments about proposals, not execution evidence.

Acceptance still names the earlier implementation head, says parent review is pending, and claims successful full gates while the repository log has now been overwritten by a failing final run. Reconcile the acceptance record, exact ignored/skipped test boundaries, capture metadata, and final commit after corrections. Keep component replay, actual native/manager captures, and separate no-worker tests distinct. No packaged application, real keychain, Linux execution, or paid inference is established by this review.

The recovery and overlap corrections have no further proven blocker in the inspected code. The public-view findings above prevent a clean verdict on the final secret-exclusion claim.
