# Selected implementation contract

The owner manages memory through conversation. Use the Sol entry design as the base, with the Astra recovery invariants. The Luna whole-document design is rejected because a one-fact change must preserve unrelated memories.

## Domain and module ownership

Add link/src/memory.rs for MemoryId, explicit individual/shared selection, resolved bot/shared ownership, per-entry revision, model proposals, outcomes, and frozen entry snapshots. Individual tool requests never accept a bot identifier. Resolve the bot from the admitted run. Create adds one entry. List returns identifiers and revisions. Update and delete address one entry and require its revision. Keep text and collection sizes bounded without silent truncation.

Manager memory owns authoritative private Markdown entry files and publication recovery. Use root-level filenames compatible with existing backup layout, but ensure the old memory-*.md scanner does not parse the new format. Instructions stay on the current document path. There is one active writer for bot memory. Import legacy memory once without changing its text, revision, scope, or provenance. Keep historical snapshots and evidence intact.

Use a dedicated run-derived MemorySource type when that avoids changing unrelated instruction sources. Record the actual settling run, turn, and bot. Do not invent a conversation message or let the model provide provenance. Legacy imports retain their existing SourceReference list. Use a deterministic identifier for each legacy document and import blank documents as no live entries.

## Publication contract

Filesystem publication and SQLite settlement cannot be atomic. Commit a durable publication intent together with the billed provider response and its accounting before changing authoritative files. A pending intent must prevent provider resend and must recover before new memory reads or run admission. Publish exact intended private bytes with fsync, rename, and directory sync. Finalize the same outcome and receipt transactionally after publication. Persist tombstones for deletion and operation identity for retry reconciliation.

Use the actual run, turn, attempt, and call identity for the publication operation. Delete atomically replaces the same canonical entry file with a tombstone, rather than adding a second move-and-create crash window. On recovery, the intended digest finishes settlement. The expected prior digest permits finishing publication. Any other state requires attention and must not overwrite a concurrent external edit. Never reconstruct current live entries by replaying old completed receipts. A duplicate callback cannot add a revision or create another entry. Existing cancellation, deadlines, configuration checks, duplicate-call rejection, and operation budgets remain before intent admission. Cancellation after intent admission cannot undo a published change, and its receipt remains visible.

## Snapshots and compatibility

Prefer a nullable runs.memory_entries_snapshot column in migration 12, extending immutable-run protection and all recovery schema ceilings. New admissions write an explicit entry array, including an empty array. Null selects the existing frozen memory_snapshot decoder. Both columns null mean no memory. No path fetches current entries for an older admitted run. Snapshot validation bounds content and checks applicable ownership without revalidating history that may later disappear.

Make SubmitRun.session optional and permit an empty worker execution policy. No session is valid only for manager-only tools. Keep existing bot/session checks for worker-backed policies. Existing routine templates can retain their required session. Desktop defaults to manager-only conversation and opts into computer execution explicitly. Do not grant process tools to make chat admissible.

## Secrets and visible behavior

Add a real value-free Vault handoff tool with an enum purpose. Report supported provider setup through the actual protected trusted local administration API. Report that reusable login-item storage is unavailable until P06 exists. Do not invent a form, save a fake secret, or ask for secret values in ordinary chat. Known provider and connector secret values must be excluded from mutations, listing, imports, frozen context, and provider dispatch. State honestly that arbitrary unknown free text cannot be deterministically identified as secret.

Expose real provider memory tools and execute them through the manager settlement path. Persist a typed committed outcome and project it into the conversation. Render saved text and scope for create, update, and delete as appropriate. The committed notice survives a failed final model reply. Proposed arguments are never a success notice. Do not echo rejected secret-bearing content in failure receipts.

Delete the shipped Memory viewer/editor, its application and native mutation entry points, active styles, and obsolete editor tests. Preserve old immutable native journals only as needed to report prior uncertain operations without replaying legacy writes. Replace fixtures with conversation proofs rather than retaining an importable editor just for old recordings.

## Verification and handoff

Verify real provider definitions, parsing, manager persistence, follow-up turns, both scopes, isolation, conflicts, one-fact updates, deletion, import, restart, duplicate/replay cases, publication crash points, snapshot compatibility, no-worker conversation, disabled worker tools, known secrets and rotation, and truthful Vault outcomes. Run applicable repository checks and record raw results. Capture a before image and an interaction video through real components using actual recorded-provider manager outputs. Label component replay versus packaged app execution.

Implementation stays in one isolated checkout. The parent integrates and independently reviews afterward. No deployment or merge is authorized. Parent owns the final PR.

## Parent rubric scores

Scores follow product behavior, durability, compatibility, secrets, simplicity, and verification in that order. Astra scores 5, 5, 4, 5, 3, 5. Sol scores 5, 5, 5, 5, 4, 5. Luna scores 3, 3, 4, 4, 4, 4. The independent Astra cross-judge recommends Sol with Astra grafts and scores Astra and Sol 27 of 30 and Luna 19 of 30. Both parent and judge select Sol. The selected contract incorporates typed provenance, a typed value-free Vault purpose, stable migration identity, cancellation at intent admission, and the legacy scanner exclusion.
