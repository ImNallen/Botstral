## Findings

### 1. [critical] Owner sessions can corrupt each other's transfer staging
**Location:** `manager/src/owner_client.rs:546–549`; session construction at `:206–240`, storage ownership at `:70–74` and `:345–359`.

**Finding:** The public API permits multiple live `OwnerSession`s from one `OwnerClient`, but every session truncates, writes, and removes the same `.transfer.part` file. The exclusive filesystem lock belongs only to `OwnerClient`; a returned session neither borrows that client nor retains its lock. Dropping the client also permits another client/process to open the directory and remove a live session's staging file.

**Trigger and impact:** Keep a watching/recovering connection while issuing a read on another connection for the same native client. The second reply truncates the first transfer's file. Its cleanup can unlink that file while the first transfer still holds it open. Valid authenticated transfers then fail parsing, and the advertised single-staging-file ownership and verified-transfer contract no longer hold. The digest is calculated from incoming chunks, whereas deserialization reads the shared, concurrently modified file.

**Evidence:** An isolated Rust harness used two sessions returned by one client against the actual Manager. The first requested a snapshot through a throttled opaque proxy; the second requested a small document search directly. Observed twice: `fast=Ok(())`, `slow=Err(Protocol)`. Both requests succeeded independently. No repository files were changed. The harness and fixture are in `/tmp/botstral-review-astra/concurrent.rs` and `/tmp/botstral-review-astra/repro.py`.

**Suggestion:** Make storage ownership structural. Retain the directory lock for every session's lifetime and serialize use of the single transfer staging slot across all sessions, or enforce a single live session in the client API. Unique staging names alone need a revised aggregate storage bound and do not fix premature lock release. Add a real two-session overlap check and a session-outlives-client lock check.

### 2. [warning] Mutation outcome classification misses storage and protocol failures
**Location:** `manager/src/owner_client.rs:506–518`; post-response staging failures at `:546–548`, `:570–573`, and `:586–591`.

**Finding:** After transmitting a mutation, `call` maps only timeout/network failures to `UnknownOutcome`. A staging write/open/sync failure or unusable reply instead escapes as `Storage` or `Protocol`, despite the mutation potentially having committed. These errors do not establish a refusal, and the documented unknown-outcome contract must not depend on where receipt of the response failed.

**Evidence:** A second isolated harness injected failure to create the staging file after authentication, then called `AppendMessage` on the actual Manager. It observed `mutation_result=Err(Storage)` and, through a fresh authoritative snapshot, `mutation_committed=true`. The harness is `/tmp/botstral-review-astra/storage.rs`, run by the same fixture above. The current probe does not automatically retry mutations, so this is an incomplete reusable-client error contract rather than demonstrated duplicate execution.

**Suggestion:** Once transmission may have occurred, classify every failure before a fully validated reply or explicit correlated refusal as an unknown mutation outcome. Preserve the underlying cause as attached diagnostic information if needed. Cover storage failure and malformed/incomplete reply cases as well as network loss.

## Review scope

Reviewed `b72bd5d..3de495e`, the accepted architecture and design verdict, transfer/resource claims, owner authority and local-authority callees, shared public TLS admission, native client lifecycle, retained worker paths, migrations, snapshot cardinality/size admission, and acceptance code. No additional concrete security bypass or server recovery-bound defect was found. This report does not claim to have rerun the full acceptance suite. Synthetic local fixture processes and credentials from the isolated repros were removed.
