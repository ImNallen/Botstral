# P05 independent review

Owners need to find older generated files, inspect their source tasks, and export identical bytes after restart or restoration. The implementation meets the storage and authority contract. One visible-flow defect must be fixed before acceptance.

Reviewed clean frozen HEAD `aa34dab8070806bf8fc768dd8c5e9b67beb8933e` in `/Users/nallen/.t3/worktrees/Botstral/t3-6ce83c69`, including every production change from `271f9cd72dabd2d0749027b84d355ca046de5f7e`, the three commits, relevant tests, fixture changes, selected design, committed architecture, and evidence. No production or committed test files were edited. No remote access, external models, spending, pull requests, merges, or subagents were used.

## Finding requiring correction

### P2. Render export feedback inside the saved-results modal.

`desktop/src/SavedResults.tsx:7` reads `snapshot`, `library`, `source`, and `busy`, but never renders the controller's `notice` or `issue`. Export success, cancellation, and local save failures are rendered only by `desktop/src/PairedWorkspace.tsx:575` behind the open modal and its blurred backdrop. Closing the modal calls `invalidateLibrary` at `desktop/src/paired-application.ts:97`, which immediately deletes both messages. A user cannot reliably tell whether the export succeeded or failed from the open library.

I reproduced this against the actual frozen components using their existing native-response replay. The isolated probe opens the library, selects the old result, completes the simulated chooser, and checks the location of the resulting message. There is one success message in the document and zero inside the still-modal library. Closing the library removes that message entirely. The screenshot shows the only message blurred behind the dialog.

Reproduction from the frozen checkout:

```sh
node /Users/nallen/.t3/worktrees/Botstral/t3-9cf8cc22/.audit/p05/reviewer-probes/save-feedback.mjs agent/runtime/node_modules/playwright/index.mjs
```

The intended assertion fails with `Save completion must render inside the open Saved results dialog.` and `0 !== 1`. The child exits 1. Evidence is in `reviewer-probes/save-feedback.log`, `reviewer-probes/save-feedback.json`, and `reviewer-probes/save-feedback.png`, beside this report. The fixture is replay only and writes no exported file. The existing committed proof checked `page.getByText(...)`, which passes for the obscured background message and therefore missed this defect.

Render export feedback in `SavedResults` and avoid duplicate background feedback while it is open. Scope the interaction proof to the dialog. Check success, cancellation, and `save_failed` feedback. The same message location problem applies to errors by inspection; the independent browser probe exercised success.

An initial probe assumed Playwright role locators would exclude modal-inert background content. This browser did not do so. I discarded that assumption. The finding rests on exact dialog containment, the disappearance on close, and the rendered screenshot, not an accessibility-tree claim.

## Accepted checks

- The manager authorizes before query validation or record access. The page scans no more than 512 artifact rows, decodes one bounded retained task at a time, and never reads blobs. Stored task views are capped at 24 KiB by `tasks::insert_task` and `tasks::save`. The allocation allowance is conservative for those legitimate stored values.
- Fixed 32-entry pages, printable ASCII query bounds, literal case-insensitive matching, lowercase filename and UUID ordering, strict cursor parsing, filter binding, and the 1 KiB cursor cap match the design. A cursor carries no authority. Sealed unavailable rows remain readable, and pending reservations remain excluded.
- Catalog identity checks bind the row, retained task, command task, offer task, offer slot, and available or transferring artifact identity. Catalog and source reads require the source command owner to equal the principal. Deleted bot source identities remain retained.
- Native reads run through the existing actor and return manager and generation scope. New commands authorize their Tauri window. The shared `TaskViewModel` excludes raw request and command records and preserves the existing ordinary action and evidence presentation. Protected-login work carries references and a plan, not credential values. The projection is correctly documented as not being a general secret scrubber.
- Export reads the authoritative retained task before requesting bytes. It preserves declaration equality, encoded and decoded size bounds, length, digest, private staging, synchronization, and rename. After the chooser, it checks current online scope, performs an authoritative owner read, then checks manager and generation again immediately before the real save. The focused live-manager test passed for revocation after scope capture. The already acknowledged same-generation revocation path is also covered by the committed regression and matches the code.
- Separate controller request serials prevent stale page and source successes and failures from replacing newer results. Epoch, generation, manager, and online authority checks cover sign-out, manager transitions, and authority loss. Filter changes invalidate source reads and export notices. Retained tasks are not inserted into execution snapshots or the protected cache.
- The synthetic worker changes are under the existing test module and the P05 opt-in flag. They do not add production authority. There is no migration or backup format change. The old private writer and extracted task presentation preserve their previous bodies.

## Verification and evidence

I independently ran these targeted checks:

| Command or probe | Result |
| --- | --- |
| `cargo test -p botstral-manager --locked --lib offline_restore_reopens_owner_history_markdown_credentials_and_artifacts` | One test passed. This covers the 512-row catalog, restore, source lookup, exact bytes, and corrupt or missing paths. |
| `npm run test --workspace @botstral/desktop -- src/paired.test.tsx` | All 21 focused tests passed. |
| `python3 desktop/fixtures/paired_manager.py --verify-journal --journal-case p05_export_reauthenticates_after_chooser` | One real manager/native test passed. The temporary fixture was cleaned up. Its log and command record are in `reviewer-probes/native-reauthorization.*`. |
| Independent source and evidence digest comparison | All 420 recorded source hashes match HEAD. All 13 recorded command exits are zero, each corresponding log ends in `EXIT_CODE=0`, and video and native capture digests match. See `reviewer-probes/source-hash-verification.json`. |
| Actual-component export feedback probe | Failed the intended modal-contained feedback assertion, as detailed above. |

The first renderer test invocation used `npm test -- --run desktop/src/paired.test.tsx`. Npm forwarded a path relative to the wrong workspace and found no tests. It exited 1. The corrected command above ran all 21 tests successfully.

The recorded source identity names pre-final-commit HEAD `fbd60bd4e4ad5688178195be9561876a6c46c041`. This is not source drift. The per-file hashes include the final UI and verification files and exactly match `aa34dab`. I reviewed the complete final command records and relevant behavioral logs, not thousands of routine build lines. I did not rerun the full broad suites or repeat the 88-second restart acceptance. Their recorded results are backed by matching source hashes and inspected test implementations.

The earlier chooser failure is a real failed behavioral assertion, not merely a startup or compile failure. Its generation-only guard matches the preserved base implementation. The parent reports that this fixed defect is now tracked as issue 61. I did not query the remote issue.

The replay video is honestly labeled. It proves actual component interaction with captured reads, not packaged native application behavior, a real OS chooser, actual worker execution, or host reboot. Real manager/native disk-save and restart checks and manager backup restoration checks provide separate byte evidence. The visual feedback finding requires a refreshed interaction proof after correction.

## No-comments report

This is a report-only pass over added or moved code comments. No files were changed. There are three recommended deletions:

- Delete the allocation narration at `link/src/artifact.rs:151`. It restates the assertion's arithmetic.
- Delete the history-trimming narration at `desktop/src-tauri/tests/journal/saved_results.rs:180`. It narrates the adjacent fixture loop.
- Delete the unused module-purpose docstring at `desktop/fixtures/p05-manager-switch.py:2`. It does not define an API contract.

Keep `scripts/verify-p05.py:2`. Its module text is used as the command-line parser's help description through `description=__doc__`; it is runtime user-facing text. Preserve the baseline chooser source evidence unchanged. No new correctness suppressions, dependency-rule comments, `MUST KILL` flags, `HOLD` comments, restored comments, encoding offers, or reruns of a comment reviewer were needed. There were zero actual deletions. No separate reviewer was spawned because the assignment explicitly prohibited subagents.

Type System Discipline guided the review of schema-derived desktop values, branded artifact and task identifiers, and discriminated request state. Prove It Works led to an actual-component check, an independent live-manager reauthorization test, and direct digest comparisons instead of relying on summaries.

## Decision trail and Attention

The parent trail's selected design agrees with `.audit/p05/design-judge.md` and the implementation. The available local P06 records support the claimed build result, reboot fixture identity, cleanup report, and subsequent local harness corrections. The eight-test harness correction is recorded as passing. The readiness and export-security command records record failing baselines and passing corrected runs. I did not repeat their remote observations or query pull request 58.

Attention remains on these reporting boundaries:

- Append the P05 handback, source-hash verification, issue 61, this finding, and its eventual correction to the parent trail. At review time, the parent trail stopped at the P06 export-security correction, while the implementation checkout had its own later completion records.
- Keep P06 and pull request 58 status separate from P05 acceptance. The parent trail's last pull request status was waiting for fresh checks, not a verified final remote state.
- The P06 reboot evidence used the recorded earlier harness identity. Later harness fixes were tested locally, and the readiness record explicitly says no reboot was repeated. Do not describe those later fixes as having a fresh real-reboot proof.
- Describe this P05 slice as retained generated-file discovery and verified export. Do not claim complete P05, packaged application acceptance, or instantaneous remote revocation visibility.

The checkout remained clean at the frozen HEAD after review. Acceptance is blocked only on the P2 modal feedback correction and its targeted visual regression check. No security or durable-byte correctness blocker was found in this scope.
