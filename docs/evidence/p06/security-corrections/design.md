# Security correction contract

The independent manager lane produced four failures at 3527f3c. Root reproduced all four at b95fb2c before changing the implementation. Device revocation retained release authority, ordinary URL encodings became public metadata, numeric private values survived run settlement, and a replacement manager falsely closed an original manager receipt. Issues 50, 51 and 52 track these gaps.

## Approval provenance

Persist a private `FillAuthority` per credential use. Its variants are authenticated local owner and paired device with its grant epoch. A fresh decision stores that authority in the same transaction as its use. Release and reconciliation require that authority to remain valid. Device grants are checked against the current durable device epoch and revocation flag, not a live connection, so ordinary disconnection does not revoke approved work.

Remembered password use inherits the originating rule decision's authority. A revoked or missing source cannot create a fresh automatic use. The owner can make a fresh decision from a current device. Denial remains conservative. Device revocation atomically invalidates affected active uses and durably cancels affected released work. Unrelated devices' grants remain intact.

The alternative is global invalidation and cancellation on any device revocation. Reject it because it loses approval provenance, disrupts unrelated authority and can still allow automatic re-preparation through a remembered rule. A private authority table deepens the existing manager-owned state model without changing public review schemas.

Migration 15 adds the private use-authority table. Old remembered allows without provenance require fresh approval. Startup already invalidates unfinished live uses. Backups retain the private authority and enforce the current schema bound.

## Private value screening

Keep one shared scanner. Normalize ordinary percent escapes and form plus encoding before matching, preserving case sensitivity of actual private values. Inspect JSON numbers, booleans and null in their public serialized representation. Bound decode layers and fail closed when encoded content cannot be safely scanned. Preserve direct, JSON, standard and URL-safe Base64 checks. Production inputs, metadata, settlement and public reads continue to share this boundary.

## Receipt provenance

Persist `ProtectedWriteRecord { manager, status }` after authenticated recovery and before private send. Read status only under that original manager identity. Another manager cannot close, overwrite or report reconciliation of an unresolved request. A resolved foreign receipt is absent from the current manager's status view and may be replaced by a later current-manager write. Unbound legacy records refuse because their origin cannot be established. Unlock remains an independent protected operation and does not resolve item-write receipts.

Use the actual stored record when injecting a lost response in tests, so fault injection preserves production-written manager identity. A two-manager test must preserve the original record, refuse false closure and resolve the original commit after reconnecting to its manager.

These are corrections within the selected manager authority, private transport and publication ownership. They do not add a second execution route or relax secret exclusion. Root will rerun the original probes, promote the failing cases into permanent tests, rerun Linux execution at the corrected source, and obtain fresh independent review.
