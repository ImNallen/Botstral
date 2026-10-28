# M06 integrated review

**Intent.** One owner installs each connector once and grants tools separately to bots. Remote HTTPS and worker-local stdio use one current manager authority. Bot work uses exact-approved tasks, and Vibe uses its existing process scope. Revocation, rotation, and uncertain effects fail closed. Backups restore encrypted credentials, history, Markdown, and artifacts. Manager and worker remain separate Linux services and OCI images.

**Verdict: PASS.** Reviewed `48bf1bb2cc917499a6889f24519426f5f0666755`, diff `4483b183671aec5bc36a65fa11c25308fb127423..48bf1bb2cc917499a6889f24519426f5f0666755`. No verified findings.

## Reproduced checks

Commands and observed output:

- `cargo test -p botstral-manager manager::state::connectors -- --nocapture` printed `17 passed; 0 failed`.
- `cargo test -p botstral-manager manager::connectors::http::tests -- --nocapture` printed `1 passed; 0 failed`.
- `cd manager/packaging && python3 -m unittest test_recovery -v` printed `Ran 8 tests` and `OK`.

These runs cover the authority and revision checks, lost-reply and restart behavior, unknown effect handling, parent-run stopping, bounded HTTP protocol behavior, and backup integrity and restore refusal paths. The failed initial Python invocation from the repository root was a test import-path mistake; rerunning from `manager/packaging` passed. It is not a product finding.

## Evidence and rejected hypotheses

- The canonical `docs/m06-decisions.tsv` references are present and internally consistent with the linked docs and retained logs. I read the available parent transcript through its final item. The transient `/tmp` evidence cited by the decision log is present in this environment. Its acceptance records preserve failed attempts and distinguish those from the passing reruns.
- The integrated report identifies the exact source archive, manager fixture and worker binaries, separate Linux hosts, observed effect counts, restoration, Vibe process membership, cancellation, and cleanup. I found no evidence that it claims Docker x86_64, Podman, OAuth-only authentication, cross-root atomic restore, or replacement of installed production services.
- The apparent limits around separate official filesystem package evidence, retained-worker restoration, external TLS trust, manager fixture inference, and non-atomic multi-root restore are explicitly documented. They do not contradict the stated M06 contract.
- The snapshot summary adds only bounded connection metadata. The updated serialized bound allows 1 KiB per connection, and each summary carries a label limited to 100 UTF-8 bytes plus fixed-size fields. I found no bound regression in this path.

## Review limits

I did not repeat broad workspace or live deployment checks. Existing integrated Linux evidence was inspected, not rerun. The T3 transcript was available through its last returned item; no transcript access limitation affected this review. No production files were changed. The interrogate and no-comments workflows normally fan out reviewers, but the task explicitly prohibited nested agents, so this was a single independent pass.
