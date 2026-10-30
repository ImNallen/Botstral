The goal is to make pull request 56 merge-ready by correcting real security findings, without merging. This independent read-only review finds no blocking defect in the frozen correction. The verdict is PASS for the selected security contract. It is not a verdict on the pending full phase and real Linux runs.

Reviewed HEAD is `13580e424dd83a568a88714593d74924440bc840`. The review includes the uncommitted files named in `/Users/nallen/.t3/worktrees/Botstral/t3-010ce7cb/.audit/p06/security-design/review-source.json`. All twelve hashes matched before review and after the probes. The exact manifest is copied to [review-source.json](review-source.json), whose SHA-256 is `b19a1bdb3cfcb44cb524c6ce3d3c88618f797ae63e3d6d2872bbc8a0d6021735`.

No production files, checkout tests, or audit files were edited. Scratch tests and their module registration exist only in `/tmp/p06-pr56-security-code-review`. There were no subagents, external messages, or pull request operations.

The security decisions follow `.audit/p06/security-design/selection.md`. Screening compares only actual manager-recorded emissions in a known profile owned by the authenticated worker. Prepare admits one root and one exact remembered successor. The first global destination comparison consumes a durable task-scoped attempt even when that comparison rejects the destination.

The source trace supports those decisions.

- `uses.rs:159-175` and `connectors/gateway.rs:22-74` require the current admitted parent, worker generation, input fence, journal, execution, permission, approval and deadlines. `uses.rs:413-422` binds that parent worker to the authenticated certificate. Historical screening does not call this authority check on old uses.
- `uses/profile.rs:17-40` reads recorded ownership from `tasks.worker` and cross-checks the saved task identity and approval binding. `tasks.rs:64-86` assigns the column during insertion. `tasks.rs:52-61` and recovery update the task view without reassigning that column. Its immutability is a production write-path invariant, not a database trigger.
- `uses/profile.rs:65-87` refuses unknown profiles, duplicate supplied releases and any mismatch with the complete set of Released, Applied and Uncertain records. `uses.rs:98-131`, `239-253` and `463-476` preserve every possible emission across revocation, reconciliation, recovery and settlement.
- `uses/profile.rs:90-123` decrypts each recorded epoch for integrity. Password uses contribute their emitted username and password. TOTP uses contribute only their retained issued code. The selected screen path never loads unrelated provider, connector, login or issued-code inventories.
- `uses/preparation.rs:112-163` bounds the saved history to a root and at most one successor. The successor requires the root's exact context, revision increment, remembered allow cause, valid source and original deadline. Cached terminal roots and successors return before capacity checking and the global destination comparison. A changed destination cannot allocate another row.
- `uses/preparation.rs:164-175` checks current use authority and capacity before committing the attempt in SQLite autocommit mode. A failed comparison leaves that row durable. A subsequent request with no saved review conflicts before the global scan. A successful review supplies the exact destination needed for a cached response.
- Migration 16 creates the retained, update-protected task-keyed attempt table. Existing saved reviews remain bounded by their history after migration. `state.rs:165-170` runs the migration transactionally. `recovery.py:148` accepts version 16 and continues to reject newer versions. Backup uses SQLite's whole-database backup, so the attempt table is not omitted.

I ran `cargo test -p botstral-manager --locked pr56_security_ -- --nocapture` in the original checkout. All ten focused tests passed. `git diff --check` passed.

I also wrote five independent probes in [probes.rs](probes.rs), SHA-256 `08ed672c9d9a29056e147061d8ab85b3d1549248fd1ff336e531085b6b3dcbb8`. They call production State methods through the existing admitted-worker fixture. The four credential production sources and migration 16 in the scratch checkout were checked byte-for-byte against the frozen checkout. All five passed, with the complete output in [probes.log](probes.log).

The independent probes established these observations.

- An applied password remains screened after credential rotation and admission of a new capture task. The newer, never-emitted password is accepted. Corrupting the old retained ciphertext causes benign screening to fail.
- A rejected first destination remains closed after a SQLite snapshot and connection reopen. Update and deletion of the attempt fail. A changed retry still returns Conflict when the global connector-secret table is unavailable, proving that retry does not reach that scan.
- After recovery revokes a remembered successor, its cached identity, state and deadline still return at global capacity. The global connector-secret table can be unavailable. Release remains refused and the number of use rows does not grow.
- A released use becomes Uncertain on recovery. A new capture task can screen its history after the original fill-authority row is removed. Benign text and the unexposed seed pass, the old password fails, and omitted history fails.
- Migration 16 preserves a preexisting saved root. A legacy second root for the same parent is refused without deleting either saved record.

Reproduce the Rust probes with the retained scratch checkout.

```sh
cd /tmp/p06-pr56-security-code-review/checkout
CARGO_TARGET_DIR=/Users/nallen/.t3/worktrees/Botstral/t3-010ce7cb/target cargo test -p botstral-manager --lib --locked independent_review_ -- --nocapture
```

The scratch checkout copies the workspace Cargo files, component manifests, source trees, migrations, runtime files and manager test fixtures. Its sole test integration change appends `#[path = "/tmp/p06-pr56-security-code-review/probes.rs"] mod independent_probes;` to the copied `manager/src/manager/state/credentials/tests/live.rs`. The shared target directory is only a build cache. The original checkout's next Cargo invocation will rebuild its own crate if needed.

A separate probe calls the production Python recovery loader. It accepts a version 16 database containing a durable attempt, preserves that attempt, and refuses version 17. The script is [recovery_probe.py](recovery_probe.py), SHA-256 `42130609bf7986ee98d94e37992eb0ccf0e5ef6e64c16ef0be6d8a6486e39044`. Its output is [recovery.log](recovery.log). Reproduce it with `PYTHONDONTWRITEBYTECODE=1 python3 /tmp/p06-pr56-security-code-review/recovery_probe.py`.

The residual first destination membership comparison is documented and bounded to one owner-admitted parent. A crash after the attempt commits may burn the parent. A new admitted task is the recovery. Unknown, incomplete or unemitted profile history fails closed. Full guest compromise, arbitrary secret transformations and coordinated rollback of every trusted record are outside this verdict. No claim is made that this change eliminates every membership signal.

The Model the Domain principle shaped the review around task ownership, recorded emissions and the root-successor state machine. The Prove It Works principle changed the verification from test inspection alone to independent calls into the production methods. Both leaf skills were read. The how workflow ran as one read-only pass because the brief forbids subagents. The no-comments delegation was skipped for the same explicit constraint. The throughput checkpoint is not applicable to this read-only investigation.
