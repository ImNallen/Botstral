# M01 manager architecture

M01 gives a local client one owner, durable bots, revocable sessions, authenticated connection metadata, and write-only provider entry. This record reconciles the finalized M01 design with the implementation based on source commit `cd9c353`. The [local API](local-api.md) and [security reference](security.md) document the actual contracts.

## One owner for ordered state

One `Manager` handle submits complete operations through a bounded Tokio channel. One dedicated operating-system thread owns rusqlite and the vault state. SQLite, Argon2id, and age run on that thread. They never run on Tokio reactor threads. Handlers parse one request, call one handle method, and encode its result.

The named shape is `Installation::Unconfigured { bootstrap_hash }` or `Installation::Configured { vault }`. The vault is `VaultState::Locked` or `VaultState::Unlocked { identity }`. Only the age X25519 identity remains unlocked in memory. Each protected operation checks its session on the state thread immediately before reading or mutating data. No handler receives a connection, decrypted identity, transaction, or cached authorization.

The queue holds at most 32 pending operations. A shared semaphore admits one setup, sign-in, or unlock operation, including time queued. Its permit belongs to the queued operation, so a disconnected caller cannot release it while work continues. Admission fails immediately at either bound. The private envelopes are boxed complete operations with typed one-shot replies. There is no public command framework or repository trait.

Serial credential latency keeps revocation and vault locking in the same order as protected operations. Parallel crypto would require a completion protocol to reject stale unlock results. M01 has no measured demand that earns that state machine.

## Module ownership

The workspace contains one package and one executable. These paths are relative to `crates/botstral-manager`.

| File | Responsibility |
| --- | --- |
| `src/main.rs` | CLI validation, process protection before threads, startup, and fixed diagnostics. |
| `src/local_state.rs` | Private filesystem checks, state files, and the process-lifetime lock. |
| `src/http.rs` | Separate routers, private wire types, bounded TCP serving, browser checks, and error mapping. |
| `src/manager/mod.rs` | Domain inputs, typed identifiers, async handles, bounded admission, and replies. |
| `src/manager/state.rs` | Complete operations, operation-time authorization, migrations, transactions, and vault state. |
| `src/manager/crypto.rs` | Password hashing, token generation and hashing, and established age encryption. |
| `migrations/0001.sql` | Singleton owner and vault, hashed sessions, bots, and encrypted provider values. |
| `tests/m01_local.rs` | One real-binary TCP acceptance test and its private process and socket helpers. |

Names contain validated strings. `BotId` and `SessionId` are distinct UUID wrappers. Login passwords, vault passphrases, provider values, session tokens, and bootstrap proofs are distinct secret types. Wire and SQL representations stay private.

## Actual interfaces

These are the implemented handle signatures. Every operation returns `Result<_, OperationError>`.

```rust
impl LocalSetup {
	pub async fn create_owner(&self, proof: BootstrapToken, input: OwnerSetup)
		-> Result<OwnerProfile, OperationError>;
}
impl Manager {
	pub async fn sign_in(&self, password: LoginPassword) -> Result<SessionGrant, OperationError>;
	pub async fn owner(&self, token: SessionToken) -> Result<OwnerProfile, OperationError>;
	pub async fn sessions(&self, token: SessionToken) -> Result<Vec<SessionMetadata>, OperationError>;
	pub async fn sign_out(&self, token: SessionToken) -> Result<(), OperationError>;
	pub async fn revoke_session(&self, token: SessionToken, target: SessionId) -> Result<(), OperationError>;
	pub async fn create_bot(&self, token: SessionToken, input: NewBot) -> Result<Bot, OperationError>;
	pub async fn get_bot(&self, token: SessionToken, id: BotId) -> Result<Bot, OperationError>;
	pub async fn list_bots(&self, token: SessionToken) -> Result<Vec<Bot>, OperationError>;
	pub async fn connection_status(&self, token: SessionToken) -> Result<ConnectionStatus, OperationError>;
	pub async fn unlock_vault(&self, token: SessionToken, passphrase: VaultPassphrase) -> Result<(), OperationError>;
	pub async fn lock_vault(&self, token: SessionToken) -> Result<(), OperationError>;
	pub async fn replace_provider_key(&self, token: SessionToken, secret: ProviderSecret) -> Result<ProviderMetadata, OperationError>;
}
```

For example, the adapter constructs `NewBot` from the caller UUID and JSON name, then awaits `manager.create_bot(token, input)`. The state thread checks the token, returns the identical record for an identical retry, rejects conflicting fields, or inserts a new record. The caller never coordinates separate authorization and write stages.

Crate-private `Manager::start(directory)` starts the state thread and waits for migrations and initialization. It returns the manager, optional restricted `LocalSetup`, and thread handle. Only the setup router receives `LocalSetup`. Startup removes sessions and begins locked. A watch notification closes setup after owner and encrypted identity commit together. Already accepted requests still face the configured-state check.

`serve(local, listen, setup_listen)` owns listeners and runtime. Graceful shutdown stops admission, drains connections and operations, joins the state thread, removes the address file, and releases the installation lock. The binary protects the process before creating runtime threads or state. Setup is never merged into the regular router.

## Encryption and durability

age 0.12.1 supports Rust 1.93.0 and is pinned in Cargo.lock. Its documented [`Encryptor`](https://docs.rs/age/0.12.1/age/struct.Encryptor.html) distinguishes human passphrases from programmatic recipients. Setup uses `with_user_passphrase` for the generated identity. Provider entry uses `with_recipients`. Every writer calls `finish()` before ciphertext enters a transaction. Unlock uses the documented [passphrase decryption API](https://docs.rs/age/0.12.1/age/index.html#passphrase-based-encryption).

age labels this pre-1.0 release beta. M01 accepts that established dependency and adds no custom cryptographic construction. The encrypted provider payload binds its format and provider. Internal decryption tests round-trip, replacement, and authentication failure. It is compiled out of production.

Setup inserts owner and encrypted identity in one immediate transaction. Key replacement commits ciphertext and metadata together. Bot identifiers make retries safe. Migrations are transactional and refuse future versions. SQLite WAL with `synchronous=FULL` commits results before responding. TCP acceptance force-kills the binary after successful writes, then compares the entire original bot after restart and new sign-in.

## Verification and reconciliation

`scripts/verify.sh` is the single rerunnable check exposed by `make verify` and Linux CI. It runs formatting, clippy with warnings denied, workspace tests including actual binary TCP acceptance, and a release build. Focused tests cover expiry, transaction rollback, authenticated decryption, and deterministic admission. TCP acceptance scans every generated file and captured output for synthetic secrets and issued session tokens.

The implementation follows the finalized Astra base with its mandated reductions and Sol grafts. These implementation details reconcile the proposal with the code:

- Bot lists return every record. There is no pagination or separate retry identifier.
- HTTP lifecycle helpers remain in `http.rs`. Filesystem protection has a small module that owns the installation lock.
- Handles consume a `SessionToken` per operation. The adapter constructs it per request and avoids token cloning. Provider entry takes `ProviderSecret` directly because M01 has only Mistral.
- The domain status contains `VaultStatus` and optional provider metadata. The adapter emits truthful fixed worker, inference, and remote strings. There are no one-variant capability enums.
- The unlocked vault retains an identity. There is no whole-vault plaintext bundle or retained passphrase.
- Exact expiry checks use a private test-only clock. Production has no clock override. Queue bounds use handle behavior with a deliberately blocked owner in a focused test. Credential budgets and socket deadlines run over TCP.
- Application errors are fixed JSON codes. Malformed traffic rejected by the HTTP parser can close without an application response. JSON content type is deliberately exact.
- Core dumps are disabled. Deployment swap configuration is a documented limit, not an M01 gate.

Independent review and remote Linux CI execution belong to parent integration. Worker execution, inference, WSS, pairing, native clients, backup automation, secret export, destructive reset, and passphrase rotation remain outside M01.
