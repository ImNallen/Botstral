# Final P01 decision-trail audit

The goal is a reviewable record for the first bounded P01 memory workflow after helper acceptance. No false success or material unlogged implementation pivot was found. One public evidence-navigation gap remains.

## Attention

reviewed by gpt-6.1-sol

- The committed `docs/p01-memory-decisions.tsv` retains ignored `.audit/p01-memory/` pointers and undefined `owner ...log` pointers. Its design row does not give a public route to the selected design or judge. `docs/evidence/p01-memory/proof-commands.tsv` names `verify-final.log`, while the published file is `repository-verification.log`; several earlier logs are not published. The relevant final proofs exist and support the claims, but a reviewer cannot resolve the trail literally from the published checkout. Add an append-only public evidence mapping or superseding row. Do not rewrite the original history. This is a traceability gap, not a correctness finding.

## Checks and results

The supplied owner export contains all 126 items with `hasMore=false`, `truncated=0`, and no item text truncation. The supplied root export contains all 84 items from positions 158 through 241, following position 157, with the same completeness indicators. I parsed and searched complete item texts for commands, results, pivots, and publication actions instead of trusting handoff summaries.

Owner command 104 runs the final `make verify` with the isolated Cargo target and existing `BOTSTRAL_MANAGER_BINARY` override. Its activity status is completed. Commands 119 through 122 preserve successful final output and clean head `109112c`. The published verification log contains 127 passing manager tests, five ignored manager tests, 15 packaging tests, feature recovery results, and the final production binary/refusal guard success. The earlier missing-packaging-binary failure and override are recorded in the owner trail. The export records command completion/failure status rather than a separate numeric exit-code field. This supports successful completion but does not independently expose a numeric exit code.

Root command 228 executes the focused Cargo selection. Root command 229 executes the nine recovery tests and checks the imported tree against the owner's head. Published logs show 15 focused passes and nine recovery passes. Root commands 230 and 232 inspect results and owner proof records. Both security regressions genuinely failed before the fixes. Owner commands 92 and 98 have failed status, and their preserved logs show an admitted Ready run and a returned claim permit, respectively. The final focused run passes both named regressions.

I independently recomputed every SHA-256 in the public manifest. All 11 entries match, and no evidence file is unlisted apart from the manifest itself. Git independently resolves both `109112c3632474f073ef725dfc0e1bf0fc35fa1f` and `c746a1aceaa856847ab3672211a2708eca48e3fe` to tree `872a0872008ff5c4cef42960291833d67bd0deef`, matching `implementation-tree.json`. Documentation/evidence additions are subsequent work and are not represented as part of the fully verified implementation tree.

The routine transaction adjustment is disclosed in owner commentary at item 32, owner decisions, root reconciliation row, and independent review. That review explains serialized state ownership, in-transaction cumulative budget/capacity checks, atomic run/occurrence/reservation/receipt insertion, and separately committed authoritative-document reconciliation. The raw-document escaping check and later key-rotation check are also recorded. I found no concealed broadening to historical retrieval, bot management, public protocol, UI, paid inference, or release work.

The original judge artifact is present. It contains the 15/13/11 scoring rubric and explains the selected immutable column, complete PreparedRun package, migration/recovery requirement, and two test grafts. These are design judgments, not measured runtime scores.

Root command 229 records scanning 54 tracked changed files for the current supplied key, checking the earlier provided key path was absent, unlinking the current key file, and asserting its absence. The public cleanup metadata matches that action. I did not open any secret file or retrieve credentials. This validates the recorded cleanup action, not a fresh filesystem or historical-secret scan.

## Scope and limits

This is an independent trail audit, not another implementation review or test run. I checked the existing independent correctness report and its stated limits. Local captured-provider delivery does not establish stochastic real-model adherence. Full P01 remains open. The full suite retains existing ignored opt-in tests. The owner trail uses a compact three-column format rather than the canonical six-column root format; item chronology still supplies provenance. This procedural difference did not conceal a material decision.

The root transcript stops at export position 241. Later publication, review responses, or document changes are outside this audit. Only this audit report was written. No provider, infrastructure, secret-file, or subagent action ran.
