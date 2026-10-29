# First acceptance evidence audit

The goal is to make the first acceptance result reviewable without overstating completion. Eight of nine checklist items are supported by the retained evidence. Helper inference remains unverified. The milestone must remain open. The decision trail needs the corrections below before final handoff.

Reviewed by gpt-6-astra on 2026-10-28. This review covers evidence and the coordinator's decisions, not a second product review of `0383c96`. It used the show-me-your-work and poteto-mode skills. It made no guest connections, provider calls, secret-file reads, product edits, or publication actions.

## Supported conclusions

- `PLAN.md:207` through `PLAN.md:215` and `docs/first-acceptance.md` consistently claim eight verified items. The final evidence has `passed=false`, one missing helper gate, no new provider attempts, and no cleanup errors. `continued-12h.log` ends with `passed=false`, `p03_passed=true`, and `clean=true`.
- The retained workflow records distinct guest identities, worker authority refusals, native service and keychain pairing, one routine receipt per repeated trigger, exact browser approval and export digest, empty cancelled scope, original execution and journal identities after restart, stopped unknown connector outcome requiring attention, snapshot recovery from cursor 615 to 936 with 160 messages, stale authority refusals, and both backup restores. These support the eight checklist items within the documented fixture boundaries.
- All five retained workflow requests identify `mistral-small-2603` and endpoint `eu`. Privacy and preview refusals and a native-visible generic unsupported-execution failure are recorded. These do not prove helper routing or every unsupported model capability. The documentation keeps that distinction.
- The public evidence is byte-identical to `continued-12h/evidence.json`. All eleven public manifest hashes and all twenty attempt-report hashes resolve. The canonical source-manifest hash matches. Final continuation harness snapshots match their recorded hashes. Initial retained workflow compiler artifacts and the final manager artifact have release profiles. The seven changed manager paths match `git diff --name-only b89321e 0383c96`. The documentation distinguishes the earlier workflow, execution-state correction, and final deletion correction.
- The campaign ledger totals 958482 microdollars under its unchanged 1000000 cap. The stopped manager cleanup records contain eighteen responded attempts, each retaining 53249 microdollars, which independently total the same ceiling. This is reserved exposure, not invoiced spend. The run-04 recovery artifact preserves its original report hash, physical path mapping, unchanged database inode, zero attempt counts, post-move quiescence, and independent review.
- Cleanup reports and the actual cleanup command support removed owned guest fixtures and backups, absent fixture processes and active units, acquired state locks, removed temporary provider file and consumed invitations, and the absent named desktop keychain entry. The transcript records successful cleanup output. The scan's two-megabyte filter excludes no current retained file. Local fixture certificates remain private audit material; the documentation does not claim every local credential was erased.
- Restoration is correctly limited to selected roots at their final private-parent paths. The original short workspace path and replacement-machine cutover remain unproved. The graphical evidence is correctly distinguished from native service assertions.

## Required corrections

1. Preserve the pre-reconciliation report snapshots. Six of seven `budget.json` reconciliation `evidence_sha256` values have no saved file matching that digest. `first_acceptance.py:934` hashes `results.json`, then adds `reconciled_budget` and overwrites the file. This exact risk was already recorded in `full-review.md`. I verified that removing only `reconciled_budget` from each current report and serializing with `json.dumps(value, indent=2) + '\n'` reproduces every missing digest. The affected reports are `run-05`, `run-07`, `run-08`, `run-09`, `run-11`, and `run-12`. Save those reconstructions under explicit names, record their derivation, and verify their bytes against the immutable ledger. Do not rewrite ledger history. Run-04 already resolves unchanged. Future driver runs should save the pre-reconciliation bytes before hashing them. This is an evidence preservation defect, not evidence of a cap overrun.

2. Append the omitted decisions to the canonical trail and refresh its public copy. Preserve all existing rows. The following pivots shaped the outcome and are absent between current rows 7 and 12 or after row 13. The exact transcript is `/Users/nallen/.codex/sessions/2026/10/28/rollout-2026-10-28T20-49-37-01a1840f-9b36-7d63-90e9-bb15b0b753a3.jsonl`.

   - The catalog context correction from 32768 to 262144 and the corresponding reservation change were implemented at transcript line 670. Record the failed model-verification attempt and the higher per-call ceiling.
   - The debug-to-release build correction and reduced planned call count were implemented at line 1011 after the browser startup failure. Record that early failed attempts used default-feature debug binaries. Qualify the documentation's general release-binary wording to apply to the retained acceptance workflow.
   - The independently reviewed release of the unused 117984-microdollar run-04 reservation occurred at line 1115. Link `zero-call-accounting-review.md` and `run-04-accounting-recovery.json`. This is essential context for how retries stayed within the cap.
   - The switch from fresh attempts to resumed fixtures, carrying forward completed checks and reserving two additional browser calls, was implemented at line 1267. Link the original run-12 report and the first continuation. Record that later continuations made zero new calls only after the browser workflow completed.
   - The final deletion finding, its failing regression, the `0383c96` correction, and final independent product approval are missing after row 13. Link `uncertain-fix-final-review.md`, `issue39-delete-red.log`, `issue39-release-review.md`, and the final commit. The existing row's 110-test result describes an earlier revision; final independent review records 112 passed and five ignored.

3. Replace weak evidence pointers through an appended correction. Row 3's `ssh preflight` is not a resolvable artifact. Point to the actual transcript preflight call and output. Row 11's reference to a provider request retained in fixture SQLite now points to removed state. Its run-08 report does not contain the invalid output slot or unsupported-proposal result. Actual tool outputs at transcript lines 916 and 925 preserve the stopped run and `result.txt` output-slot proposal. Point there or save a narrowly redacted excerpt. Do not change or delete the original rows.

The main `make verify` was still running when this review began. Its result is outside this verdict and must be recorded from the eventual exit status. No full-suite success is inferred from partial log output.

## Attention

reviewed by gpt-6-astra

- Preserve the six recoverable ledger evidence snapshots before handoff.
- Append the missing build, accounting, resume, and final-fix decisions. Qualify early debug attempts.
- Keep helper routing and full first acceptance open. Eight verified items do not close the ninth.
- Record the final `make verify` exit status separately.
