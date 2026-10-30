# PR56 security design candidate

## Problem

`Screen` currently treats worker-supplied `Tracked { profile, releases, complete }` as evidence, then scans every retained manager secret. The live task and worker are authenticated, but neither proves that the caller's history is complete or belongs to it. `Prepare` also inserts a fresh durable review on every call and counts terminal reviews toward 4096. The design must make manager-recorded releases the source of truth, preserve old release coverage, and make retries idempotent without weakening consent, exact destination checks, or RememberPassword's single revision handoff.

## Usage (caller's view)

The worker still uses the existing private endpoint. Prepare's request shape remains stable; retrying the same admitted operation returns the existing review. A changed destination uses the same task admission but is charged against a bounded per-parent budget.

```rust
let review = manager.prepare(authority, destination.clone(), previous)?;
let decision = manager.poll(review.id)?;
let values = manager.release(review.id, destination, profile)?;
manager.settle(review.id, applied)?;
let safe = manager.screen(authority, profile, observed_text)?;
```

After a `RememberPassword` decision revokes the first review and advances the item revision, the runtime does exactly one `prepare(authority, same_destination, Some(old_id))`. Same-body retries return that second review; retries after expiry or terminal state return its existing state or refuse, never mint a new authority.

```rust
// A task retry with the same body and execution identity.
assert_eq!(manager.prepare(auth, dest.clone(), None)?,
           manager.prepare(auth, dest, None)?);

// A different destination under the same parent is bounded.
manager.prepare(auth, dest_a, None)?;
manager.prepare(auth, dest_b, None)?; // admitted only within parent budget

// Screen derives history from manager records, not a worker-provided release list.
manager.screen(auth, profile_epoch, text)
```

## Shape

Choose a manager-owned per-worker profile ledger. Keep wire `ProfileProtection` for compatibility during this change, but parse it at the endpoint as a profile claim only; ignore `releases` and `complete` for authority. `Unknown` still refuses. Screen is bound to the authenticated `BrowserAuthority` worker and the live Computer parent. It reads only manager-recorded possible emissions for that `(worker, profile epoch)` and checks only those login epochs and retained issued TOTP codes. It does not call `reject_known_secrets` on this path; global public-channel scanning remains unchanged.

```rust
// State-actor-owned, serialized in the manager DB.
struct ProfileLedgerKey { worker: WorkerId, profile: ProfileEpoch }
struct ProfileLedger { releases: BTreeSet<ReleaseId> }
struct ProfileRelease { key: ProfileLedgerKey, release: ReleaseId }

struct PrepareKey {
    task: TaskId,
    execution: ExecutionId,
    fence: InputFence,
    journal: JournalId,
    item: CredentialId,
    purpose: CredentialFields,
    destination: LiveDestination,
    prior: Option<CredentialUseId>, // only the explicit revision handoff
}
struct ParentPrepareBudget { task: TaskId, used_destinations: u16 }

impl State {
    fn prepare_credential_use(
        &mut self, authority: &BrowserAuthority, destination: LiveDestination,
        previous: Option<CredentialUseId>,
    ) -> Result<CredentialUseReview, OperationError>;
    fn release_credential(
        &mut self, authority: &BrowserAuthority, id: CredentialUseId,
        destination: LiveDestination,
    ) -> Result<ProtectedBrowserReply, OperationError>;
    fn screen_profile(
        &self, authority: &BrowserAuthority, profile: ProfileEpoch,
        text: Zeroizing<String>,
    ) -> Result<ProtectedBrowserReply, OperationError>;
}
```

`PrepareKey` is a lookup key over the existing durable review rows, not a new authority object. Persist a unique key/index for active-or-retryable review identity, or scan the task's bounded rows if schema simplicity wins; ensure lookup occurs before global capacity accounting. Destination identity uses full `LiveDestination` equality. Keep RememberPassword's prior-ID path distinct: accept only its revoked review, exact same parent/destination/item/epoch/purpose, exactly one revision advance, and matching rule cause; then record the successor key once. A deny, expired review, consumed release, or other terminal state cannot be resurrected.

`ProfileRelease` is inserted atomically with transition to `Released`, before reply bytes. It means “manager may have emitted these values,” so lost replies remain protected. It is keyed by worker as authenticated from the task, not by a worker-asserted owner field. A release ID is globally unique; duplicate ledger inserts are idempotent and conflicting profile associations refuse. Empty ledger is valid for a profile epoch because it is manager-observed emptiness. An unknown profile epoch has no history and can screen text against an empty set; this reveals no unrelated secret membership. A different worker's profile claim cannot retrieve its ledger because the worker key comes from authenticated task authority.

For each ledger release, load the corresponding review by release ID, require profile equality and internally consistent worker/task metadata, decrypt the exact recorded item epoch, and for TOTP require its retained issuance row. Never require current task, item, device grant, or owner approval validity for historical screening. Missing ledger/review/epoch, corrupt serialization, failed decryption, or missing TOTP code refuses. De-duplicate release IDs on write and read. Claimed unknown, duplicate, omitted, or extra release IDs are irrelevant because the manager ledger controls coverage. Retain all records under documented global limits; do not prune to admit new work.

Prepare pseudocode, inside the serialized State actor:

```text
authenticate parent; validate destination and task operation
derive PrepareKey from manager task + exact request
if exact prior attempt exists: return its existing review (even near capacity)
if previous is present: validate the sole RememberPassword successor transition
else: charge one new destination to this task's bounded admission budget
if budget exhausted: refuse; do not discard history
validate current use authority, policy, origins, plan, and live item snapshot
check global credential_uses capacity
transaction: insert review and key/budget update; commit; return review
```

The key must collapse same-body retries, including after a response loss, before capacity accounting. Altered destinations consume a small finite per-parent budget (for example, eight distinct keys), with one handoff slot tied to the prior revoked review. Budget and review insertion share a transaction. Since State owns the SQLite connection and serializes calls, concurrent duplicate requests see the first committed row; a crash commits both or neither. Retrying a committed review is idempotent.

Release pseudocode keeps the exact existing gate: stored `Authorized`, supplied destination exactly equals stored destination, fresh `use_authority`, current item snapshot/policy, then atomic code retention + `Released` + profile-ledger insertion before returning bytes. Preserve fresh TOTP issuance per separately approved fill, consume-before-bytes, and exact destination identity. The profile is taken from the stored destination, so caller cannot redirect retained history.

Interface depth is high: callers supply authority, destination, profile epoch, and text; the manager owns deduplication, budget accounting, profile ownership scope, actual-emission history, decryption, and refusal on corruption. The public surface adds no new wire token. Boundary parsing turns existing transport data into typed IDs and validated domain inputs; checks stay at the manager boundary, while domain operations consume stored records.

## Rationale and explicit invariants

- Only the authenticated worker associated with the live task can read the ledger rows for its profile epoch.
- A ledger row records possible exposure, not proof that a compromised worker actually filled the DOM. This preserves conservative protection after response loss.
- Screening checks only values released to that profile and retained issued codes. Passwords and TOTP seeds are scoped to each historical epoch; codes require their exact retained issuance. Historical protection survives expiry, removal, task settlement, device revocation, and restart.
- Worker claims alone cannot expand or erase history. Empty/unreleased history succeeds only as empty manager-recorded history; corrupt or missing records fail closed.
- Same admitted Prepare is idempotent before capacity checks. Destination spam is bounded per parent. Terminal and expired requests cannot mint fresh review IDs; RememberPassword gets only its exact one-time successor.
- Approval remains per fill, TOTP consent remains fresh, release remains one shot and is persisted before bytes, and item revision handoff remains exact.
- No retention deletion is used as capacity relief. Reaching global history limits remains a deliberate owner-level limit.

## Alternatives considered

1. **Manager-issued signed profile manifest.** Release returns or records an opaque signed token that the worker presents for Screen. It can prove provenance without a DB lookup, but token lists still expose omission/completeness coordination to callers, require key rotation and replay semantics, and add wire types. The ledger keeps the manager as the only source of truth and hides more complexity behind the endpoint.
2. **Filter global scan by IDs the worker supplies.** This is the smallest implementation but preserves empty-list bypass and permits a guessed release ID from another worker to test secrets. It leaves the core authority invariant exposed to callers and loses.

## Tradeoffs accepted

- We accept a durable per-release profile association and lookup cost in exchange for manager-enforced history completeness and scoped screening.
- We accept allowing a previously unseen profile epoch to screen against an empty set in exchange for avoiding a new profile enrollment handshake; this provides no secret-membership oracle.
- We accept bounded per-task review admission and refusal after exhaustion in exchange for retaining every actual release record and preventing one parent from consuming global capacity through retries.
- We accept a small durable idempotency key or bounded per-parent lookup in exchange for response-loss retry safety.

## Verification cases

- A valid compromised worker submits empty tracked history and candidate text matching an unrelated provider, connector, or login secret: Screen succeeds because it compares only that profile's ledger; global public-channel screening tests still reject the same text.
- Profile A release screened by its owning worker is rejected when text contains that released password; an unrelated worker using A's profile ID does not load A's history.
- An authorized-but-unreleased review never enters the ledger. A password release does; an unreleased TOTP review has no ledger row/code, while a released TOTP row requires its issuance record.
- Remove/expire task, item, and device grant; restart manager; prior release still protects its exact old password epoch and codes. Missing/corrupt ledger, review, epoch, or code refuses.
- Duplicate and reordered claimed releases, empty claims, unknown claimed IDs, and `complete=false` cannot alter manager-derived coverage; duplicate internal records resolve to one release, conflicting associations refuse.
- Repeat identical Prepare before and after lost reply and near 4096: same review ID and release ID, no capacity increase. Change destination repeatedly: parent budget eventually refuses without pruning. Concurrent identical calls and crash points yield one committed review or none.
- Expire, deny, or consume the review and retry: no new ID and no renewed deadline. RememberPassword permits exactly one same-destination, same-parent successor with the same prior cause; repeated and altered handoffs refuse.
- Approval remains fresh for each TOTP fill; a second release fails; wrong destination identity fails; release state and ledger persist before returned bytes.

## Alternatives, remaining risks, and next step

This ledger is not a cryptographic proof of DOM provenance, nor protection from arbitrary code with full shared guest privileges. It records the manager's conservative decision to send values to the profile named by the validated destination. Review whether the 8-destination budget matches ordinary browser retries and whether profile epochs are guaranteed unique across worker restarts. Next, add the ledger and prepare idempotency invariants to the manager State schema and exercise the listed live tests before changing the wire shape.
