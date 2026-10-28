# M05 correction review

Approved for the five production corrections at `ef2b09e291eeef0a02c5639341dd7a426927fbce`. No production blocker found. This is not final milestone acceptance: final-source repository checks, native acceptance, two-VM public-relay acceptance, and their final evidence validation remain pending.

Reviewed the complete `3de495e..ef2b09e` correction diff, accepted architecture, all three original review reports, review synthesis, and relevant client, TLS, owner transport, State queue, device admission, worker enrollment, provider verification, and regression paths. The original panel scope was `b72bd5d..3de495e`. Production files remain unchanged from the correction head during this review.

## Correction verdicts

1. **Client storage ownership is fixed.** `OwnerClient::connect` acquires the sole session permit before authentication. `ClientLease` retains both that permit and the shared locked file through authentication and the returned session's lifetime. Failed or cancelled authentication releases admission through ownership. Dropping the client does not release the directory lock while its session lives. `recover` checks the originating lock allocation before sending any recovery request or installing state. The native contract checks second-session refusal, session-outlives-client locking, successful use of that surviving session, reopening after session destruction, and cross-client recovery rejection.

2. **Mutation failure classification is fixed.** A transport failure during send becomes `UnknownOutcome`; all failures after successful send become `UnknownOutcome` except `Refused`. `receive_reply` constructs `Refused` only for an error carrying the current request identity. Storage, malformed transfer metadata, decoding, digest, stream, and timeout errors therefore cannot imply definite rejection. Reads preserve their own errors. Arbitrary mutations are not automatically retried.

3. **Transient TLS retry is fixed without weakening trust.** The pinned verifier still performs chain, name, validity, exact leaf, and handshake-signature verification before the WebSocket/application exchange. Certificate failures and missing certificates map to terminal `Trust`. The locked Rustls version maps cryptographic signature verification failures to `InvalidCertificate`, including `BadSignature`. An early `AlertReceived(InternalError)` maps to `Network`; the watch loop retries with the existing trust profile and bounded backoff. No error path imports or substitutes a pin.

4. **Anonymous failure budgets no longer exclude valid credentials.** Owner authentication/enrollment and worker invitation redemption first execute their real credential checks and atomic admission. Only failures enter `remote_attempt`. An exhausted failure budget consequently cannot change a successful owner or worker result. Existing connection, queue, invitation, expiry, role, and revocation checks remain. The retained before-test log shows the real `RateLimited` failure; the corrected test admits both owner and worker credentials.

5. **Provider verification no longer holds materialization admission during network waits.** The branch drops the incoming permit before preparing/calling the provider, retains provider-call capacity during that request, and reacquires materialization admission before State completion. It rechecks owner authority before committing the result. `MaterializedOwnerReply` carries the reacquired permit into serialization. Other operation replies and detached blocking artifact work retain their permit through the same boundary. The transport no longer retains an extra clone across provider verification. The provider-gate test blocks the real provider fixture, admits another owner's replay, then releases verification and checks its configuration result.

Cancellation still drops owner jobs and network waits. Blocking artifact and serialization closures own their permit, so abandoning their async waiter does not admit another hidden blocking materialization. Refusal transmission happens after the operation's permit is released. The changed production lines introduce no narrative comments, workaround comments, or suppression annotations.

## Independent executions

`python3 manager/tests/m05_client_regressions.py --output /tmp/botstral-m05-independent-client-corrections` passed against the current real binaries. Its retained logs and summary cover exclusive storage, surviving-session lock ownership, a committed mutation whose staging creation fails, cross-client recovery ownership, and an unauthenticated fatal TLS internal-error alert followed by healthy pinned recovery.

Three temporary pinned-TLS response fixtures extended the mutation check against the real Manager and native probe:

- `/tmp/m05-correction-invalid_transfer.py`: the fixture committed the message, sent invalid transfer metadata, and observed `Error: UnknownOutcome`; the authoritative Manager snapshot contained the message.
- `/tmp/m05-correction-uncorrelated_refusal.py`: the fixture committed the message, sent a valid Busy refusal for another request, and observed `Error: UnknownOutcome`; the snapshot contained the message.
- `/tmp/m05-correction-correlated_refusal.py`: the fixture sent Busy for the current request without dispatching the mutation and observed `Error: Refused(Busy)`; the snapshot did not contain the message.

All temporary fixtures stopped and their private credential roots were removed. These additional fixtures are review evidence, not committed regression coverage.

I inspected the retained correction library, authentication, provider-gate, and Clippy logs and matched their invocations/results to the root transcript. I did not claim independently rerunning the full library or repository suites. The provider regression was added after the fix; no before-fix execution is claimed for it.

## Decision-trail audit

Read `docs/m05-decisions.tsv`, the available root transcript export, the earlier parent design trail, and relevant initial/correction evidence. The export has 177 activity items, of which 23 are truncated. This review does not claim exhaustive coverage of omitted text or a complete implementation-agent transcript.

The root transcript supports design selection and temporary relay choice, independent review acceptance, the failed valid-owner regression, the permission repair to that test fixture, the correction implementation, and the focused verification commands. The placeholder timestamp correction remains explicit and append-only. Initial acceptance names the earlier production revision; the review synthesis explicitly leaves corrected-source acceptance open. I found no claim that the current pending acceptance has completed.

The parent corrected two evidence gaps surfaced during review. An appended audit row now points to the preserved earlier design/route decisions and `initial/path-map.json`; every mapped inherited log is byte-identical to its original retained file. The untracked evidence validator now requires exact local/remote gate names, exact endpoint keys with true values, and complete remote binary keys instead of accepting mere counts or empty maps.

## Attention

reviewed by gpt-6-astra

- **Final acceptance remains open.** `docs/m05-decisions.tsv` review row at `2026-10-27T21:48:26Z` and `docs/m05-review.md` correctly preserve this gate. Do not use the initial `e2547e1` acceptance as proof for `ef2b09e`. The current full repository run and final-source native/VM runs must complete, with cleanup and final manifests checked.
- **Finish the evidence validator's source coverage and rerun its controls.** Its reviewed version omitted component Cargo manifests and Agent sources from `production_objects`. The parent accepted expansion to `manager/Cargo.toml`, `link/Cargo.toml`, `agent/Cargo.toml`, and `agent/src`. Verify that expanded contract against the final manifest, then execute changed-log, failed-acceptance, changed-source, replaced-gate, and missing-endpoint controls. This is pending evidence work, not a production-code rejection.
- **Review scope is bounded.** Twenty-three transcript items are truncated. Initial log preservation and focused source review support the checked claims; they do not establish an exhaustive transcript audit. Malformed-transfer and refusal-correlation checks passed as temporary review fixtures but are not retained regression tests in the repository.
