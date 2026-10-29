# Independent P01 correctness and security review

The goal is for corrected same-bot Memory to survive restart and reach a new run, with immutable bounded admission snapshots, exact provenance, unchanged prompts and protocol, and no permission grants.

Verdict: approve this bounded slice. I found no demonstrated material correctness or security defect in the full 22-file change from `7ac6c61` to `c746a1aceaa856847ab3672211a2708eca48e3fe`. This is an independent review. I did not implement the change. Full P01 and real-model adherence remain unverified.

## Evidence and reasoning

- `PreparedRun` owns the view and optional Document together. The explicit insert stores the snapshot atomically in the run row. Migration 11 extends the immutable trigger. `load_run_memory` reads only this column, validates shape, bot, and kind, and never consults current Memory. SQL NULL and a stored blank Document remain distinct. See `manager/src/manager/state/runs.rs:14`, `:345`, `:763`, and `manager/migrations/0011.sql`.
- New admissions reconcile authoritative files and select only the requesting bot's Memory. Owner retry returns the original view before reconciliation. Transcript rendering preserves the original text or multimodal prompt after a provenance-bearing User message. Frozen references do not require later source-history retention. See `runs.rs:67`, `:220`, and `:293`.
- Moving routine preparation outside its outer transaction is consistent with the existing serialized state worker. One synchronous job owns mutable State until completion, so owner changes to bindings, configuration, or budgets cannot interleave during preparation and insertion. Each event visits a routine once. Batch insertion rechecks shared budgets and active-run capacity on the same SQLite connection, so earlier uncommitted inserts count. Internal errors abort the transaction; ordinary admission refusals become blocked occurrences. Runs, reservations, occurrences, and receipts remain in the insertion transaction. See `manager/src/manager/mod.rs:137`, `:225`, `state/routines/ingress.rs:50`, `:96`, and `state/routines/reconcile.rs:4`.
- Reconciliation can publish document metadata before a subsequent admission fails. This is a separate authoritative-document operation, not a partially admitted run. Source-health notifications after failed ingress also remain separate, as before. Replayed receipts return before new preparation. Routine command retries return before preparation. Pending wakeups discard their provisional admission package and capture current Memory when due.
- Admission checks the raw emitted Document and the encoded prepared request for provider-secret reflection. The shared renderer repeats raw Document checking at claim, including after key rotation. This avoids the quote/backslash escape hole caused by embedding JSON after a prose prefix. Provider dispatch retains its complete-body reflection guard. Blank Markdown emits no document or provenance. See `runs.rs:80`, `:315`, `:559`, and `manager/src/manager/provider.rs:312`.
- Transcript and provider-request byte bounds include emitted Memory. Existing claim-time request bounds remain after connector and wakeup tool preparation. Execution permissions remain in the unchanged request and policy checks. The no-permission-grant test exercises an actual rejected process proposal.
- Migration and recovery both support schema 11. Existing future-schema tests moved to 12. No public protocol files changed. The old oversized-catalog test keeps its original ten bindings and assertions after helper extraction. The HTTP provider's new response behavior is opt-in through `memory_echo`, whose default is false.

## Independent checks

The Prove It Works principle led me to rerun the actual tests and inspect provider-boundary assertions rather than accept the owner's handoff alone.

I ran the existing owner-built manager test executable identified in `verify-final.log`, without invoking Cargo or writing shared build artifacts:

```sh
/Users/nallen/.t3/worktrees/Botstral/t3-dfacc57c/.audit/p01-memory/target/debug/deps/botstral_manager-a528a300628b1bf8 p01_ --test-threads=2
/Users/nallen/.t3/worktrees/Botstral/t3-dfacc57c/.audit/p01-memory/target/debug/deps/botstral_manager-a528a300628b1bf8 routine --test-threads=2
python3 -m unittest discover -s manager/packaging -p 'test_recovery.py' -v
git diff --check 7ac6c61..HEAD
```

Results were 15 P01 tests passed, 15 routine tests passed, 9 recovery tests passed, and no whitespace errors. The two Rust selections overlap in two tests. Git confirms this checkout and owner commit `109112c` have the identical tree `872a0872008ff5c4cef42960291833d67bd0deef`.

The HTTP test observes the actual local provider request, compares it with persisted provider input and turn messages, and derives its response from the received document. It excludes preference A, Instructions, and another bot's Memory. The routine tests exercise transaction rollback, batch shared-budget exhaustion, receipt replay, stale bindings, locked vaults, overlap, and due-time wakeup capture.

I read root `decisions.tsv`, selected design, judge, scope documentation, owner handoff, `owner-decisions.tsv`, `proof-commands.tsv`, and `verify-final.log`. The final log supports the claimed 127 passing manager tests with five ignored, 15 packaging tests, feature tests, and production release guard completion. The recorded early packaging failure and pre-fix security failures are not represented as passing proofs. The transaction and escaping adjustments are recorded. No unrecorded scope deviation was found.

## Limits

I reused the owner-built test executable and did not independently rebuild or rerun the entire workspace. Root is performing a separate focused build. I did not add a separate near-capacity event-batch test; the active-run capacity recheck was reviewed structurally, while the shared-budget and rollback paths were executed. The HTTP fixture establishes delivery and deterministic recorded response behavior, not stochastic model adherence. External concurrent Markdown edits are captured when read during preparation; the files are not locked through SQLite admission. Existing dispatch checks still bound any later-added tools. No paid provider, credential, infrastructure, or nested-agent action occurred. Only this report was authored.
