# Derive authority from retained reviews; admit one fill per parent

## Usage (caller's view)

Keep the protected-browser wire protocol unchanged. The manager owns retry identity, profile provenance, and the screening inventory. A parent Computer task contains exactly one ProtectedLogin operation, so it admits one destination and one fill. RememberPassword can replace its initial review once, without extending its deadline.

```rust
// The existing runtime retries a lost Prepare response unchanged.
let first = call(Prepare { destination: destination.clone(), previous: None });
let retry = call(Prepare { destination: destination.clone(), previous: None });
assert_eq!(first.id, retry.id);
assert_eq!(first.release, retry.release);
assert_eq!(first.expires_at, retry.expires_at);

// The existing remembered-password flow has one explicit successor.
owner.decide(first.clone(), RememberPassword);
let remembered = call(Prepare {
    destination: destination.clone(), previous: Some(first.id),
});
// Repeating this request returns remembered, including its terminal state.
// It never issues another review, approval, release, or deadline.

// The worker still supplies its conservative, durable possible-release history.
call(Screen {
    protection: Tracked { profile, releases: possible_releases, complete: true },
    text,
});
// Manager checks that history against its own emitted inventory before scanning.
```

A second fill, including a second TOTP code, requires a new admitted parent task and fresh TOTP approval. Returning an old Released or Applied review is not permission to refill; Release still requires Authorized and consumes that state before returning bytes. Changed page, document, field, profile, or any other destination field requires a new parent.

## Problem and grounding

At source 13580e4, `uses.rs::screen_profile` validates only claimed release rows, then invokes the global secret scanner. Empty claims reach unrelated provider, connector, login, and code secrets. `prepare_credential_use` allocates new identifiers and charges the 4096-row limit for every valid call. `previous` matters only when item revisions differ.

The existing durable representation is sufficient for this correction. Each review contains exact destination, task binding, incarnation, item epoch, purpose, and state. Retained task commands identify the assigned worker. Released, Applied, and Uncertain are evidence that the manager committed possible emission. Current paths only enter Uncertain from Released. Denied, Revoked, Expired, Refused, AwaitingApproval, and Authorized do not establish emission. Release commits the issued code and Released state together before sending its reply. Task completion does not delete task commands; the inspected task code has no task-row deletion path.

The relevant live tests cover exact release, remembered revision handoff, rollback before code issuance, retained password screening, owner-device revocation, and release transaction rollback. Several tests currently expect fresh preparation after consumption or revocation in the same parent. Those expectations encode the behavior being corrected and must change deliberately, not be preserved by weakening idempotency.

## Shape

Keep all policy in `manager/src/manager/state/credentials/uses.rs`. Retain existing epoch decryption in `credentials.rs`, the representation matcher in `provider`, and the unchanged worker transport. No wire or database migration is needed. Do not add a synchronized profile ledger or in-memory retry cache.

These are private domain sketches, not exported transport aliases:

```rust
/// Constructed only after current browser_parent and certificate checks.
struct BrowserCaller {
    worker: WorkerId,
    task: TaskView,
    authority: BrowserAuthority,
}

/// Keyed broadly enough that changing a fence, revision, or destination cannot
/// buy another slot within the admitted parent. Exact authority is checked too.
struct ParentUse {
    initial: CredentialUseReview,
    remembered: Option<CredentialUseReview>,
}

/// Only these rows may contribute plaintext to a profile's scanner.
struct EmittedUse(CredentialUseReview);

/// Built from all retained manager reviews and their retained task commands.
/// Construction rejects missing, corrupt, or inconsistent provenance.
struct RecordedProfile {
    worker: WorkerId,
    profile: ProfileEpoch,
    emitted: BTreeMap<ReleaseId, EmittedUse>,
}

/// Constructed from complete=true, bounded input, and set comparison.
/// A claim is not a release capability or a source of scanner secrets.
struct CoveredProfile(RecordedProfile);

enum PrepareTransition {
    Return(CredentialUseReview),
    Initial,
    Remembered { prior: CredentialUseReview, source: FillAuthority },
}

fn recorded_profile(&self, caller: &BrowserCaller, profile: ProfileEpoch)
    -> Result<RecordedProfile, OperationError> { todo!() }
fn cover(profile: RecordedProfile, claimed: Vec<ReleaseId>, complete: bool)
    -> Result<CoveredProfile, OperationError> { todo!() }
fn parent_use(&self, caller: &BrowserCaller)
    -> Result<Option<ParentUse>, OperationError> { todo!() }
fn prepare_transition(&self, caller: &BrowserCaller, destination: &LiveDestination,
    previous: Option<CredentialUseId>, existing: Option<ParentUse>)
    -> Result<PrepareTransition, OperationError> { todo!() }
fn screen_profile(&self, caller: &BrowserCaller, protection: ProfileProtection,
    text: Zeroizing<String>) -> Result<ProtectedBrowserReply, OperationError> { todo!() }
```

`BrowserCaller` is constructed once inside the existing entry point and passed privately. The public interface remains one operation dispatcher. This concentrates the policies behind a small surface, per boundary-discipline and minimize-reader-load. The typed covered inventory prevents accidentally feeding unvalidated claims into the scanner. The database remains the single source of truth, per make-operations-idempotent and encode-lessons-in-structure.

### Profile provenance and coverage

Read the complete retained `credential_uses` inventory, bounded by the existing global limit, not the owner display's last 128 rows. For every row whose destination profile matches, load its retained task command and validate its task/execution association. All such rows must identify the authenticated caller's worker. A profile with no recorded rows is unknown and refuses. A profile shared by conflicting workers refuses; Prepare also refuses introducing another worker's existing profile identifier. Missing parent records or malformed matching history refuse. Never interpret an inner-join omission as an empty inventory.

Profile identity is consequently anchored by the first accepted manager review, with ownership derived from retained task commands. This does not authenticate the worker's claimed browser filesystem. It does stop another worker from selecting a known profile UUID and scanning that worker's history. Within one worker, access is intentionally to its retained profiles; that worker has already received their values. Historical worker revocation is not checked for old rows. The current caller must still be an authenticated, live, nonrevoked worker execution.

Use states Released, Applied, and Uncertain to derive the emitted set. Include every matching historical epoch, across tasks and revisions, irrespective of current item state, device grants, or old execution deadlines. Never call `use_authority` on these historical reviews. New task activity on the same worker can screen old profile releases after old task expiry, item removal, device revocation, or recovery. If required provenance has actually been deleted, refuse the whole screen; do not omit that release and return text.

Reject oversized raw lists before allocation. Convert the claimed list into a set and require exact equality with the manager's emitted set; duplicate IDs are harmless set duplicates and cannot satisfy missing entries. Unknown IDs, another profile's IDs, and missing actual emissions refuse. A claimed but un-emitted release also refuses. This deliberately preserves conservative restore/lost-request behavior: the worker cannot obtain a successful screen when it has recorded possible release but the manager has no committed emission. The existing pre-issuance TOTP refusal test remains valid. A known owned profile with no emissions and an empty claim succeeds for any otherwise valid text without consulting any secrets. An unknown profile refuses independent of text.

```text
screen(caller, claim, text):
    require bounded text and Tracked(complete=true)
    inventory = recorded_profile(caller, claim.profile)
    covered = cover(inventory, claim.release_ids, true)
    for emitted in covered.emitted:
        require retained item epoch exists and decrypts successfully
        if password emission:
            reject when reflects_secret(text, old_username or old_password)
        if TOTP emission:
            require retained code row exists and decrypts in review code_context
            reject when reflects_secret(text, issued_code)
    return original text
```

The epoch decrypt check also preserves missing/corrupt history refusal for TOTP. Its seed, password, and username never enter a TOTP-only scanner. A password release screens its emitted username and password, not the seed. Codes use the exact retained issued bytes, not a newly generated current code. Past password epochs remain in scope even after later rotations. An unrelated secret equal to an emitted value necessarily still matches; the test is value-based, not evidence of the unrelated credential's existence.

Release must also reject a profile already recorded under a different worker. Use the same provenance reader, without requiring Screen's complete emitted-set equality: Release's list can include the pending release, and its existing consume-before-bytes contract remains unchanged. Screen, rather than caller coordination, enforces capture completeness.

### Prepare admission

Resolve existing history by the admitted task identifier before allocating identifiers or checking global capacity. Verify exact execution binding, fence, incarnation, item identity, purpose, and destination against the existing row. A changed destination is a conflict, regardless of terminal state or a changed `previous`. Do not key admission by destination, item revision, or caller nonce, because each can be varied to evade the bound.

The initial request must have `previous=None`. Its immutable identity is the initial review. Same-body retries return that review after existing reconciliation; they never apply a newly discovered remembered rule to upgrade the old review. Terminal reviews remain terminal. Reconciliation may downgrade Authorized to Revoked or Expired, but never restores approval.

The sole successor request has `previous=initial.id`. It is permitted only for the existing RememberPassword transition: password purpose, initial Revoked, exact authority and destination, same item epoch, current revision exactly initial revision plus one, matching allow rule naming that initial ID, appropriate policy/certainty, and still-valid recorded approval provenance. Additionally, `now < initial.expires_at` is required. Its deadline is the initial deadline, also bounded by current parent deadlines. This prevents the revision handoff from renewing an expired review.

If the successor already exists, the identical successor request returns it after reconciliation, including denial, expiry, consumption, or revocation. A second successor is forbidden. A bad `previous` never falls through to initial creation. An initial retry after the handoff still returns the initial Revoked row, allowing the existing runtime to follow its explicit handoff. Retrying the successor returns the successor. A revoked approving device cannot create a fresh awaiting review through that old handoff; a new parent is necessary.

```text
prepare(caller, destination, previous):
    validate current caller and structural destination contract
    load all existing reviews for caller.task.id
    decision = prepare_transition(caller, destination, previous, history)
    if Return(review):
        reconcile review; return it without any capacity check
    validate current item and complete existing use authority
    validate profile ownership against retained history
    if Initial: deadline = min(now+30, parent deadlines)
    if Remembered: deadline = min(initial.deadline, parent deadlines)
    check global capacity only now
    allocate identifiers and persist the single selected transition transactionally
    return review
```

Existing databases may already have several reviews for a parent. Never delete them or drop their emitted inventory. Classify only an unambiguous initial and optional exact remembered successor. If legacy multiplicity makes that impossible, refuse further Prepare for that parent and retain all old reviews for Screen, Poll, Release checks, and audit. Owner approval of a new parent is the recovery path. This avoids inventing a new authority by choosing an arbitrary legacy row.

### Concurrency, crashes, and retention

The serialized State actor is the only writer, so lookup, classification, and insertion cannot interleave with another Prepare. There is no second shared cache and no additional lock. Each insert and authority write remains one database transaction. A crash before commit leaves no admitted row; a crash after commit makes the retry return that row. Recovery revokes unreleased reviews rather than recreating them. Release commits consumption and any issued code before bytes, so a lost reply remains in emitted history and cannot be retrieved again.

At global count 4096, all retries of existing reviews still work without allocation; a new initial or genuine remembered successor refuses if no slot remains. At 4095, initial admission can consume the final slot and later prevent its remembered successor. That is the existing finite retention limit, not repeated-request exhaustion; no deletion or implicit capacity reset is introduced. Each new legitimate owner-admitted parent consumes at most one row, or two for RememberPassword. New global usage can still exhaust the documented limit. This design fixes amplification by one admitted parent and does not claim infinite retention.

## Explicit invariants and verification

1. Screen never loads a provider secret, connector secret, unrelated login epoch, or unrelated issued code. Keep unrelated global public-channel screening unchanged.
2. A successful screen covers exactly every manager-committed emission to the recorded worker/profile. A missing or corrupt required record cannot produce success.
3. Released, Applied, and Uncertain remain historical emission evidence after authority expiry. No old live grant is required to screen them.
4. One parent has one exact initial destination and at most one exact RememberPassword successor. No retry extends a deadline or renews permission.
5. A TOTP fill always needs fresh approval, and one approval emits at most one code. Additional fills use new parents.

Focused manager tests should prove these outcomes, including cases that fail current code:

- Screen with a known empty profile returns unrelated password, seed, provider, connector, and issued-code candidates unchanged; random text has the same outcome. Unknown empty profiles always refuse.
- After one password release, its username/password refuse, while an unrelated login and its seed succeed. A TOTP-only profile refuses its issued code but accepts its un-emitted password and seed.
- Omitting one of two committed releases refuses even for safe text. Duplicate IDs do not conceal omissions. Unknown, cross-profile, and another worker's profile histories refuse.
- Historical same-worker screening under a new live task works after old task expiry, item rotation/removal, grant revocation, and manager recovery. Missing epoch, code, parent row, and malformed history all refuse.
- Thousands of identical Prepare calls preserve IDs, deadline, and row count. Thousands of altered destinations preserve row count and refuse. Test profile changes too.
- Authorized, denied, expired, revoked, released, applied, and uncertain retries never create authority. Invalid `previous` cannot allocate.
- RememberPassword creates exactly one successor, with the original deadline. Repeated successor requests, requests after its consumption, and requests after device revocation never create another. First successor after original expiry refuses.
- At global capacity, identical initial and successor retries still return their rows. New admissions refuse without deleting retained records. Failed release transactions still roll back both code and consumption.

Update the existing fresh-TOTP and revoked-device retry tests to use new admitted parents. Update `allow_rule()` fixtures to establish remembered policy through an earlier parent, rather than expecting a repeated Prepare to silently replace an existing AwaitingApproval review. Run these focused live tests and the existing controlled-browser remembered handoff before implementation is declared complete. No tests were run for this design artifact.

## Synthesis decision and alternatives considered

Choose the retained-review design as the complete base. It derives both policies from records the manager already owns and changes no public surface. No parts are grafted from another runner at this stage.

The structurally distinct alternative is a dedicated durable profile ledger plus durable parent admission records. Prepare would transactionally claim a worker/profile and admission slot; Release would append immutable emission rows containing worker, profile, item epoch, purpose, and issued code reference. Screen would query only that ledger, while admission records point to initial/successor reviews. This hides policy behind an equally small interface and allows task-row deletion independently of credential history. It loses here because it requires a migration, historical backfill rules, and synchronized review/ledger transitions despite existing retained rows proving the same facts. It is the better future shape if task deletion or mutable worker assignment becomes a requirement.

A client-supplied idempotency token plus signed profile manifest is not selected: varying tokens still needs a manager parent bound, signatures do not establish that omitted releases are absent, and manifest refresh would expose coordination to callers. It adds surface without replacing the manager inventory.

## Tradeoffs accepted

- We accept a fresh parent for changed destinations and additional fills in exchange for a strict, understandable per-parent abuse bound.
- We accept a bounded scan of at most 4096 reviews in exchange for no cache, index migration, or duplicated authority store.
- We accept refusal after conservative worker history gets ahead of manager emission in exchange for preserving fail-closed capture behavior.
- We accept dependence on retained task commands in exchange for avoiding a provenance migration. Future deletion must retain equivalent provenance or keep screening closed.

## Implementation reconciliation

None. This artifact is a sketch only. Existing fixture expectations identified above are intentional contract changes, not permission to mint replacement authority.

## Open questions and risks

- Could future task reassignment mutate the recorded worker? If introduced, worker provenance must become immutable retained emission data before reassignment ships.
- Could future use states lose emission evidence? Any new transition out of Released must preserve the emitted classification or introduce an immutable emission record.
- Does a future product require same-parent destination retry? That requires an explicit bounded owner-authorized transition, not relaxing the admission key.

This protects the stated manager boundary and supported representation checks. It does not authenticate arbitrary worker filesystem claims, protect against full guest privilege, or recognize arbitrary secret transformations. Prepare's existing global public-destination collision check is outside this scoped Screen correction; this design does not claim to eliminate every possible membership signal in the application.

## Next implementation step

Add the focused failing Screen and Prepare tests, then implement the private retained-inventory and admission classifiers in `uses.rs` without changing the wire schema.
