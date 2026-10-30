Score each criterion1-5 with concrete source-backed rationale.
1. Prevent a compromised authenticated worker from using Screen as a global unrelated-secret oracle.
2. Validate profile ownership and complete manager-recorded emitted history while retaining old password/code screening across expiry, removal and recovery.
3. Make Prepare idempotent and capacity-bounded per admitted parent, including altered destinations, expiry, consumption and the exact remembered-password handoff.
4. Preserve existing public/wire behavior where legitimate, fail closed on unknown/missing/corrupt inventory, and avoid dropping actual history or authority.
5. Prefer a deep small manager boundary and the smallest verifiable change; justify any migration, new mutable ledger or caller coordination.
