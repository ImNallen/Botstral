## Findings

### 1. [warning] A committed mutation can return a plain storage or protocol error

**Location:** `manager/src/owner_client.rs:517` (`OwnerSession::call`); response staging at lines 546–591.

**Trigger:** A mutation reaches the Manager and commits, but the client cannot create/write/sync its response staging file, or fails to validate/decode the response. Only network errors and the outer timeout become `UnknownOutcome`; `Storage` and `Protocol` pass through unchanged.

**Impact:** The caller receives an ordinary client failure without the required indication that the mutation may already have taken effect. This violates the accepted unknown-mutation-outcome contract and can produce incorrect cancellation/approval feedback or unsafe retry decisions. The current probe stops rather than automatically retrying, so I classify this as a warning rather than claiming an observed duplicate effect.

**Evidence:** I built this checkout with `cargo build -p botstral-manager --locked --bin botstral-manager --bin owner-probe`. `/tmp/m05_sol_mutation_repro.py` uses the actual Manager and native probe plus a pinned TLS response fixture. The fixture receives one append-message call, commits it through the actual Manager, and injects a response-stage creation failure before sending its transfer start. Output: `{"committed_message":"3a597755-5eb4-499a-8318-8cb230422b06","native_error":"Error: Storage","message_in_actual_manager_state":true}`. An out-of-space write or failed sync follows the same error path without the fixture's directory injection. The existing acceptance only exercises connection loss after dispatch.

**Fix:** Once a mutation send succeeds, treat every failure that prevents establishing its domain result as an unknown outcome, while retaining the underlying cause if useful. Keep a fully received, correlated domain refusal distinct. Add focused coverage for post-dispatch storage and transfer-validation failures.

### 2. [warning] A transient TLS alert permanently stops reconnect

**Location:** `manager/src/owner_client.rs:380–385` (`OwnerSession::authenticate`); terminal handling at lines 322–331.

**Trigger:** A peer returns a TLS alert such as `internal_error` during handshake. The code classifies every wrapped `rustls::Error` as `Trust`, including `AlertReceived(InternalError)`, even before the peer supplies a certificate. `watch` returns this error immediately.

**Impact:** A temporary peer failure or unauthenticated early-handshake alert stops the native reconnect driver and requires intervention. This is not evidence of a pin, chain, name, or signature failure. An opaque relay or network intermediary can cause the terminal state without knowing the Manager key. The pin remains enforced, but the accepted transient-failure reconnect behavior is broken.

**Evidence:** `/tmp/m05_sol_tls_repro.py` creates a valid protected client trust profile, accepts its ClientHello, and returns the raw fatal TLS `internal_error` alert `15 03 03 00 02 02 50`. The freshly built native `watch ... 3` exits immediately with `{"alert":"fatal internal_error","exit":1,"native_error":"Error: Trust"}`. No certificate or application credential exchange occurs.

**Fix:** Downcast and inspect the Rustls error. Reserve terminal trust failures for certificate/pin/name/handshake-signature or permanent trust-configuration failures. Route transient peer/internal alerts through bounded reconnect, preserving the configured pin on every attempt. Verify a single internal-error alert followed by a healthy pinned Manager recovers, while wrong-leaf acceptance still fails before application bytes.

## Review scope

Reviewed `b72bd5d..3de495e`, accepted architecture and design verdict, snapshot calculation, enrollment/revocation State implementation, shared authority changes and callees, TLS/public/worker routing, owner transfer and client lifecycle, and acceptance/bounds tests. No additional concrete security, resource-bound, compatibility, or structural blockers found. No repository edits, child agents, issues, or PRs were created. Repro scripts and this report are outside the repository; fixture processes and private roots were removed.
