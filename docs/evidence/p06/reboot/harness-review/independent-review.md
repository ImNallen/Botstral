# P06 independent reboot review

The goal is to close the actual host-reboot locked-Vault gap before P05. The recorded fifth run closes that gap for the production manager. The rerunnable harness has one correctness issue to fix before accepting the change.

Reviewed source checkout `/Users/nallen/.t3/worktrees/Botstral/t3-d00fcd62`, diff `271f9cd72dabd2d0749027b84d355ca046de5f7e..840743b2d30ab4ce32bbd7181c2fc3beee9be37e`. I read all 16 changed files, the retained local attempt records, the supporting HTTP/WebSocket helper, and the production protected-browser request path. I made no remote calls or implementation edits.

## Required harness fix

`manager/tests/p06_reboot.py:465-478` catches the final export failure but leaves `passed=true`, cleans up the remote evidence, and returns normally. A future invocation can therefore succeed with only pre-reboot artifacts. A local mocked execution reproduced this exact outcome without network or filesystem writes. Set the recorded result to failed and raise after cleanup on export failure. Preserve any original verification exception.

The regression artifact is `.audit/p05/p06_export_regression.py` in the parent checkout. Run it with an optional harness pathname. Against the reviewed harness, three tests ran. The successful-export and original-failure-preservation tests passed. The export-failure test failed with `unexpectedly None`, proving that no exception was raised. This finding does not invalidate the recorded fifth run, whose complete post-reboot artifacts were independently compared.

## Evidence verdict

- The local archived source SHA-256 matches `build.json`. Every one of its 2,382 regular files exactly matches the recorded source commit through `git show`. The retained Linux binary hash matches `build.json`, `resources.json`, and both running-process identities. The harness and support-helper hashes match the recorded values.
- Every raw file under `.audit/p06-reboot/run-5` matches its published evidence file byte for byte. The boot ID and systemd invocation changed. The enabled service was running. Its binary and credential ciphertext were unchanged. The enabled unit contains no passphrase or automatic unlock configuration.
- `exercise()` checks the Vault is locked before creating the new task. The new worker handshake, recovery, task approval, claim, and permit establish fresh authority. Lines 338-349 send the same `prepare` request before and after explicit passphrase unlock. The first returns 403. The second yields an awaiting-approval review. The production path validates the browser parent and worker fingerprint before calling `credential_identity()` in `uses/preparation.rs:21`. Thus the refusal is supported as a lock refusal rather than a stale-authority artifact.
- The item metadata, bot restriction, origin, revision, and every-fill policy survive. Unapproved release fails. Explicit approval permits the saved values. The controlled HTTPS destination accepts those values exactly once. Repeated release fails. The evidence explicitly states `browser_executed=false` and `synthetic_worker=true`; the documentation does not claim real browser filling or production agent supervision.
- Fixture paths are random, private, and constrained to the owned guest prefix. The controller restricts the SSH target. Inspections reject unrelated UID 1000 processes, unexpected running services, and held package locks. Cleanup checks unit contents, stops and disables the unit, checks inactive state and zero PID, removes the unit, checks it is unloaded, and removes the owned root. Recorded run-5 cleanup passes. This is a guard for the named test guest, not a general workload detector for arbitrary machines.
- Published records contain metadata, hashes, synthetic identifiers, process inventories, and two `manager ready` log lines. They contain no private fixture file, private key, passphrase, password value, device credential, or bootstrap token. The harness also checks synthetic values against manager logs and redacts exported artifacts. Private values were removed with the fixture; exact-value leakage cannot now be independently rescanned against those deleted values.
- All four earlier failures remain marked failed in their local ledgers. Attempts two through four retain their stderr files. Attempt one retains its failure string, partial pre-reboot artifacts, and cleanup result. The published attempts summary preserves all four and explicitly excludes them from acceptance. The local attempt-two cleanup failure log also remains, although its later successful cleanup is the recorded final state.

## Optional robustness improvement

Initial remote directory creation and script copy occur before the controller's `try/finally` at lines 430-432. A failed initial copy can leave the owned empty or partial directory behind with only the initial resource ledger. Moving setup under cleanup handling would improve failed-run hygiene. No service or credential exists at that stage, and this does not affect the accepted run.

The controller records the current checkout commit but relies on its documented caller requirement that the supplied binary came from that source. The recorded run satisfies this requirement through the archived source and build evidence. Reuse should preserve that build evidence.

## Verification and comment review

Python AST parsing and `git diff --check` passed. Source checkout status remained clean. The Prove It Works principle changed the review from trusting summaries to hashing the real binary, comparing archive bytes to Git, comparing published evidence to raw files, and reproducing the export failure.

Comment review covered the full added script. Deletion count is zero. `MUST KILL` count is zero. `HOLD` count is zero. The only comment token is the executable shebang. The module docstring supplies the public CLI description. No narration, commented-out code, suppressions, or dependency-rule justifications were added. Generated logs and evidence prose were not treated as source comments.

Root owns the already identified stale introductory PLAN and P06 wording. No additional blocker was found in the recorded reboot acceptance evidence.
