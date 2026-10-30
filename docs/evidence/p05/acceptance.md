# Saved results acceptance

This slice lets an owner find retained generated files, inspect their source tasks, and save identical verified bytes after restart or backup restoration. Full P05 acceptance and P06 reboot verification remain separate.

## Reproduction

Install workspace dependencies with `npm ci`. Install the local browser dependency with `npm ci --prefix agent/runtime`. Run:

```sh
python3 scripts/verify-p05.py --full --video
```

The script records each command, exact exit status, source hashes, and whether source files changed during the run in [source-identity.json](source-identity.json). Each command has a separate log. No external inference provider, SSH guest, or paid service is used. The provider and worker in manager fixtures are synthetic.

## Behavioral proof

The manager catalog and backup test covers 512 records, 32-entry pages, deterministic duplicate filename ordering, two bots, literal wildcard search, invalid limits and cursors, filter-bound cursors, unavailable outputs, pending output exclusion, mismatched artifact identities, source ownership, retained deleted-bot identity, and missing, corrupt, and symlinked files. After an offline backup restoration it discovers the original artifact, reads its source task, downloads the bytes, and compares them with the original. See [catalog-restore.log](catalog-restore.log).

Owner authority is checked before query validation. Worker enrollment is denied, the database refuses worker rows in the owner-device table, and a revoked owner lease cannot read a page. See [catalog-authority.log](catalog-authority.log). [catalog-bounds.log](catalog-bounds.log) checks complete request and reply envelopes and the conservative allocation allowance against existing transport admission.

The real manager and native acceptance test creates 35 sealed files across two bots, then adds 34 newer terminal tasks. It proves that the old source task is absent from recent workspace history. It discovers the file through the catalog, reads the retained source, and saves verified bytes with the real private staging and rename implementation. It repeats discovery, source reading, download, and disk save after both manager and native restart. It compares saved bytes and verifies permissions of 0600. A manager-scope mismatch and sign-out prevent saving. A separate two-manager test re-pairs the same profile to another real manager at the same local generation. It confirms that a source task is readable there and still refuses bytes captured under the old manager scope. See [manager-switch.log](manager-switch.log). See [native-acceptance.log](native-acceptance.log) and the captured [native-views.json](native-views.json).

The shared task projection retains ordinary exact action text, targets, and evidence. Protected login work accepts credential references and site plans. It rejects an injected secret field. The renderer receives the shared projection rather than raw task records. See [native-unit.log](native-unit.log).

Delayed controller promises cover superseded successes and failures within one generation, source selection changes, filter changes, offline, blocked, suspended and signed-out states, manager changes, generation changes, disposal, and stale export notices. Library changes preserve unrelated operation state. See [desktop-tests.log](desktop-tests.log).

## Revocation defect

The original chooser checked generation alone. The real manager acknowledged revocation, native state cleared authority without incrementing generation, and the copied chooser guard still saved a file. The intended assertion failed. This is a correctness defect for parent issue tracking.

[revocation-before.log](revocation-before.log) preserves the failed assertion and exit status. [chooser-before-source.rs](chooser-before-source.rs) preserves the original chooser implementation from base `271f9cd72dabd2d0749027b84d355ca046de5f7e`. [revocation-after.log](revocation-after.log) proves the fixed export refuses that same acknowledged state.

[chooser-reauthorization.log](chooser-reauthorization.log) separately revokes the device after chooser scope capture and attempts saving without first refreshing the native service. The authoritative read refuses the save. Connection closure can initially report offline. A subsequent owner handshake identifies revocation. This does not claim instantaneous visibility of remote revocation.

## Visual evidence

[component-before.png](component-before.png) was captured before visible UI edits at the base source identity. The [component video](saved-results.webm) drives the actual `PairedWorkspace`, `SavedResults`, `TaskCard`, and `PairedController` components from a local file. It shows paging, bot and literal filename filters, the old source task, exact action details, chooser cancellation, and save completion. [component-proof.json](component-proof.json) records the browser, video digest, and limitations.

The video replays captured native manager reads. Its chooser and save notices are simulated and labeled. It writes no exported file. The real native disk-save checks are separate. The packaged native application, OS chooser interaction, real worker execution, and host reboot were not exercised by this slice.

## Retained development failures

The evidence includes the intentional revocation failure, an early compile/startup failure, synthetic worker queue exhaustion, a missing second-session acknowledgment, an offline refusal before explicit revocation classification, and the corrected worker-row test assumption. These are labeled failure logs. A focused check with default desktop features disabled also failed in the existing P06 native example because its optional collection support was unavailable. The required default-feature build is checked separately. No P06 file was changed.

The append-only decision trail remains local at `.audit/p05/decisions.tsv`. The parent still owns independent review, integration, and P06 acceptance.

Model the Domain shaped the ephemeral state unions. Prove It Works required real manager reads and disk-byte comparisons. Sequence Work into Verifiable Units shaped the catalog, native, and desktop commits.

## Final verification

| Command | Exit status |
| --- | --- |
| `python3 scripts/verify-p05.py --full --video` | 0 |
| `./scripts/verify.sh` | 0 |
| `./scripts/verify-desktop.sh` | 0 |

All 13 recorded commands passed with source hashes unchanged. Renderer tests passed 45 cases with one existing skip. [final-verification.log](final-verification.log), [verify.log](verify.log), and [verify-desktop.log](verify-desktop.log) retain exact exits. [cleanup.json](cleanup.json) confirms source and video hashes and no remaining owned processes. [deslop-check.log](deslop-check.log) confirms the extracted task presentation and private file writer preserve their original bodies.

One superseded broad run was deliberately stopped after source changed during added manager-switch coverage and identity checks. [superseded-verification.json](superseded-verification.json) records the stopped process tree. The final frozen run above replaced it.
