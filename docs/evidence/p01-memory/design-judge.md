# Cross-judge verdict

Use Astra as the base. It meets the workflow with one immutable column, a private prepared admission package, and no public protocol changes. Keep its `PreparedRun` contract intact: `insert_run` accepts that package, never independent view and snapshot arguments. Dispatch and legacy SQL NULL must never consult live memory.

Scores use 0 for absent, 1 for substantial gaps, 2 for a workable design needing a concrete correction, and 3 for a complete design.

| Criterion | Astra | Sol | Luna |
| --- | ---: | ---: | ---: |
| Atomic immutable snapshot; legacy and idempotence behavior | 3 | 2 | 3 |
| Authoritative same-bot provenance; full context bounds and secret reflection | 3 | 3 | 2 |
| Unchanged public protocol and original prompt | 3 | 3 | 3 |
| Owner, routine, and wakeup coverage | 3 | 3 | 1 |
| Smallest durable interface and migration structure | 3 | 2 | 2 |
| Total | 15 | 13 | 11 |

The Luna durability score includes the task's clarification that its column has an immutable trigger and explicit insert columns. Its document alone does not specify those details. Sol describes no snapshot updates but does not specify a database immutability trigger for its new table; add such enforcement if choosing that alternative.

## Evidence and tradeoffs

The repository currently has one bare production `INSERT INTO runs`, at `manager/src/manager/state/runs.rs:710`. Its only production callers are owner submission and routine occurrence commit. The other located insert, `connectors/tests/catalog_bounds.rs:57`, already names columns, including rowid. Converting the production insert is bounded work. Preserving five-column positional inserts is therefore not a reason to keep a permanent second row lifecycle.

`manager/migrations/0006.sql` owns the existing immutable-run trigger. Extend that trigger in migration 11 to cover the new column. `state.rs` explicitly caps schema version at 10 and sequentially applies migrations. Advance the supported version and update unknown-version tests together. The parent additionally verified that `manager/packaging/recovery.py:148` hardcodes supported schema versions through 10. Update backup and restore support to 11 and add a focused recovery regression in the same migration change. Legacy rows remain NULL without backfill.

Sol's separate table can be atomic when both inserts share a transaction. It nevertheless introduces an absent-row state that is both legitimate for legacy runs and potentially hides a missed new-admission insertion. Its distinction between legacy absence and newly admitted NULL is diagnostic, not required behavior. Astra stores both as no memory and preserves explicit cleared Documents separately when available.

Astra and Sol correctly require fresh authoritative reconciliation during new admission, complete encoded-request bounds and secret reflection, and source ownership validation at admission. Specifically run `reflects_secret` over the complete prepared chat request before insertion, including the memory context; the existing initial SubmitRun-only guard cannot cover injected memory. Luna lacks explicit secret-reflection coverage and its separate `view` and `memory` insertion arguments permit mismatching an otherwise correct admission pair. Luna also does not resolve the existing wakeup preflight caller. Startup reconciliation alone is insufficient for external edits after startup.

The existing owner idempotence check precedes validation and must remain there. Existing `prepare_occurrence` and `prepare_wakeup` take `&self`; both need mutable access when admission reconciles documents. Wakeup preflight discards its provisional package. The eventual due occurrence captures current memory. Snapshot loading validates stored shape, bot, and kind without checking live source retention or fetching current Markdown.

## Concrete grafts

1. Add Sol's explicit exclusion proof for the same bot's Instructions document, alongside another bot's Memory. This strengthens the narrow Memory-only boundary with little fixture cost and no interface expansion.
2. Add Sol's transaction rollback proof to Astra's routine tests: failure after run insertion must leave neither a run nor occurrence reservations. With a column there is no independent snapshot row to orphan, but the test verifies that the existing routine transaction still owns the complete admitted package. Use an existing fault or transaction fixture if available; do not add a production fault framework solely for this test.

Keep Astra's choice to omit an empty Markdown document from provider context while preserving its stored snapshot. Sol's empty metadata message is valid but adds input without helping the corrected-preference workflow. Keep Astra's memory message before the unchanged original request, with full structured provenance and a fixed untrusted-data preface. Neither ordering nor the preface is an authorization boundary; existing manager policy remains authoritative.

No production files changed. This judgment read grounding and all three candidates completely, then checked the actual inserts, migration owner, owner idempotence, routine preparation and commit, and wakeup preflight.
