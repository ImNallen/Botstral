# Security and recovery reference

M01 serves one local owner on literal loopback addresses. Setup has its own listener and a random bootstrap capability in a private file. The ordinary router cannot create the owner. Host and browser checks apply before routing. M02 adds a separate mutual TLS worker listener. Owner-device pairing, remote access, model calls, and native apps remain unimplemented.

## Authentication

The only owner row has SQL primary key `1` and a constraint rejecting other identifiers. Setup commits that row and the vault row in one immediate transaction. The owner password uses Argon2id version 19 with a random salt, 65,536 KiB of memory, three passes, one lane, and a 32-byte output.

Session tokens contain 256 random bits and are returned once. SQLite stores SHA-256 token hashes, public session identifiers, and absolute expiry times. Sessions expire after 24 hours. Startup transactionally removes all old sessions. Every protected operation authorizes when it executes on the state thread. Revocation, sign-out, locking, and writes execute in that same order. Other valid sessions retain their authority after a session is revoked or signed out.

## Vault format and unlock

The credential store uses age 0.12.1. Setup generates an age X25519 identity and encrypts its textual representation with the owner-provisioned vault passphrase. Each provider value is encrypted independently to the identity's public recipient. The encrypted provider payload binds format version 1 and provider `mistral` to the value. SQLite stores only finished ciphertext and nonsecret metadata.

Setup, lock, and every restart leave the vault locked. Unlock decrypts and validates the identity before changing state. Wrong passphrases and invalid identity ciphertext leave it locked. Unlocking an already unlocked vault returns conflict. The unlocked vault retains only the identity. It discards the human passphrase and keeps provider values encrypted while idle. Lock discards the identity after preceding operations complete.

The sign-in password cannot derive or replace the vault passphrase. Sign-in never unlocks the vault. Protected provider entry requires a valid session and an unlocked vault. Replacement commits ciphertext and metadata together. A failed encryption or database transaction preserves the preceding stored value. No client route reads or exports provider values.

Identity ciphertext tampering fails unlock. Unlock does not decrypt every provider item. Provider ciphertext tampering fails authenticated decryption when that item is exercised internally. Focused tests verify this without exposing a production retrieval method.

## Recovery policy

Setup requires acknowledgment that there is no escrow. The owner keeps an offline copy of the separate vault passphrase. The database and passphrase are both required to recover encrypted provider credentials. The login password cannot recover those values.

M01 provides no automatic unlock, escrow, destructive reset, passphrase rotation, or backup service. Losing the passphrase loses access to Botstral's stored provider values. Recovery then depends on recovering or rotating credentials at the provider and configuring another installation. There is no endpoint that bypasses encryption.

Copying a live SQLite database alone is not a complete backup. A stopped manager's state directory includes its database and any SQLite sidecars left by an unclean shutdown. Later backup and restore features remain open in PLAN.md.

## Local state and process boundary

The manager sets umask `077` and disables core dumps before starting runtime threads. The state directory must be owned by the service account and have mode `0700`. Existing state files must be regular, singly linked, owned by that account, and have mode `0600`. Symlinks, special files, unsafe permissions, and a symlinked state-directory entry are rejected. The canonical directory is checked again before use. Parent directories and processes running as the same service account remain trusted.

The database, SQLite WAL and shared-memory files, lock, bootstrap capability, and listener-address file use private permissions. A process-lifetime file lock excludes a second manager. SQLite uses foreign keys, WAL, `synchronous=FULL`, and a two-second busy timeout. Schema changes are transactional. An unsupported future schema version refuses startup before listeners open.

The manager has no request logging. It prints a fixed readiness message and fixed startup or shutdown failure text. It never logs request bodies, headers, query strings, raw errors, passwords, passphrases, keys, or session tokens. Application errors contain fixed secret-free codes.

Secret input types cannot be cloned, debug-printed, or serialized. Their owned buffers clear on drop. Parsed secret fields also clear when requests fail validation. Temporary plaintext provider payloads and identity buffers clear on drop. Network, JSON, allocator, and operating-system buffers can retain copies. These measures do not protect against a compromised manager, another process with the same account's inspection rights, a privileged host, swap inspection, or the receiving provider. Deployment memory and swap policy is outside M01.

## Verification scope

`make verify` runs formatting, clippy with warnings denied, all workspace tests, and a release build. `m01_local` launches the actual executable on private ephemeral loopback ports. It uses synthetic credentials and scans every generated state file and captured output for passwords, passphrases, provider values, and every issued session token, including JSON-escaped and base64 encodings.

The TCP test covers capability rotation, concurrent setup, setup isolation, authentication, idempotent bot creation, forced-restart persistence, session revocation and sign-out, locked startup, incorrect and correct unlock, key replacement, explicit lock, read refusal, browser boundaries, JSON and size limits, deadlines, credential budgets, private permissions, second-process exclusion, migration reuse, future migration refusal, unsafe state, and identity tampering. It reports actual process exits and kills and reaps child processes on failure.

Focused tests cover the exact absolute expiry boundary without waiting 24 hours, bounded queue and credential admission, atomic migration and setup rollback, failed key replacement preserving the previous decryptable value, successful replacement, vault round-trip, and provider ciphertext tampering. Production has no test clock option or secret export function.

## Worker authority

M02 binds a worker to SHA-256 of its verified client certificate DER. Enrollment requires an expiring one-use invitation for that fingerprint. The manager stores the invitation hash and consumes it atomically with enrollment. Worker connections have no owner authority or bearer secret. Certificate renewal requires revocation and explicit enrollment of the new certificate.

The dedicated worker listener exposes enrollment and WSS only. Worker schemas cannot grant approvals, enroll owner devices, retrieve credentials, or change security policy. The daemon validates the manager CA and hostname with rustls. No insecure verifier or trust bypass exists.

Connection generations and per-session ownership revisions fence obsolete messages. Takeover remains pending until the worker confirms the new revision after stopping its old input producer. Disconnection does not prove that a remote effect stopped. Production refuses execution in M02. The separate test worker exists only in a debug feature build. Release compilation rejects that feature.

See [deployment and certificate recovery](worker-deployment.md), [protocol limits](worker-protocol.md), and [the verification and acceptance gaps](m02-architecture.md).
