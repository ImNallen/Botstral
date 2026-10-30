# Independent cross-judgment

Choose candidate A as the base, with the mandatory corrections below. Candidate B is a credible fallback if historical task identity must survive deletion, but current source does not justify its migration. Candidate C leaves too many security and lifecycle decisions unresolved. None closes the Prepare oracle as written.

## Scores

Scores are 1–5. They judge the written designs, not implementations.

| Rubric criterion | A | B | C |
|---|---:|---:|---:|
| 1. Prevent unrelated-secret Screen oracle | 5 | 5 | 4 |
| 2. Ownership, complete emitted history, historical protection | 4 | 4 | 2 |
| 3. Idempotent bounded Prepare and remembered handoff | 5 | 4 | 2 |
| 4. Preserve legitimate behavior and fail closed | 4 | 3 | 2 |
| 5. Small manager boundary; justified extra state | 4 | 3 | 2 |
| Total | 22 | 19 | 12 |

1. A and B explicitly select emitted password username/password or exact issued TOTP code and eliminate Screen's global scanner. C chooses the same general scope, but its invariant referring to passwords and TOTP seeds is ambiguous and must not become a seed scan. All three need the separate Prepare correction below before claiming the worker's global oracle is fixed.
2. A requires full retained inventory, exact set coverage, historical states, and no historical live authority. Its ownership derivation should use the immutable task worker column, with consistency checks. B's durable worker identity is sound after a proven backfill, but its TOTP pseudocode only decrypts the code, whereas its tests promise missing/corrupt epoch refusal. C ignores complete=false and claimed missing/unknown releases, accepts unknown profiles, and leaves ledger backfill unspecified. An absent ledger cannot safely mean empty inventory after upgrading existing databases.
3. A gives an explicit maximum of two records per task, original deadlines, exact remembered cause, terminal retries, full-capacity retries, and legacy multiplicity refusal. B is nearly equivalent but keys lookup by task/execution while asserting a parent-wide maximum, and returns a successor for an initial retry. The current execution is fixed, but task-wide lookup is simpler and does not turn identity changes into new capacity. C offers an example quota of eight without a definitive bound or exact accounting for multiple remembered successors and altered previous values. Its key contains mutable execution/fence/journal identity and its budget counts destinations, leaving the relation between key count and budget unclear.
4. A explicitly preserves epoch checks even for TOTP and discusses fixture changes. Its refusal of unknown profiles matches the requested fail-closed posture. B permits unknown empty profiles, despite the task asking to fail closed on unknown inventory; its TOTP integrity rule needs correction. C intentionally discards existing completeness evidence and lacks migration/legacy treatment. None should silently preserve old same-parent refill tests by minting more authority.
5. A reuses bounded retained records and existing serialized transactions. B adds a private worker column and startup backfill for a future deletion requirement not present in the inspected code. C duplicates emission state and adds preparation accounting without proving that the existing records are insufficient. All three leave the demonstrated Prepare scanner dependency unresolved, lowering this score.

## Evidence and required synthesis

### Retained task identity is usable today, with a precise source of truth

`manager/src/manager/state/tasks.rs:64` inserts `tasks.worker` from the original command. `tasks::save` at line 52 updates only `view`. Repository search found no `DELETE FROM tasks`, no other SQL update of tasks, and no assignment to `task.command.worker` or `task.command.execution`. The task view is mutable, but that does not make its original worker assignment currently mutable in the production lifecycle.

Use `tasks.worker` as durable worker ownership and cross-check the parsed command worker, task ID, and execution against the review. Do not derive authority solely from whichever worker happens to be in mutable JSON. Missing or inconsistent parent metadata must refuse, never disappear through an inner join. This avoids B's new column and backfill while providing an existing immutable identity anchor. If actual task deletion or reassignment is introduced, immutable credential ownership must be preserved before that feature ships. A promise to support deletion is not a reason to pretend current retained rows are insufficient.

This conclusion is source-specific, not a claim that arbitrary database corruption is harmless. A new column also does not authenticate a malicious rewrite of the database.

### Inventory must include all retained emissions

`uses.rs:224` limits owner display to 128 reviews. Do not reuse it. Scan the full bounded credential-use inventory; fail parsing errors rather than dropping rows. Require known owned profile and exact equality of supplied and emitted release sets. Duplicates may be normalized as in A; raw length still needs bounding. Unknown IDs, omissions, another profile, another worker, and supplied unemitted releases must refuse. A known prepared profile with no committed emissions may accept an empty complete claim.

`invalidate_matching`, `reconcile_credential_use`, and release/settlement retain emission evidence as Released, Applied, or Uncertain. Current invalidation turns Released into Uncertain, not Revoked. These records must be screened after item removal, deadline expiry, device revocation, and recovery. Do not invoke `use_authority` on historical records. Validate retained epoch existence/decryption for every emitted record, including TOTP, then match only the exact emitted values. Seeds and unissued codes never enter the scanner.

A supplied unemitted release can conservatively refuse even if the manager knows no bytes were sent. That preserves existing missing-issuance behavior without inventing a secret. A compromised worker can invent a different profile identifier; none of these designs authenticates its browser filesystem. That limit does not justify allowing it to query another recorded profile.

### Prepare maximum and remembered exception

Adopt A's task-wide one-root plus one exact RememberPassword successor. A task contains exactly one ProtectedLogin operation in `uses.rs:330`. Reject changed exact destination or identity after admission. Return existing terminal/reconciled reviews without allocating or consulting capacity. Preserve the original deadline. An initial retry should return the initial review; explicit `previous=root.id` returns the successor. This preserves the existing runtime's explicit revoked-review handoff and is easier to audit than B's aliasing.

The only second record requires the exact existing revision+1, epoch, destination, binding, fence, incarnation, password purpose, allow-rule cause, valid approval source, and unexpired root. Deny, expiry, consumption, arbitrary revision changes, or revoked remembered provenance cannot buy another record. At global capacity, retries remain possible but a genuinely new successor may refuse. Do not reserve capacity or reclaim history for this patch.

Existing databases can contain more than two reviews per parent. Retain all history and refuse ambiguous new preparation. Do not pick an arbitrary initial review or delete older rows.

### Existing tests need intentional fixture changes

`credentials/tests/live.rs:399` expects a second same-parent TOTP preparation to be AwaitingApproval after release. Replace that with an unchanged consumed review; create another admitted parent to test fresh TOTP consent. `live/authority.rs:454` expects new AwaitingApproval authority after revoking the remembered approval device; require no new authority and use a new parent for a subsequent legitimate request. `allow_rule()` fixtures and their subsequent Prepare calls likewise need separate parent admission where they currently depend on creating a replacement root. Keep the exact remembered transition test around `live.rs:408–417`.

### Mandatory bounded Prepare scanner correction

`uses.rs:307` calls `reject_known_secrets(&destination)` before structural validation, task-plan matching, and any durable allocation. An authenticated worker can repeatedly vary a field name with guessed values. Every matching guess refuses before any review exists; the one-root rule is never reached. Simply moving the scanner after existing-review lookup does not bound a sequence of rejected first requests. A explicitly excludes this path; B and C implicitly preserve it. That leaves the same global secret-membership capability available through another operation.

Necessary action within this fix: ensure no worker-controlled, unbounded stream reaches the global scanner. The smallest design that preserves public-destination secret rejection is a durable, task-scoped first-attempt claim made after nonsecret structural/authority checks but before the global scan, covering rejected as well as accepted attempts. Store a digest of the exact destination, not the potentially secret-bearing rejected destination. On a rejected attempt, persist a terminal outcome and refuse all changed/repeated attempts without rescanning. On success, link or derive the existing review; its one remembered successor uses the same exact destination and does not rescan. Commit the claim before the scan so a crash cannot restore an unlimited allowance. A crash after claiming but before completion may conservatively burn that parent's attempt. Bound storage to admitted task identity; do not create an unbounded request ledger.

This adds at most one global comparison per owner-admitted parent, rather than claiming to remove every possible membership signal. If the requirement is zero unrelated-secret comparison, replace the Prepare global scan with an appropriate scoped check and explicitly review how secret-bearing destination metadata is kept off owner/public output. That is a different tradeoff and should not be silently assumed safe. The bounded admission correction above preserves existing global public-channel screening and is the narrower patch. Do not store a raw rejected review merely to consume a slot, since that can persist the very secret-bearing metadata the check rejected.

Add a focused regression proving that first rejected destination guesses consume the attempt, later changed guesses never invoke global screening or allocate reviews, and restart retains that bound. This test is necessary alongside repeated successful Prepare tests.

## Rejections and final recommendation

Reject C's arbitrary quota, ignored completeness flag, absent migration story, and duplicate ledger. Reject B's worker migration unless implementation discovers a real lifecycle mutation/deletion that invalidates the retained task anchor. Graft B's insistence on durable worker identity into A by using the already durable `tasks.worker` field. Keep A's exact retry identity, original remembered deadline, full inventory, historical epoch checks, and explicit fixture changes. Add bounded durable rejected-attempt admission for the existing Prepare scanner. No production edits or tests were performed during this judgment.
