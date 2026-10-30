# Manager-owned preparation slots and emission inventory

## Usage (caller's view)

The wire operations remain unchanged. The manager owns retries, profile provenance, and the set of values eligible for profile screening.

```rust
// Runtime retry after an interrupted response. The same IDs and deadline return.
let first = protected(Prepare { destination: exact.clone(), previous: None })?;
let retry = protected(Prepare { destination: exact.clone(), previous: None })?;
assert_eq!(first.id, retry.id);
assert_eq!(first.release, retry.release);

// Existing remembered-password flow. Exactly one successor is possible.
let revoked = poll(first.id)?;
let successor = protected(Prepare {
    destination: exact.clone(), previous: Some(revoked.id),
})?;
// Retrying this operation returns successor, including its terminal state.

// A later live Computer task on the same authenticated worker captures text.
let output = protected(Screen {
    protection: complete_worker_history, text: captured_text,
})?;
// Screen uses manager-recorded emissions for this profile, never global secrets.
```

## Problem and source grounding

At `13580e4`, `protected_browser` authenticates the live parent and worker certificate. `screen_profile` then trusts a supplied release list and calls the global scanner even for an empty list. `prepare_credential_use` allocates new identifiers before accounting against all 4096 retained rows. `previous` only matters when the item revision changed. Both defects are inside one manager-owned boundary, `credentials/uses.rs`.

I read the live fixtures and tests for remembered rules, retained removed values, missing TOTP issuance, device revocation, release rollback, and validity boundaries. No suites ran. Existing tests deliberately expect a second TOTP Prepare on the same parent to create a new awaiting review. That expectation must change: another fill needs another admitted parent, and therefore another fresh TOTP decision.

## Shape

Keep transport types at `protected_browser`; construct private validated domain values inside `uses.rs`. No caller retry token, new transport operation, public review field, or browser coordination is needed.

```rust
struct BrowserCaller {
    worker: WorkerId, // From authenticated task assignment, never request data.
    parent: AdmittedParent,
}
struct ParentSlotKey { task: TaskId, execution: ExecutionId }
struct AdmittedParent { key: ParentSlotKey, binding: ExecutionBinding,
    fence: InputFence, journal: JournalId }

// Existing review plus immutable manager metadata, stored together transactionally.
struct UseRecord { review: CredentialUseReview, worker: WorkerId }
struct ProfileInventory { profile: ProfileEpoch, worker: WorkerId,
    records: Vec<UseRecord> }
enum Emission {
    Password { item: CredentialId, epoch: CredentialEpoch },
    Totp { release: ReleaseId },
}
struct VerifiedProfileInventory {
    // Created only after complete inventory/ownership validation.
    emissions: Vec<Emission>,
}
enum PrepareAdmission {
    Existing(CredentialUseReview),
    Initial,
    RememberedSuccessor { cause: CredentialUseId },
}

fn authenticate_browser(&self, peer: VerifiedIdentity,
    authority: &BrowserAuthority) -> Result<BrowserCaller, OperationError>;
fn prepare_credential_use(&mut self, caller: &BrowserCaller,
    destination: LiveDestination, previous: Option<CredentialUseId>)
    -> Result<CredentialUseReview, OperationError>;
fn verified_profile_inventory(&self, caller: &BrowserCaller,
    claim: ProfileProtection) -> Result<VerifiedProfileInventory, OperationError>;
fn screen_profile(&self, caller: &BrowserCaller, claim: ProfileProtection,
    text: Zeroizing<String>) -> Result<ProtectedBrowserReply, OperationError>;
```

`UseRecord.worker` is durable ownership, separate from approval provenance. Add a private worker column to `credential_uses`, leaving the existing public JSON review unchanged. Add an index on worker and profile only if profile is stored as an indexed expression of the JSON; at 4096 records a bounded full read is acceptable. Slot lookup can likewise parse that bounded inventory. Do not introduce a second synchronized release list. Every record is retained. Released, Applied, and Uncertain are evidence of committed emission under the existing state machine; AwaitingApproval, Authorized, Denied, Expired, and Revoked are not. Audit every transition before relying on that invariant. No transition may erase an emitted state into an unemitted one.

The worker column requires a small migration because review JSON has no durable worker identity. Backfill from recorded task assignment without invoking live authority. A historical missing assignment or conflicting worker ownership fails the migration closed with a repair error; never guess ownership, silently drop history, or serve traffic with a partial backfill. This is necessary to preserve protection if historical task rows are later removed. Existing item epochs and issued-code ciphertext remain the payload source of truth.

Profile identity is globally bound to the first persisted review's worker. Before creating a review, reject any existing record for that profile owned by another worker. Before screening, inspect all recorded rows for the profile and reject inconsistent ownership. A profile with no manager records and an empty complete claim has an empty manager inventory and returns text unchanged. It grants no secret access and requires no unbounded registration table. Profile identifiers alone never grant access to another worker's stored inventory. This protects manager-released inventory, not a malicious worker that relabels copied browser files.

### Screening pseudocode

```text
authenticate current caller; enforce text/list bounds and Tracked complete=true
reject duplicate supplied IDs; interpret list order independently
load all records for claimed profile; require immutable worker == caller.worker
require each supplied ID names a recorded review in this profile and worker
require every manager-recorded emitted release is supplied
require every supplied release has committed emission evidence
  (conservative unreleased worker history therefore refuses, as today for TOTP)
for every recorded emitted release:
  password: decrypt its recorded epoch; select username and password only
  TOTP: require and decrypt its retained issued-code row and correct context
  reject missing/corrupt rows or unavailable decryption before inspecting text
apply existing reflects_secret representation checks to these selected values
return original text only if no selected value matches
```

The claimed set must equal the recorded emitted set. Unknown, omitted, duplicate, wrong-profile, other-worker, and unreleased IDs refuse independently of candidate text. A complete empty claim succeeds only with an empty recorded inventory. Prepared-but-never-released profiles can screen with an empty claim; including their possible unreleased release conservatively blocks capture. This avoids inventing a TOTP code or treating a seed as browser exposure. Retain past password epochs and every issued code indefinitely within the existing finite retention bounds. Never require historical `use_authority`, active items, live historical tasks, or valid historical devices. Current caller authentication remains mandatory. Leave `reject_known_secrets` and public-channel screening unchanged.

### Preparation pseudocode

```text
authenticate current caller; parse and validate exact destination and task plan
load all retained reviews for (task, execution), ignoring caller-chosen previous
if multiple incompatible initial roots or successors exist: refuse closed
if no root:
  require previous absent and current item revision == admitted revision
  perform existing item, rule, profile-owner, origin, and authority checks
  account global capacity; allocate and save the initial review
else:
  require destination == root.destination, and exact binding/fence/journal/purpose
  if remembered successor exists:
    accept previous absent or root.id, otherwise refuse
    reconcile and return existing successor; never reset its deadline or state
  if current item revision == root.item.revision:
    require previous absent
    reconcile and return root, including terminal states; allocate nothing
  else:
    require previous == root.id; root was Revoked by RememberPassword
    require root was never emitted; exact epoch/id/destination/parent equality
    require current revision == root.revision + 1 and matching allow rule cause
    require root.expires_at > now and current live authority and rule provenance
    account capacity; allocate exactly one successor with expiry <= root.expires_at
    persist successor and its remembered approval provenance atomically
```

Choose `previous absent` for ordinary retries; retain `previous=root.id` only for the existing remembered transition and retries of that transition. Different destinations, profile IDs, browser identities, frames, or element identities conflict after the first root. A new fence or journal does not create a new slot. It conflicts until another parent is admitted. This deliberately limits one parent to one exact fill, plus the one revision handoff, instead of an arbitrary quota that a worker can exhaust.

An expired, denied, revoked, released, applied, or uncertain root is returned as its existing reconciled state on exact retries. It never authorizes a new release. If a terminal root happens to satisfy a later allow rule, it still cannot reopen. The remembered transition is the only successor edge and requires the initiating root's original deadline. The successor never renews a thirty-second approval. TOTP never takes this edge and requires a new admitted parent and explicit approval for another fill.

Check duplicate lookup before global capacity. A retry at 4096 returns the original row. An initial creation or remembered successor at 4096 refuses without deleting history. That can leave a legitimate remembered decision without successor capacity; reserving capacity is unnecessary to fix task-local abuse and would add another accounting policy. The documented finite lifetime cap remains an owner-visible usage limit.

## Invariants and concurrency

1. One admitted parent has at most one root and one remembered-password successor. Exact retries never allocate IDs, approvals, events, or deadlines.
2. Every screening candidate is compared only with values committed for that authenticated worker's claimed profile.
3. Every recorded emitted value must be covered, independently of supplied history and current historical authority.
4. Password emission exposes username and password, never the TOTP seed. TOTP emission exposes only its retained issued code.
5. Release commits consumed state and issued-code retention before bytes leave the manager. A lost response remains protected and cannot be replayed.
6. No actual release history is reclaimed to solve preparation exhaustion.

The serialized State actor is the only writer, so check-and-create executes without actor interleaving. Transactions persist each new review and its authority together. A crash before commit leaves no new review; after commit an identical retry finds it. Existing release rollback behavior extends unchanged. Migration and profile ownership checks must occur before release bytes. No locks, worker caches, or dual-writer inventories are required.

## Synthesis decision

Choose retained review records with durable worker ownership and a parent-owned slot. This keeps the existing wire and one-shot state machine and derives inventory rather than synchronizing another ledger. It concentrates security policy in `uses.rs`, per boundary discipline and idempotent transitions. Its small interface hides completeness checks, selective decryption, ownership, retry admission, and the remembered handoff.

## Alternatives considered

An independent immutable emission ledger plus a preparation-slot table is a complete alternative. Release atomically writes a receipt containing worker, profile, item epoch, purpose, and issued-code reference; Screen reads receipts, while Prepare owns a unique parent slot and successor pointer. It makes emission proof and lookup explicit and tolerates future mutable review states. It loses here because existing retained review states already encode emission and slots can be derived within 4096 rows. Two new tables would duplicate lifecycle facts and require migration consistency checks across three representations. Its external interface is equally deep, but internal ownership is less concentrated.

A manager-issued profile capability and preparation request token is another complete shape. Register a profile, return an authenticated capability bound to worker, and carry it in Prepare, Release, and Screen; store exposure receipts and token outcomes. It supports explicit pristine-profile registration and profile transfer policy. It loses because callers coordinate new lifecycle state, transport schemas change, and token selection still needs parent-scoped admission to stop destination spam. It buys no necessary protection for current UUID profile identities and manager-recorded worker ownership.

## Tradeoffs accepted

- We accept a small worker-ownership migration in exchange for retained protection independent of historical task availability.
- We accept bounded scans of at most 4096 records in exchange for avoiding synchronized indexes and slot state.
- We accept conservative refusal for possible but unemitted worker history in exchange for missing-issuance safety without synthetic codes.
- We accept a new admitted parent for another exact fill in exchange for a strict per-parent durable-record bound.
- We accept the existing finite global retention limit in exchange for preserving all historical screening coverage.

## Verification cases

Add focused manager fixture cases that fail current code:

- An authenticated worker supplies complete empty history and guesses unrelated provider, connector, login, seed, and issued-code values. Results equal an equally valid harmless string. After its own password release, its username/password refuse while unrelated values succeed.
- A profile has two actual releases. Omitting either refuses even for harmless text. Duplicate, unknown, cross-profile, and other-worker IDs refuse. Another worker claiming the profile with an empty list refuses.
- Screen old password epochs and issued TOTP codes from a new live caller task after original task removal, item rotation/removal, device revocation, and restart. Safe text succeeds; exposed values refuse. Seeds and unissued codes are not added to the screen set.
- Corrupt epoch ciphertext, missing epoch, missing code, and corrupt issued-code ciphertext refuse harmless text before matching.
- Repeat Prepare thousands of times. IDs, row count, original expiry, and decision stay fixed. Change each exact destination identity and previous ID in turn; every altered request conflicts without new rows.
- Retry at full capacity and after denial, expiry, release, settlement, and restart. No new identifiers or renewed authorization appear. New admitted parents still require fresh TOTP consent.
- RememberPassword produces one exact successor, retaining the original deadline. Repeat with and without previous after successor creation; return the successor. Wrong previous, changed destination, expired root, second revision advance, deny cause, and revoked grant cannot create it. Capacity failure retains the root and all history.
- Preserve release transaction rollback and consume-before-bytes tests. Inject failure during successor authority save to prove retry creates only one complete successor.

Update the existing same-parent second-TOTP-Prepare assertion to require the consumed review; test fresh consent on a newly admitted parent. Browser runtime and retained-capture scenarios remain necessary integration checks after implementation, not part of this design run.

## Implementation reconciliation

No implementation occurred. The emitted-state invariant and worker-column migration must be reconciled with every schema and state writer before coding. If any state transition can erase emission evidence, choose the independent immutable ledger alternative rather than add a second inference rule.

## Open questions and risks

Can every existing emitted review be backfilled to one immutable worker assignment? If not, migration must refuse with explicit repair instructions rather than weaken protection. Can deployment tolerate refusing legacy parents already containing multiple roots? The safe default is refusal and a newly admitted task; no history deletion.

Manager profile ownership cannot authenticate browser filesystem provenance against a compromised worker that invents a fresh profile identity. Honest-worker paired-marker checks still protect missing or corrupt local history. Full guest privilege and arbitrary transformations remain outside the documented guarantee. Existing `reflects_secret` representation coverage is preserved, not expanded.

## Next implementation step

Implement private durable worker ownership and the focused cross-worker/empty-history screening tests, then add parent-slot admission before changing any release behavior.
