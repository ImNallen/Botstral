# Protected reusable logins

A conversation hands reusable-login entry to the paired desktop. The handoff contains the bot identifier and no private values. The owner chooses public metadata and an exact HTTPS origin. macOS collects the username, password and optional TOTP seed in `NSSecureTextField` controls. Other platforms refuse protected entry.

The native service sends a bounded binary frame outside `OwnerOperation` and `Actor::mutate`. Its durable record contains a request identifier and a value-free status. It never replays private input. Reconcile an unresolved receipt before entering values again. The manager serializes closure with writes, so a late write cannot commit after a closed-without-commit receipt.

The manager encrypts each revision to the unlocked age identity. The authenticated payload binds the installation, item, encrypted epoch and login kind. Metadata changes advance the item revision. Rotation advances both revision and encrypted epoch. Removal leaves a tombstone and retains encrypted historical epochs for disclosure screening. There is no stored-login-value read operation.

Backups contain the encrypted age identity, encrypted login epochs, metadata and receipts. Startup and restore remain locked. Unlock requires the owner-provisioned passphrase. An unavailable passphrase cannot be recovered from the backup. Restoring an older backup also restores its older policy and revocation state. The owner must reconcile later changes.

Removing or rotating a login does not log out browser sessions or revoke website cookies, copied credentials or remote grants. Historical encrypted values remain in the manager and backups. This does not establish secrecy against arbitrary code with full shared guest privileges. Rust-owned private buffers are cleared when dropped. Browser, JavaScript, transport-library and operating-system memory cannot be guaranteed to zeroize.

The supported TOTP seed encoding is uppercase unpadded Base32. The intended fill parameters are HMAC-SHA-1, six digits and a 30-second period. The algorithm check uses all SHA-1 vectors in [RFC 6238 Appendix B](https://www.rfc-editor.org/rfc/rfc6238#appendix-B). Seed entry and the vector test do not establish browser filling.

Run `python3 scripts/verify-p06.py` for manager storage and actual paired native-service checks. Each invocation writes exact command results under `docs/evidence/p06/`. The baseline screenshot and video replay the real handoff component. The paired native-service test exercises authenticated transport and disk persistence. Neither is proof of packaged native interaction or Linux browser execution.

The live-fill unit and final independent review remain pending. No P06 completion checkbox is advanced by this storage unit.
