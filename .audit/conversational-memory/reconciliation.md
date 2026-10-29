# Design reconciliation

The selected Sol design and Astra recovery invariants remain the implementation contract. No required behavior was intentionally weakened.

| Decision | Reason and evidence |
| --- | --- |
| List bounded entries by scanning authoritative root files. | A new search index is unnecessary for List. Instructions retain the existing rebuildable document index. New entry names cannot enter the legacy parser. |
| Import and recover after Vault unlock. | Secret inventory checks require the decrypted installation identity. Waiting publication never resends its provider call. Locked startup remains available. |
| Exclude old Memory documents from document search and workspace projection. | Memory has one conversational path. Legacy documents are read only by the import path; they are never copied into the old plaintext index before secret validation. |
| Preserve imported source identifiers without requiring surviving history. | Migration must retain provenance after history pruning. Shape and ownership still validate. The acceptance test imports a removed message reference. |
| Keep a finite limit of 256 canonical files, including tombstones. | The existing installation record ceiling bounds scans. Create and import refuse admission at capacity. Tombstones are retained for authoritative deletion state. |
| Move old document-index and aggregate-capacity fixtures to Instructions. | WriteDocument Memory is deliberately rejected. The replacement memory suite tests legacy imports, old snapshots, publication crashes, and backup restoration of live entries, tombstones, archives, and pending intents. The remote production acceptance harness now labels its unchanged document-backup proof as Instructions. |
| Reconstruct archived Vault notice text from its typed outcome. | Compaction otherwise substitutes a generic execution-detail message. The saved handoff remains truthful after compaction. |
| Check the actual retained secret inventory. | Current Mistral key plus all retained connector credential epochs. Unknown strings, arbitrary encodings, and overwritten provider keys cannot be perfectly classified. This boundary is documented rather than replaced with a heuristic classifier. |
| Use a component interaction recording over actual native captures. | Native integration proves the manager and journal paths. The video proves rendering and interactions. It does not claim packaged Tauri execution or real worker execution. |

The accepted base is 41aec24ae8b62b7cb35874cdd4feb3760425e198. One writer used the assigned checkout. No additional checkout, subagent, push, pull request, deployment, or paid inference was used. Parent review and integration remain separate.

## Parent integration

The preceding implementation record describes the isolated owner's work. The parent integrated its four commits, ran independent reviews, and corrected the findings. The reviewed runtime source is `bace80bed72fe5aca55e9f258ab68827178bb1d2`.

The public boundary now scans the complete `RunView`, including immutable configuration and archived tool-call identifiers. Partial field redaction was rejected because it missed arbitrary string fields. Protected or unscannable runs return an explicit refusal. Paired workspaces retain safe summaries through `WorkspaceRun.request: None` and no turns. Normal native reconciliation still retrieves a full checked run and compares the exact original request.

Publication attention rechecks the same expected and intended digests after an external repair. An unchanged conflict creates no duplicate events. Routines count `waiting_for_memory` as unresolved work. Regressions failed before these corrections and pass afterward. Four restart tests now assert locked read refusal before explicit unlock and retain their original persisted-content assertions.

Conversation notices show facts and scope without internal entry identifiers, revision numbers, or milestone names. Protected provider setup remains documented in the API reference. No reusable login-item storage was added.

The parent's separate `parent-decisions.tsv` records integration, corrections, and final verification. The original two ignored parent planning rows were replaced when the implementation's tracked trail was integrated. Their loss is acknowledged. Surviving design and judgment artifacts preserve the decisions, without a claim that the combined history was continuously append-only.
