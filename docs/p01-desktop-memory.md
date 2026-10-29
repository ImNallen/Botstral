# Current saved Memory in the paired desktop

The owner can inspect, correct, and clear the selected bot's current saved Memory. This bounded P01 slice preserves provenance and admitted run history. Full memory deletion and the full P01 milestone remain open.

## Controls and scope

Expand **Memory** in the paired workspace to read plain Markdown, its saved revision, and ordered source references. **Edit memory** prepares a multiline draft. **Clear current memory** prepares a blank draft. Both require **Save memory**. The counter measures UTF-8 bytes against 4096. **Discard draft** leaves saved Memory unchanged.

Absent Memory is read-only because initial authoring needs a source-selection workflow outside this slice. A saved blank document remains editable. Worker readiness, inference configuration, vault unlock, and execution permission do not gate Memory edits.

Ordinary refreshes retain drafts. A changed document puts the editor into conflict and shows the current version alongside the retained draft. **Review draft against current** explicitly adopts the displayed revision as the new base. A subsequent **Save memory** remains a deliberate action. Further refresh changes require another review.

Offline and suspended sessions retain drafts and disable writes. Reconnection never saves or retries an edit. Bot selection, manager, owner, and native session generation key the component's lifetime. The editor discloses that switching discards unsent drafts. Sign-out and revocation remove protected workspace content. Late callbacks cannot update an unmounted editor.

An uncertain save retains its draft and the existing operation notice. Refresh reconciles the durable native journal. Leaving an operation unresolved does not save it. The editor requires **Load current memory** before another edit. A stale load response does not release this requirement. The notice **Current memory matches this edit** requires an exact comparison and disappears when current Memory changes. It does not attribute a write to this desktop.

## Native boundary

The strict Zod projection accepts only Memory documents and derives the renderer type from that schema. Older projections default to an empty document list. `Memory.tsx` owns the view, editing, saving, uncertain, and conflict union. Its draft base stays fixed until explicit review.

`PairedApplication.writeMemory` sends only the bot, expected revision, and Markdown to the authorized Tauri `write_memory` command. Native recovery requires an existing Memory document at that revision, copies its ordered sources, and submits the existing `OwnerOperation::WriteDocument` through the durable mutation path. The renderer cannot select document kind or provenance.

The journal admits only Memory writes. Both acknowledged replies and confirming snapshots must match bot, Memory kind, checked expected revision plus one, Markdown, and ordered sources. An unchanged document, later revision, reordered sources, different bot, or overflowing revision does not confirm the edit. Recovery never replays a Memory write. Acknowledged uncertainty remains recorded.

Manager production code and the public link protocol do not change. Corrected Memory enters newly admitted runs. Clearing contributes no Memory context. Previously admitted snapshots remain frozen.

## Acceptance evidence

[Verification results](evidence/p01-desktop-memory/verification.json) record commands, outcomes, and limits. Retained logs include the frontend checks, native suites, all nine actual-manager journal cases, and 15 manager P01 tests. The recorded-provider test now checks blank-context delivery, preserved sources, clear persistence after another manager restart, and unchanged historical runs.

The 14 observable Memory tests cover existing, absent, and blank documents, provenance, edit and clear, multibyte limits, refresh and conflict, offline recovery, uncertainty, bot and session changes, sign-out, revocation, and late callbacks. The native invocation test verifies the narrow command and strict reply parsing. The complete frontend suite passes 42 tests with its expected external-fixture skip.

The [video](evidence/p01-desktop-memory/memory-controls.webm) drives the real `PairedWorkspace` and `Memory` components. It replays [captured native snapshots](evidence/p01-desktop-memory/native-controls.json). The banner and [proof report](evidence/p01-desktop-memory/component-proof.json) label this replay. Native manager behavior is proved separately by the journal tests. Refresh injects the competing-current snapshot in the video.

The [before image](evidence/p01-desktop-memory/before.png) is the owner's unmodified baseline. The [conflict frame](evidence/p01-desktop-memory/conflict.png) shows retained text and explicit review. The [after image](evidence/p01-desktop-memory/after.png) shows saved Memory and provenance. These local images were visually inspected. The video proves interaction, not packaged Tauri window behavior or real inference.

Reproduce the native capture and component video from the repository root:

```sh
BOTSTRAL_MEMORY_VIEWS="$PWD/docs/evidence/p01-desktop-memory/native-controls.json" python3 desktop/fixtures/paired_manager.py --verify-journal --journal-case p01_memory_controls_and_crash_journal_recovery
node desktop/fixtures/p01-memory-proof.mjs /absolute/path/to/playwright/index.mjs
```

Playwright and Chromium must already be installed. Proof tooling adds no package dependency and starts no preview server. The fixture uses an isolated actual manager, a recorded local provider, and a synthetic worker. No paid or real external inference runs.

## Independent review and integration

Independent Astra review found no correctness defect and replayed the actual-manager native Memory fixture. Integration replay passed all 42 frontend tests and 15 manager P01 tests. Workspace and native all-target Clippy passed with warnings denied. The component video replay passed without page errors and reproduced byte-identical saved and conflict images. [Integration results](evidence/p01-desktop-memory/integration.json) record the checked implementation and limits.

The inline editor keeps one temporary draft per selected bot and owner session. Native code owns sources and durable write recovery. This keeps the correction workflow small without a controller-wide draft store or a new protocol operation.
