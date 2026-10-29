# Independent design judgment

Use Sol as the base, with Astra's typed provenance, closed Vault purpose, and precise cancellation boundary. Preserve authoritative Markdown and add the publication intent journal. Do not adopt Luna's write-before-receipt protocol.

## Scores

Each criterion is scored from zero to five. These judge the proposed design, not demonstrated implementation.

| Rubric criterion | Astra | Sol | Luna |
| --- | ---: | ---: | ---: |
| Both scopes, conversation CRUD, no editor | 5 | 5 | 4 |
| Durable settlement and crash safety | 5 | 5 | 2 |
| Snapshots, provenance, recovery, protocol | 4 | 4 | 3 |
| Secrets and protected handoff | 4 | 4 | 3 |
| Small public model and clear ownership | 4 | 4 | 3 |
| Real provider and visible verification | 5 | 5 | 4 |
| **Total** | **27** | **27** | **19** |

Sol wins the tie because it identifies the actual backup layout constraint, native send prerequisites, snapshot immutability trigger, and obsolete owner write path. Astra supplies stronger details for several boundaries. Neither is ready to implement verbatim.

## Base and grafts

1. Keep Sol's scoped entries, manager-derived bot, single writer, nullable new snapshot column, and manager-only admission. Entries are a defensible product choice for independent fact deletion, but the brief does not explicitly require independent identifiers. Do not justify their cost by claiming scoped documents cannot satisfy CRUD. Their benefit is avoiding unrelated fact replacement and shared-document contention.
2. Keep Sol's root-level layout, but change the proposed filename prefix or narrow the legacy scanner. `state/memory.rs::reconcile_documents` currently interprets every `memory-*.md` as an old `Document`; `memory-entry-<id>.md` collides with that parser. Use a distinct prefix and strict metadata validation. Define tombstone and archive filenames under the same root layout contract.
3. Graft Astra's memory-specific `MemorySource::Run { bot, run, turn }` and imported-source variant. Sol's new global `SourceReference::Run` expands an existing protocol used by Instructions and other documents unnecessarily and records less precise provenance. Derive live provenance from settlement, validate ownership at creation, and preserve imported references without revalidating deleted source records on every frozen snapshot read.
4. Graft Astra's closed Vault purpose enum and typed supported/unsupported outcomes. Sol's free-text `purpose: String` contradicts its claim that the handoff cannot carry a secret value. Reject unknown fields and never echo invalid arguments.
5. Graft Astra's explicit intent-commit cancellation boundary. Persist the billed response and reservation before file publication. After intent commit, recovery completes the admitted operation even if cancellation arrives; the durable receipt remains visible, and subsequent inference stops.
6. Graft Astra's deterministic legacy entry identifier and explicit empty-document handling. Define one idempotent import state transition, including the crash between new-file publication and old-file archival. Never import both active copies or treat an archive as a live legacy file.
7. Keep Sol's new nullable snapshot column and immutable-run protection. New admissions store an explicit empty vector when appropriate. Old snapshots and SQL null retain their original meaning. Define combined individual-plus-shared context bounds, not only separate per-scope bounds.
8. Keep both designs' typed conversation notices independent of final model prose. Show committed changes after restart and after failed follow-up inference. List output must also have an honest visible outcome if the following inference fails.

## Required refinements

- Specify publication identity using the actual durable run/turn/attempt and call identifiers. Do not add independently generated retry identities. Reuse with changed arguments must fail before publication.
- Replace an active entry with a tombstone at the same canonical path. Sol's wording about moving the file and creating a tombstone leaves a second filesystem transition unspecified. Atomic replacement plus directory sync keeps deletion inside the existing publication protocol.
- State that unresolved publication blocks dependent reads and new snapshot admission, including recovery paths. Never serve a half-published state or retry the provider while publication is pending.
- Validate known secrets at admission, mutation, migration/import, list/context dispatch, and visible receipt rendering as applicable. Credential rotation can make previously safe text sensitive. Do not persist rejected candidate text in journal errors or notices.
- Audit the actual protected secret inventory. Neither provider-key reflection alone nor an unknown-string classifier proves general secret exclusion. Explicit secret-storage prompts need recorded-provider tests that produce handoff or refusal, with the unknown-secret detection limit stated honestly.
- Define stale native mutation handling. Preserve old journal decoding where necessary, but reject pending legacy memory writes explicitly; do not replay them after conversational migration.
- Include backup/restore with active entries, tombstones, archives, and unfinished publication intents. Confirm the current schema number before choosing a migration number.

## Why the other designs are not the base

Astra's namespace directory avoids the legacy scanner, but `manager/packaging/recovery.py::manager_layout` accepts only root files and the existing artifacts directory. Its proposal needs an explicit recovery-layout change. Root files with a noncolliding prefix are smaller. Its versioned snapshot decoder is reasonable, but Sol's added nullable column makes old/new context distinction and trigger coverage easier to audit.

Luna keeps the smallest storage domain, but writes Markdown while the SQLite transaction is still uncommitted. A crash after rename can leave no committed billed response, operation identity, or settlement receipt. Content/revision matching alone does not identify the original admitted action or prevent provider redispatch. Its delete protocol is similarly underspecified. Adopting a durable intent would erase much of its claimed simplicity advantage. It also leaves manager-only admission changes mostly as requirements rather than concrete contract changes, and relies too heavily on subsequent bot replies for visible durable truth.

## Acceptance evidence

Run actual provider schema and settlement flows for both scopes and all operations. Fault before intent commit, before rename, after rename, and before receipt commit; verify one revision and one visible receipt. Cover migration interruption, null/old/new-empty snapshots, guessed foreign identifiers, shared access, limits, cancellation, credential rotation, malformed handoff arguments, stale native journals, and backup restore. Record the real conversation components with committed notices and no editor; label recorded-provider or unpackaged execution limits precisely.
