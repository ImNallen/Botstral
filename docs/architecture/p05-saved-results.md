# Retained generated results

Owners can find generated files outside recent task history, inspect their source tasks, and save verified bytes after restart or backup restoration.

## Catalog

`ArtifactQuery`, `SavedArtifact`, and `ArtifactPage` live in `link/src/artifact.rs`. `OwnerOperation::Artifacts` returns a derived page from the existing artifact table and retained tasks. No database migration, backup format change, or workspace limit increase is required.

The manager authenticates the owner before validating the query or reading records. It scans at most 512 artifact rows and decodes one source task at a time. Each entry includes the artifact, task, and bot identities, the output declaration, and the existing `ArtifactState`. The offer must match the task and slot. Available and transferring output identities must match the artifact row. The source command's owner must equal the current principal. The retained-task route checks the same ownership.

Sealed unavailable outputs remain discoverable. Pending reservations are excluded. Listing reads no blobs and makes no claim about current disk integrity.

Pages contain at most 32 entries. Requests require a limit of 32. A literal, case-insensitive name fragment accepts at most 80 printable ASCII bytes. Existing filename rules remain unchanged. Ordering uses lowercase ASCII filename and artifact identity. JSON cursors carry that ordering key and the canonical bot and name filters. Malformed cursors, mismatched filters, and cursors over 1 KiB are rejected. Each page reauthenticates. A cursor grants no authority.

Pagination is live. Refresh includes inserts before the current cursor. Availability can change between pages. There are no total counts or chronological ordering claims.

The complete request envelope budget is 8 KiB. The reply budget is 64 KiB. Conservative working allocation is below 2 MiB: one decoded task allowance of 256 KiB, 512 projections at 2 KiB each, and a page copy. Tests compare these bounds with existing owner transport admission. Execution snapshots remain unchanged.

## Native authority and export

`RemoteService` serializes catalog and source reads through its actor session. Replies include generation and manager identity. Read results never enter the mutation journal, workspace snapshots, or desktop cache. Both Tauri commands authenticate their window.

Source reads and workspace serialization share `TaskViewModel`. Ordinary actions retain exact arguments, contents, targets, and evidence. Protected login work contains credential references and a site plan. The projection is not a general secret scrubber. Raw `TaskView` values never enter the webview.

Export first reads the authoritative retained task and matches the task and artifact identities. The existing artifact route supplies bytes. Native code preserves declaration equality, encoded and decoded size bounds, exact length, SHA256 verification, private staging, file synchronization, atomic rename, chooser cancellation, and directory synchronization.

After the chooser, export checks current online authority, generation, and manager. It reads the retained task again through the owner session and rechecks local scope immediately before saving. This rejects acknowledged revocation even when generation stays unchanged. A disconnected session also refuses the save. Remote revocation visibility is not instantaneous.

## Desktop state

`PairedController` owns ephemeral library and selected-source unions. Independent page and source request serials reject superseded replies and failures within one generation. Acceptance also requires the current epoch, manager, generation, and online authority. Filters, sign-out, manager changes, generation changes, and loss of online authority invalidate outstanding reads. Export notices have their own serial. Library changes preserve unrelated operation state.

`SavedResults` presents all-bot and bot filters, literal filename search, next-page navigation, Refresh, source details, and verified save. Unavailable files have disabled save controls. `TaskCard` is shared with current activity. Retained tasks are never inserted into the recent execution projection.

Export success, cancellation, and failure feedback appears inside the open library. Its position remains visible while source details are scrolled. Background workspace feedback is suppressed while the library is open.

## Accepted implementation choices

The selected design is preserved. JSON encodes the bounded cursor. A modal contains the library. The shared native task projection owns its small output vector so both narrow replies and workspace serialization use one implementation.

The existing synthetic desktop fixture acknowledged only its original session. An opt-in P05 branch acknowledges additional sessions and uses their fences. This supports two-bot acceptance without altering ordinary fixture behavior. Generated tasks run sequentially to respect the existing worker queue.

Uploads, OCR, content previews, editing, semantic search, a new durable catalog, and complete P05 acceptance remain outside this slice. P06 files, plans, SSH guests, and reboot verification are separate.

[Acceptance evidence](../evidence/p05/acceptance.md) records commands, proof, and limits.
