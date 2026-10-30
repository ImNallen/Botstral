The goal is protected reusable credentials and exact approved browser fills without private values in ordinary bot channels. **Reject overall P06 code acceptance at `086b1acb41226be3e5ace6055be6f765c070b8fa`.** The current-catalog correction passes this independent recheck. A separate, inherited connector-revocation defect blocks safe credential creation, metadata listing, receipt reconciliation and plain conversation. This is a demonstrated availability failure, not a disclosure or authority bypass, and not a regression introduced by the catalog correction.

Reviewed by **gpt-6-astra, high effort**, working alone. Accepted base `bf470fb00e98ed6e519b02f7a314fe48a808bdc1` is an ancestor. The checkout matched the requested head before and after testing. [Final source verification](final-state.json) records clean Git status and an empty tracked diff. No production edits, subagents, Docker operations, coordinator-file writes, pull requests, pushes or merges occurred. Temporary test includes were restored in `finally`. All review artifacts are in this checkout's `.audit/p06-catalog-recheck` directory. Prove It Works shaped the decision to exercise actual manager methods and complete conversation settlement instead of accepting source inspection or the coordinator's passing suite alone.

**R1. Revoking a connector breaks unrelated private entry and ordinary publication. Medium priority.**

[The fresh expected-safe probe](publication-probes.rs) first installs a remote connection, admits and settles discovery, grants its catalog on another live bot, completes a connector-enabled conversation, screens benign text and commits a safe login. It then calls the real `change_connection(..., true)` operation. No stored credential overlaps catalog data. No later user, model or site text introduces a collision.

Revocation correctly makes current connector inventory empty and rejects the old policy. It also creates a connection version with an incremented credential epoch and no ciphertext. The retained-secret scanner visits that version and tries to load its nonexistent ciphertext. The resulting database error becomes `Internal`.

The final probe records:

```text
revoked_epoch_ciphertexts=0
benign_scan=Err(Internal)
fresh_write=Err(Internal)
safe_rotation=Ok(Committed { ... })
metadata=Err(Internal)
receipt_replay=Err(Internal)
plain_chat=Err(Internal)
```

The expected-safe assertion fails in [fresh-catalog-complete.log](fresh-catalog-complete.log), exit 101. Its four other test functions pass. Rotation can still commit; I do not claim that it fails. The failure to retrieve an already committed receipt is separately observed. This occurs without connector tools in the new conversation and before admitting any newly colliding private value.

The causal code is `manager/src/manager/state/connectors.rs:84`, where `known_connector_secrets` reads all connection versions and decrypts every distinct credential epoch. `change_connection` advances the epoch at line 332, deliberately leaves encrypted data absent for revocation at line 339, and persists the version at line 352. `connection_secret` requires a ciphertext row at line 245. The common scanner calls it through `memory/entries.rs:142`. New credential metadata invokes that scanner at `credentials.rs:226`; receipt recovery invokes it at line 170. Catalog enumeration is not the source of this error.

[The source comparison](inherited-revocation-source.json) and [runnable source verifier](verify-source.py) prove that `known_connector_secrets`, `connection_secret` and `change_connection` are byte-identical to the accepted base. The base itself was not executed. This is an inherited defect exposed by the requested unavailable-catalog review, not evidence that the new catalog patch regressed revocation. It nevertheless blocks the requested P06 behavior on a legitimate retained store, so my overall code verdict remains rejection. The narrower catalog correction is accepted in the exercised cases.

A correction must handle the intentionally ciphertext-free terminal version while preserving every actual historical encrypted credential and failing on unexplained missing inventory. Do not skip all history belonging to revoked connections or delete retired secrets. Existing revoked stores need a regression, not only newly revoked connections. No fix is included in this review.

The focused catalog verdicts are supported by [the final fresh run](fresh-catalog-complete.log):

| Contract | Independent result |
| --- | --- |
| Owner intake across bots | Twelve cases pass. Private usernames and passwords collide with nested schema enum values, percent-encoded descriptions, connection identities, schema digests, provider aliases and random raw binding names. The login is restricted to the original bot while the catalog belongs to another bot. Every collision receives a final value-free refusal, preserves zero initial epochs and replays the same refusal with changed values. |
| Rotation and retained privacy | Every intake case also commits a safe control, refuses a colliding rotation without changing its item or epoch, and replays the refusal. A subsequent safe rotation and removal preserve complete connector and plain conversation. Original private username and password remain screened after removal, with both epochs retained. |
| Generated codes | Four additional cases pass. A current deterministic code already exists in another bot's raw binding name, nested enum, numeric schema default or encoded description before login setup and release. Two release attempts both return `Conflict`, retain zero codes and leave the use `Authorized`. Complete connector and plain conversations survive expiry and removal. These extend the permanent description/property cases. |
| Stale and unavailable grants | Permission removal, empty grants, a new grant revision and reconnect all reject old policies. Public tool inventory equals prospective inventory. A current replacement grant remains usable and its marker is refused at intake. Permission-disabled, empty-grant and needs-discovery catalogs are absent from both inventories. No currently usable catalog gap was reproduced. Revocation also filters correctly but exposes R1 in the separate history scanner. |
| Multiple live bots | Three catalogs on three distinct live bots work before testing. Disabling one bot leaves both other catalogs in prospective context. Their collisions refuse and both conversations still complete. The disabled catalog's marker may be stored because that catalog is not currently usable. Re-enabling it later is a new-context change, not a tested guarantee. |

The shared fixture is adapted from the production-method catalog fixture to accept arbitrary valid tools and distinct worker identities. It uses real install, discovery admission, settlement, grants and run methods. It supplies a synthetic discovery catalog and benign model completion, and marks the discovery parent complete as fixture setup. It does not contact an inference provider or run a real connector worker. Generated-code tests invoke the actual private-release method through the live credential fixture. They do not prove the Linux socket transport.

The original rejection and all before/after evidence remain untouched. I read `final-review-068b771/report.md`, the correction premise, the original expected-safe command records, and the renderer-only binding failure. `catalog-before.json` records exit 101 on old production and `catalog-after.json` records exit zero after correction. `catalog-binding-conversation-before.log` proves `InvalidInput` after acceptance under the intermediate renderer-only repair. Those are historical failures, not failures of this head. The current permanent catalog tests pass in my fresh full manager run.

Unchanged boundaries were rechecked as follows:

| Boundary | Evidence and limits |
| --- | --- |
| Manager authority, retained values and recovery | My full manager library run passes 171 tests with six opt-in skips. It covers paired-device revocation before and after release, remembered provenance, unrelated revocation, disconnect, peer/task fences, transaction rollback, deadline boundaries, locked restart, ciphertext binding, missing issued-code profile coverage, retired inventory and explicit pre-entry restore. It also passes numeric publication and ordinary URL/percent-encoding regressions. This is a rerun of permanent cases, not a claim that I independently mutated every authority field. |
| Exact browser destination and payload | The prior independent browser probe was rerun against this checkout's production adapter with actual Chrome. One control produced one receiving-server-validated POST. Seven field/form/input/formdata mutations produced zero POSTs and unapplied settlement. Authority callbacks are synthetic. This is not a packaged native or Linux transport run. |
| Profile inventory provenance | Three worker tests pass. The runtime probe separately confirms clear empty state and protected outcomes for possible release, metadata rollback, partial publication and a legacy marker. Coordinated arbitrary rewriting of browser files and both ledgers remains outside the guarantee. |
| Native receipt manager binding | A fresh real two-manager paired-service fixture passes. A second manager cannot close or replace the first manager's unresolved receipt. Both owned manager fixtures exit zero. Lost-reply state is injected. |
| Locked native recovery and refusal | Fresh paired-service tests pass for locked reads preserving the unlock path and final value-free collision receipts. These tests do not drive AppKit controls. R1 separately shows that connector revocation can obstruct receipt retrieval. |
| Readiness and cancellation | Source still caps readiness polling by approval/task deadlines and a monotonic maximum of 30 seconds. Launch and observation have separate command bounds. StopIntent survives completion of an in-flight start, and release rechecks deadlines. This is source review plus existing targeted evidence, not an independent Linux supervision experiment or an immediate-kill guarantee. |
| Typed context and migrations | The manager run passes complete-conversation publication coverage, all four vision cases and both corrected migration fixtures. No new serialization or migration failure was found. Finite token coverage does not establish universal context completeness. |

Commands run from `/Users/nallen/.t3/worktrees/Botstral/t3-e73580b6`. [commands.json](commands.json) contains argument vectors, timestamps and exits; adjacent logs contain full output. The fresh probe accepts a log label and optional Rust test-name filter. It asserts the exact head and a clean tracked diff, temporarily includes two test fragments, and restores both files even when Cargo fails.

| Exact command | Exit | Result |
| --- | ---: | --- |
| `git rev-parse HEAD` | 0 | Requested head. |
| `git merge-base --is-ancestor bf470fb00e98ed6e519b02f7a314fe48a808bdc1 HEAD` | 0 | Accepted ancestry. |
| `python3 .audit/p06-catalog-recheck/probe.py fresh-catalog-complete` | 101 | Four fresh functions pass; R1 fails its expected-safe assertion. |
| `cargo test -p botstral-manager --locked --lib` | 0 | 171 pass, six opt-in skips. |
| `cargo test -p botstral-agent --locked protected_browser::tests -- --nocapture` | 0 | Three worker tests pass. |
| `node agent/tests/p06_profile_provenance.mjs` | 0 | Clear control and four protected cases pass. |
| `npm ci --prefix agent/runtime --ignore-scripts` | 0 | Dependencies installed. Node 26.12.1 warns against the production pin 24.22.1. |
| `node .audit/p06-catalog-recheck/browser-probe.mjs` | 0 | One validated control and seven refused mutations. |
| `cargo test --manifest-path desktop/src-tauri/Cargo.toml --no-default-features --locked --test journal --no-run` | 0 | Native fixture build. |
| `python3 desktop/fixtures/paired_manager.py --verify-journal --journal-case p06_native_receipt_manager_binding` | 0 | Two-manager fixture passes. |
| Same command with `p06_locked_reads_preserve_the_paired_unlock_path` | 0 | Locked recovery passes. |
| Same command with `p06_collision_receipt_is_final_and_value_free` | 0 | Collision receipt passes. |
| `python3 .audit/p06-catalog-recheck/audit-evidence.py` | 0 | Completed current Linux and historical packaged/TOTP records validate. |
| `python3 .audit/p06-catalog-recheck/capture-cutoff.py` | 0 | Completed root phases and canonical trail recorded. |
| `python3 .audit/p06-catalog-recheck/verify-source.py` | 0 | Exact source, clean restoration and inherited-method equality verified. |

The three native fixture commands use `BOTSTRAL_P06_EVIDENCE="$PWD/.audit/p06-catalog-recheck/native-receipts"`, `native-locked` and `native-collision` respectively. To rerun only R1, use `python3 .audit/p06-catalog-recheck/probe.py revocation-only fresh_catalog_revocation_preserves_unrelated_private_entry_and_chat`. The same expected-safe assertion should fail on this head. The initial run already reproduced R1. The intermediate `fresh-catalog-isolated` run failed to compile because my probe passed `u64` directly to SQLite; I corrected the probe's cast. That compilation error and the initial unused-import warning are reviewer setup issues, not product findings. Their logs are preserved. No failing product expectation was weakened.

Root-gate evidence has a fixed cutoff of **2026-10-30T00:37:56.435276+00:00**. [The cutoff record](root-driver-cutoff.json) contains 13 completed zero-exit phases for `20261030T002914969649Z`, including repository verification. Desktop completion is absent. I did not use its partial log. The driver began at clean `086b1ac` with the empty-diff SHA-256. The earlier 14-phase clean `b589c76` run is complete historical evidence, not current-source acceptance. A later green desktop result cannot resolve R1 by itself.

The rebuilt [current Linux evidence audit](evidence-audit.json) passes independently of that pending gate. The complete `linux-catalog-final` records identify clean `086b1ac` and manager binary SHA-256 `523b2bf3249e285f7a0591953b07f1992833217782e1adb2689d414a608c7f96`. The build log compiles the manager release. One password POST and two freshly approved current-code submissions pass receiving-site validation through the retained runtime, systemd scope, cgroup admission, private socket and worker mutual TLS. Missing certificate, changed generation, out-of-scope same-UID peer and repeated release refuse. All 12 command records exit zero; cleanup is recorded and emulation snapshots are identical. I inspected records without operating Docker.

The combined packaged native-to-Linux proof remains historical at production `5d5da1814ad120eb5895a12fed70afa5fdfa092d`, with native binary `d914c416b66a0b364e806b811039996c6da1a37c888a5fd6ca4a1814f68e5d63`. I read its completed integration, native observation and cleanup records. They support secure fields, cleared entry, cancellation feedback, exact packaged approval, metadata-only listing, early refusal, one validated password POST, local sign-out and an absent keychain entry. I did not repeat that UI session. Native code, shared protocol and production agent code are unchanged since that source; manager checks changed. Unchanged native code does not turn the historical combined run into exact-current packaged execution. The fixture uses host loopback and no inference service.

I audited [Choose the Next Poteto Mode Step](t3-thread://v1/8fde3333-5c51-4ed5-a084-8d11ab4c2520) through bounded `t3_thread_read` calls. Direct message coverage after position 1565 includes 1577, 1581, 1604, 1658, 1679, 1682, 1686 and 1694. Activity coverage includes 1566–1585, 1621–1655 and 1687–1693. Several large payloads were truncated; I do not claim complete activity coverage. The decisive renderer-only failure at 1651 and typed-definition edit at 1653 were read untruncated. Earlier transcript claims rely on the prior review's explicitly bounded audit. No raw transcript snapshot is published. I read all 28 committed coordinator lines, all 11 implementation lines and the canonical coordinator trail through line 29. [The canonical snapshot](coordinator-trail-reviewed.tsv) preserves that cutoff.

Final Attention flags from **gpt-6-astra** are:

- **Accept the catalog correction narrowly, and preserve R1 separately.** Coordinator line 28 and transcript positions 1581, 1604, 1651 and 1653 document rejection, reproduction and the typed-plus-rendered correction. My four passing fresh functions support that correction. R1 comes from accepted-base methods and must not be mislabeled as an introduced catalog regression or hidden as a setup failure. Existing-catalog failures and ordinary future-context collisions remain distinct.
- **Keep code acceptance separate from root checks.** Coordinator lines 27–29 and transcript positions 1577, 1682 and 1694 keep source labels and gates explicit. The old broad pass, current Linux pass and 13 completed current phases are supported. Desktop is pending at my cutoff. Neither those gates nor the 171-test pass cover R1.
- **Retain the historical packaged limit.** Coordinator line 21 and the native records support the actual combined run at `5d5da18`. The latest Linux result at line 29 exercises the changed manager but does not repeat packaged AppKit execution. Do not claim exact-current packaged acceptance, public relay support or update continuity.
- **Keep the material incident and uncertainty visible.** Coordinator lines 18–20 preserve the privileged Docker emulation incident, restored registrations, unexplained cross-build keychain wait and bounded readiness correction. Matching later snapshots and fresh-profile success do not erase those facts or establish universal cold-start reliability.
- **Retain fixture and privacy limits.** Coordinator line 22 and implementation lines 3–6 and 9–11 retain persistent-cookie/code-window requirements, actual document identity, redirect refusal, retained profile inventory and native formdata validation. Native entry remains macOS-only. Only top-level native forms or one same-origin child are supported. Extra hidden controls, custom submission, redirects and deeper or cross-origin frames refuse. Site scripts can read filled values. Full guest privilege, arbitrary transformations and uncontrolled memory remain limits. Protected pixels refuse; uncertain profiles may require verified empty replacement. Existing cookies, website sessions and remote grants have separate revocation. No payment/contact, full takeover, signed distribution or session-only persistence claim is supported.

This report makes no PLAN change and grants no publication approval. Overall P06 code acceptance is rejected for R1; the tested current-catalog omission is repaired.
