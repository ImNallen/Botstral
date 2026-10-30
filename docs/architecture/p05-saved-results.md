# P05 saved results design

## Problem and usage

Owners must find retained generated files, inspect the producing task, and export identical bytes after restart or backup restoration. Open Saved results, choose all bots or one bot, search by file name, page through results, open Source task, then Save verified file. The UI labels file-name ordering and recorded availability honestly.

Use the existing SQLite/artifact store. Do not enlarge execution snapshots or add a second result store. No uploads, OCR, previews, editing, semantic search, or full P05 completion claim.

## Chosen contract

Use candidate-astra.md as the base. Both parent and independent judge selected it. Graft candidate-sol.md's explicit transport/allocation bounds and existing ASCII filename rules. Reject Luna's task-rowid ordering because it implies chronology without a product timestamp. Omit media filters, availability filter controls, and a backward-cursor stack unless needed for the basic complete flow. Include unavailable sealed results with disabled export and readable source details. Pending reservations with no sealed offer are not saved results. Fixed next-page plus Refresh is sufficient.

The core data shape is a bounded artifact page derived from at most 512 artifact records joined to retained tasks. Each entry has stable artifact ID, task ID, bot ID, output declaration, and existing ArtifactState. Match artifact/task/slot and available/transferring ID consistently. Authorize before inspecting inputs or records. Require source command owner == principal. Scan bounded artifact rows, not unbounded task history. Do not read blobs while listing. Avoid retaining every decoded task at once if a per-row projection bounds allocations more simply.

Shared types belong in link/src/artifact.rs, with a new OwnerOperation and OwnerReply arm. No schema migration or backup format change is necessary. Fixed pages of 32, at most 80 ASCII bytes in a literal case-insensitive name fragment, bounded cursor of at most 1 KiB, deterministic (ASCII-lowercase filename, artifact ID) order. Printable ASCII punctuation and spaces are legal under existing Computer output rules. Normalize filters once and bind cursor to canonical filter values. Reject malformed, oversized, or mismatched cursors. Every page reauthenticates. A cursor grants no authority. Define and test the complete request/reply envelope bounds and allocation bound against existing owner transport admission; do not enlarge workspace bounds for this read.

Pagination is live. Inserts before the cursor appear on Refresh. Availability can change between pages. Do not promise immutable pages, total counts, timestamps, or fresh disk integrity from catalog metadata.

Native RemoteService has narrow catalog and retained-source read methods, serialized through the existing actor session. Each returns a generation and manager scoped result. Reads never enter the mutation journal, workspace snapshots, or protected desktop cache. Reuse and extract the existing TaskViewModel projection, never raw TaskView into the webview. That projection includes ordinary exact actions and evidence, and is not a general secret scrubber. ProtectedLogin remains references and plan; add regression coverage. Every new Tauri command authenticates its window.

Use Zod schemas with inferred desktop domain types. PairedController owns one ephemeral library state and selected-source state using discriminated unions, plus separate page/source request serials. Per-request acceptance requires matching epoch, manager, generation, and request serial with current online authority. Invalidate on filter changes, sign-out, manager change, generation change, offline/suspend/blocked/revoked state. Late failures must be ignored as well as successes. Do not splice old tasks/runs into current execution state. Reuse TaskCard presentation for source details; no duplicate action formatting.

Export always fetches the authoritative retained Task first and verifies requested task/artifact IDs before asking the existing Artifact byte route. Preserve declaration equality, maximum encoded/decoded sizes, length and SHA256 checks, private staging/sync/rename, native chooser and cancellation behavior. Never use catalog metadata as integrity authority. After chooser completion, require current generation, manager and online owner authority, then perform an authoritative owner read that detects acknowledged lease revocation before writing, and recheck local scope. Revocation can clear authority without incrementing generation. No claim of instantaneous remote revocation visibility. Reproduce the old same-generation chooser flaw with a meaningful failed-before test before fixing it, and report proof to parent so it can track the defect.

## Module ownership

Link owns the shared protocol. Manager state/artifacts owns one derived catalog query and state/devices dispatches it. remote.rs owns native read serialization, shared task projection and export integrity; shell.rs owns window-authorized commands and chooser. paired-domain/native/application own validation, adapters, and ephemeral request state. PairedWorkspace with one adjacent SavedResults component owns presentation.

One implementation owner changes this coupled protocol/manager/native/UI in its isolated worktree. P06 reboot verification uses its own files, worktree and SSH guest, so it can complete independently. P06 acceptance remains open until its real reboot evidence arrives. P05 must not touch either SSH test guest or P06 scripts, docs, or PLAN rows.

## Verification and delivery

Use existing synthetic manager fixtures without model calls or spending. Prove an artifact's task is absent from recent workspace history after more than 32 newer terminal tasks, then discover/open/export it. Cover two bots, duplicate filename ordering, literal wildcard search, complete pages, filter changes, invalid limits/cursors, owner/worker/revoked authority, unavailable rows, missing/corrupt bytes, retained deleted-bot source identity, and maximum serialization/allocation budgets.

Real manager/native tests must discover and export the old artifact with byte comparison after manager/native restart. Manager backup tests must discover source and download identical bytes after restoration. Preserve current artifact handling for missing files, symlinks and corruption. Controller delayed promises must prove same-generation query/source races, authority changes and stale notices do not republish data. Native export tests must cover same-generation acknowledged revocation during chooser as well as sign-out and manager switch. Use the real save implementation; simulated chooser UI alone is not disk-export evidence.

Capture the baseline before editing visible UI. Record an honestly labeled actual-component interaction video with native captured responses, filters, paging, old source task and save interaction. Separate real manager/native export evidence from replay. Do not use preview tools. Use local file Playwright as existing fixtures do. If packaged app not exercised, state that exact limit.

Run meaningful focused checks, then scripts/verify.sh and scripts/verify-desktop.sh. Read output and fix failures. Prefer three ordered commits for catalog, native reads/export, and UI/acceptance, each verified. Reconcile accepted implementation deviations in the actual committed design doc. Root will independently review code, evidence and decision trail before creating a ready PR. No merge is authorized.

## Candidate scores and reconciliation

Parent scoring agrees with judge. Astra and Sol each score 11 of 12; Luna scores 9. Astra explicitly fixes observed-revocation export fencing. Sol adds more concrete request budgets and filename semantics. The selected contract combines those strengths without extra filters or stored pagination state. Design alternatives converged on one store and narrow reads, so no contested architectural choice needs an adversarial panel at this stage.

## Implementation notes

The catalog reads artifact identifiers with a SQLite row iterator and decodes one retained task per row. At most 512 projected entries are sorted in memory. It returns 32 entries and a JSON cursor bound to the canonical bot and lowercase literal fragment. Listing never opens a blob. A source read also checks the retained task's owner principal.

The request envelope budget is 8 KiB. The reply budget is 64 KiB. Conservative working allocation is 2 MiB, comprising one 256 KiB decoded task allowance, 512 projections at 2 KiB each, and a page copy. These bounds fit existing transport admission. Workspace limits are unchanged.
