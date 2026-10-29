# Initial saved Memory in the paired desktop

The owner can create the selected bot's first Memory from one existing conversation message and an explicit preference. This adds initial authoring to the [existing edit and clear controls](p01-desktop-memory.md). Full P01 and historical deletion remain open.

## Scope and acceptance

**Create memory** opens a draft without selecting a source. Only existing same-bot messages appear. **Save memory** requires an explicit selection and a nonempty draft within 4096 UTF-8 bytes. An empty source list explains why creation is unavailable. This feature does not add conversation messages or turn run history into provenance.

The native workspace projection joins existing messages to their conversations. It includes the manager's bounded set of at most 256 message previews, taking entries from the end of the workspace list. Later messages from another bot cannot hide an existing source. Each preview contains only its message identifier, bot identifier, and first 240 Unicode characters. Older projections default to no sources. The strict renderer schema brands message identifiers and rejects extra fields or oversized previews.

`DraftBase` distinguishes a selected creation source from an existing authoritative document. Missing sources retain their selection and draft text. The editor never selects a replacement. A competing document becomes a conflict even if creation was already in progress. Only **Review draft against current** adopts that document's revision and sources for editing. Existing edit and clear commands still preserve authoritative source order.

Offline and uncertain outcomes retain creation text. Reconnection and refresh never save or replay it. After acknowledged uncertainty, **Load current memory** either confirms exact matching revision-one state or retains the draft for review. A match requires the same bot, Memory kind, revision, Markdown, and single message source. It confirms current state, not authorship. Bot and owner session changes discard drafts with the existing disclosure. Late callbacks cannot restore disposed drafts.

`PairedApplication.createMemory({bot, sourceMessage, markdown})` invokes authorized Tauri `create_memory` and native `IntentCommand::CreateMemory`. Native recovery requires absent Memory and validates the selected message through a same-bot conversation. It submits expected revision zero and exactly one message source through the existing `mutate(OwnerOperation::WriteDocument)` journal. The manager remains the race authority. Overlapping revision-zero requests can confirm identical current state. Different Markdown or sources conflict. Native recovery never replays document writes.

Manager production code and the public link protocol are unchanged. Test fixture changes reconnect the existing worker and computer session after a manager restart.

## Evidence

[Verification results](evidence/p01-initial-memory/verification.json) list commands and outcomes. The frontend cases cover explicit selection and save, empty and foreign sources, a removed selection, competing creation, retained text, byte bounds, offline recovery, uncertain outcomes, and disposal. Existing editing regressions remain covered.

The [native capture](evidence/p01-initial-memory/native-controls.json) comes from an isolated real manager. The contract rejects missing and foreign sources, rejects oversized Markdown, checks conflicting creation, and reconciles only the exact revision-one document. Concurrent owner transfers can receive a definitive `Busy` refusal before document admission. The test then deliberately submits fresh requests. Admitted identical revision-zero writes confirm matching current state. Different content or sources conflict. Unknown mismatches remain durable across desktop and manager restarts without replay.

The [provider proof](evidence/p01-initial-memory/provider-proof.json) contains the created document, captured provider requests, and historical run views. After manager restart, a new run receives the exact saved document. The earlier run and its persisted admission snapshot remain unchanged.

The [interaction video](evidence/p01-initial-memory/initial-memory.webm) drives the real components using those captured snapshots. The banner and [recording report](evidence/p01-initial-memory/component-proof.json) label the replay. The [before image](evidence/p01-initial-memory/before.png) uses the baseline Memory component at the commit recorded in [baseline details](evidence/p01-initial-memory/baseline-proof.json). The [selected source](evidence/p01-initial-memory/selected-source.png) and [saved Memory](evidence/p01-initial-memory/after.png) images were visually inspected.

The video does not control a packaged Tauri window. Native tests use an in-memory secret store, a local recorded provider, and a synthetic worker. They prove no paid inference, external execution, or operating-system keychain behavior. [Independent review and parent integration](evidence/p01-initial-memory/independent-review.md) record the corrected source-availability defect, final checks, and remaining limits.

Reproduce the capture and video from the repository root:

```sh
BOTSTRAL_INITIAL_MEMORY_VIEWS="$PWD/docs/evidence/p01-initial-memory/native-controls.json" python3 desktop/fixtures/paired_manager.py --verify-initial-memory
BOTSTRAL_INITIAL_MEMORY_VIEWS="$PWD/docs/evidence/p01-initial-memory/native-controls.json" npm test -- src/paired-contract.test.ts
node desktop/fixtures/p01-memory-proof.mjs /absolute/path/to/playwright/index.mjs --initial
BOTSTRAL_MEMORY_BASELINE_REF=7495f8c120c8cb8acb6f2f5c943fbedcb75d11f6 node desktop/fixtures/p01-memory-proof.mjs /absolute/path/to/playwright/index.mjs --baseline
```

Playwright, its video encoder, and a browser must be installed. Set `BOTSTRAL_CHROME` to an installed browser executable when the matching Playwright browser is unavailable. The proof builds a temporary file and starts no server.
