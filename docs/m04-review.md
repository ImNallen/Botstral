# M04 first-slice review

The reviewed change adds durable manager-owned single-turn text inference. It leaves worker task authority intact. M04 remains open.

## Independent findings and corrections

Three independent reviewers inspected the implementation at `e46fbc8`. The configured models were GPT-6 Astra, GPT-6.1 Sol, and GPT-6 Luna. Sol reported no findings. Astra and Luna identified the two corrections below. The parent confirmed both with failing regression tests.

| Finding | Evidence before correction | Correction in `33bb4e5` |
| --- | --- | --- |
| Astra found that documented standard-tier usage could not settle. | The success fixture with `usage.service_tier` returned `awaiting_attention` instead of `completed`. | A private provider usage type accepts omitted or `standard` tier metadata. Priority tiers, unknown tiers, and unsupported billing fields remain blocked. |
| Luna found that a deadline during local capacity wait retained an unsent reservation. | The capacity regression returned `awaiting_attention` instead of `not_sent`, with zero provider chat requests. | Pre-HTTP expiration returns a known unsent outcome and releases accounting. Later transport uncertainty still retains the reservation. |

Both findings are tracked in [issue 17](https://github.com/ImNallen/Botstral/issues/17). The comment review proposed three narration deletions, which were applied. No suppressions or workaround constraints remain open.

## Automated desktop compatibility finding

Subsequent PR review found that the native desktop decoder accepted only the older `not_implemented` inference marker. The manager's `single_turn_text` response therefore broke desktop sign-in. This consumer was outside the initial TypeScript checks. The issue is tracked in [issue 19](https://github.com/ImNallen/Botstral/issues/19).

The existing native test against the actual manager reproduced `Protocol` and `SignedOut`. The decoder now accepts both known markers, and its fixture uses the current manager response. All six native local-service tests then passed, including the actual-manager test. Native service formatting and clippy passed with the Tauri shell feature disabled.

## Final verdict and evidence

A fresh GPT-6 Astra reviewer inspected `33bb4e5`, the corrected call paths, and verification logs. It found no actionable code findings. This was source and evidence-log review using the supplied transcript summary, not fresh test execution or a full transcript audit.

The parent ran `make verify` before the final corrections. After those corrections, all ten focused inference tests and workspace clippy passed. TypeScript and binding checks passed, as did all 17 desktop tests. The parent also checked formatting and whitespace. The broader verification run was not repeated after the corrections.

The [acceptance record](m04-acceptance.md) maps tests to behavior. Live provider connectivity, actual account and billing behavior, process-kill acceptance during live inference, and two-VM acceptance remain unverified.

## Design decisions

Model the Domain led to a separate inference ledger because worker tasks require one worker command. Boundary Discipline kept provider metadata in a private response type. Separate Before Serializing Shared State gave implementation its own checkout and kept SQLite under one writer. Prove It Works required authenticated HTTP acceptance and failing-before regression evidence. The throughput checkpoint put design checks first, then assigned the coupled API, ledger, and transport to one owner.
