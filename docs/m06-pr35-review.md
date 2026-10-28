# M06 pull request review follow-up

The coordinator babysits [pull request 35](https://github.com/ImNallen/Botstral/pull/35) without merge authority. GitHub reports no merge conflict. The GitHub CLI is the selected forge because Origin is unavailable. The thread inventory contains no other babysitter for this pull request.

Both automated review findings at `e8b2e49c040d251b5af347e0a93c84c2fd020f7b` are classified as fixes. [Issue 36](https://github.com/ImNallen/Botstral/issues/36) tracks them. No finding was dismissed.

| Finding | Executed failure | Correction |
| --- | --- | --- |
| Redaction corrupts result envelopes. | A real manager State discovery settlement with credential `tool` returns `Internal`. Real stdio calls with credentials `type`, `tool`, and `value` lose fields or replace the result discriminator. [Manager failure](evidence/m06-pr35/manager-before.log) and [stdio failure](evidence/m06-pr35/stdio-before.log). | Redaction matches the typed catalog or tool result. It redacts variable catalog fields and untrusted payloads without serializing and modifying the enclosing enum. Python preserves the same envelope. |
| Recorded unknown outcomes remain pending. | The existing acknowledgment predicate was moved unchanged into `Settlement`. A captured unknown result with a durably resolved unknown manager response returns `connector settlement pending`. [Failure](evidence/m06-pr35/recovery-before.log). | The matching call, task, and execution can acknowledge captured non-success evidence after durable resolution. The manager's unknown classification remains unchanged. Reserved or dispatch-possible calls cannot acknowledge. Captured finished results still wait for final or late-result evidence. |

The passing recovery regression also rejects wrong invocation, task, and execution identities. It observes the acknowledgment file and removal from the pending evidence set. It does not change the call's unknown state. The reproduction proves the persistent pending record. It does not claim a new 33-item live scheduler run.

## Validation

The [report](evidence/m06-pr35/report.json) retains failure and success exits and log digests.

- The [manager connector tests](evidence/m06-pr35/manager-after.log) pass all 21 cases.
- The [worker library tests](evidence/m06-pr35/agent-after.log) and [feature-enabled worker tests](evidence/m06-pr35/feature-agent.log) each pass all nine cases.
- The [real stdio contract](evidence/m06-pr35/stdio-after.log) passes all seven cases, including credential words matching envelope fields.
- Workspace Clippy passes for [default](evidence/m06-pr35/clippy.log) and [feature-enabled](evidence/m06-pr35/feature-clippy.log) targets.
- Actual Vibe 2.24.2 passes the [tool contract](evidence/m06-pr35/vibe-contract.json) and [local stdio relay checks](evidence/m06-pr35/vibe-local.json). No paid inference runs. These macOS checks do not replace the earlier live Linux scope evidence.

The helper bytes changed. Existing connector runtime pins therefore require regeneration through the documented `agent/connector/configure.py` command and fresh approved policies when deploying this update. The task does not modify existing installations.

Fix Root Causes keeps redaction inside the typed result boundary and acknowledgment inside the captured settlement boundary. Prove It Works requires failing-before tests and actual pending-file retirement. The earlier milestone evidence keeps its original source identities. This follow-up does not claim another broad deployment acceptance or independent review of the correction.
