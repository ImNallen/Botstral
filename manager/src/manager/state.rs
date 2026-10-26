mod workers;

use super::{crypto, *};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use std::{
    fs,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use subtle::ConstantTimeEq;

enum Installation {
    Unconfigured { bootstrap_hash: [u8; 32] },
    Configured { vault: VaultState },
}
enum VaultState {
    Locked,
    Unlocked { identity: age::x25519::Identity },
}

struct Attempts {
    start: Instant,
    count: u8,
}
impl Attempts {
    fn new() -> Self {
        Self {
            start: Instant::now(),
            count: 0,
        }
    }
    fn admit(&mut self) -> Result<(), OperationError> {
        if self.start.elapsed() >= Duration::from_secs(60) {
            *self = Self::new();
        }
        if self.count >= 5 {
            return Err(OperationError::RateLimited);
        }
        self.count += 1;
        Ok(())
    }
}

pub(super) struct State {
    connection: Connection,
    installation: Installation,
    directory: PathBuf,
    configured: watch::Sender<bool>,
    sign_in_attempts: Attempts,
    unlock_attempts: Attempts,
    setup_attempts: Attempts,
    worker_connections:
        std::collections::HashMap<botstral_link::WorkerId, workers::WorkerConnection>,
    #[cfg(test)]
    test_time: Option<i64>,
}

impl From<rusqlite::Error> for OperationError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Internal
    }
}

fn migrate(connection: &mut Connection) -> Result<(), OperationError> {
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > 2 {
        return Err(OperationError::Internal);
    }
    if version == 0 {
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(include_str!("../../migrations/0001.sql"))?;
        tx.commit()?;
    }
    if version < 2 {
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(include_str!("../../migrations/0002.sql"))?;
        tx.commit()?;
    }
    Ok(())
}

impl State {
    fn from_connection(
        mut connection: Connection,
        directory: PathBuf,
        configured: watch::Sender<bool>,
    ) -> Result<Self, OperationError> {
        connection.busy_timeout(Duration::from_secs(2))?;
        connection.pragma_update(None, "foreign_keys", true)?;
        migrate(&mut connection)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        let owner_exists: bool =
            connection.query_row("SELECT EXISTS(SELECT 1 FROM owner)", [], |row| row.get(0))?;
        let vault_exists: bool =
            connection.query_row("SELECT EXISTS(SELECT 1 FROM vault)", [], |row| row.get(0))?;
        if owner_exists != vault_exists {
            return Err(OperationError::Internal);
        }
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM sessions", [])?;
        tx.commit()?;
        let installation = if owner_exists {
            remove_bootstrap(&directory)?;
            configured.send_replace(true);
            Installation::Configured {
                vault: VaultState::Locked,
            }
        } else {
            let capability = age::secrecy::SecretString::from(crypto::random_token());
            use std::{io::Write, os::unix::fs::OpenOptionsExt};
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(directory.join("bootstrap.token"))
                .map_err(|_| OperationError::Internal)?;
            file.write_all(capability.expose_secret().as_bytes())
                .map_err(|_| OperationError::Internal)?;
            file.sync_all().map_err(|_| OperationError::Internal)?;
            Installation::Unconfigured {
                bootstrap_hash: crypto::token_hash(capability.expose_secret()),
            }
        };
        Ok(Self {
            connection,
            directory,
            installation,
            configured,
            sign_in_attempts: Attempts::new(),
            unlock_attempts: Attempts::new(),
            setup_attempts: Attempts::new(),
            worker_connections: Default::default(),
            #[cfg(test)]
            test_time: None,
        })
    }

    pub(super) fn open(
        directory: PathBuf,
        configured: watch::Sender<bool>,
    ) -> Result<Self, OperationError> {
        let connection = Connection::open_with_flags(
            directory.join("manager.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        Self::from_connection(connection, directory, configured)
    }

    fn now(&self) -> i64 {
        #[cfg(test)]
        if let Some(time) = self.test_time {
            return time;
        }
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_secs() as i64
    }

    pub(super) fn is_configured(&self) -> bool {
        matches!(self.installation, Installation::Configured { .. })
    }

    pub(super) fn create_owner(
        &mut self,
        proof: BootstrapToken,
        input: OwnerSetup,
    ) -> Result<OwnerProfile, OperationError> {
        let Installation::Unconfigured { bootstrap_hash } = &self.installation else {
            return Err(OperationError::Conflict);
        };
        self.setup_attempts.admit()?;
        if !bool::from(bootstrap_hash.ct_eq(&crypto::token_hash(proof.0.expose_secret()))) {
            return Err(OperationError::Unauthorized);
        }
        let password_hash = crypto::hash_password(&input.login_password)?;
        let identity = crypto::new_identity(input.vault_passphrase)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO owner(id, name, password_hash) VALUES(1, ?1, ?2)",
            params![input.name.as_str(), password_hash],
        )?;
        tx.execute(
            "INSERT INTO vault(id, format_version, identity_ciphertext) VALUES(1, 1, ?1)",
            [identity],
        )?;
        tx.commit()?;
        self.installation = Installation::Configured {
            vault: VaultState::Locked,
        };
        self.configured.send_replace(true);
        remove_bootstrap(&self.directory)?;
        Ok(OwnerProfile { name: input.name })
    }

    pub(super) fn sign_in(
        &mut self,
        password: LoginPassword,
    ) -> Result<SessionGrant, OperationError> {
        self.sign_in_attempts.admit()?;
        let hash: Option<String> = self
            .connection
            .query_row("SELECT password_hash FROM owner WHERE id = 1", [], |row| {
                row.get(0)
            })
            .optional()?;
        if !hash.is_some_and(|hash| crypto::verify_password(&password, &hash)) {
            return Err(OperationError::Unauthorized);
        }
        let token = SessionToken(age::secrecy::SecretString::from(crypto::random_token()));
        let now = self.now();
        let session = SessionMetadata {
            id: SessionId(Uuid::new_v4()),
            created_at: now,
            expires_at: now + 24 * 60 * 60,
        };
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM sessions WHERE expires_at <= ?1", [now])?;
        tx.execute("INSERT INTO sessions(id, token_hash, owner_id, created_at, expires_at) VALUES(?1, ?2, 1, ?3, ?4)",
            params![session.id.0.to_string(), crypto::token_hash(token.0.expose_secret()).as_slice(), now, session.expires_at])?;
        tx.commit()?;
        Ok(SessionGrant { token, session })
    }

    fn authorize(&self, token: &SessionToken) -> Result<SessionId, OperationError> {
        let id: Option<String> = self
            .connection
            .query_row(
                "SELECT id FROM sessions WHERE token_hash = ?1 AND expires_at > ?2",
                params![
                    crypto::token_hash(token.0.expose_secret()).as_slice(),
                    self.now()
                ],
                |row| row.get(0),
            )
            .optional()?;
        id.map(|id| {
            id.parse()
                .map(SessionId)
                .map_err(|_| OperationError::Internal)
        })
        .transpose()?
        .ok_or(OperationError::Unauthorized)
    }

    pub(super) fn owner(&mut self, token: SessionToken) -> Result<OwnerProfile, OperationError> {
        self.authorize(&token)?;
        let name: String =
            self.connection
                .query_row("SELECT name FROM owner WHERE id = 1", [], |row| row.get(0))?;
        Ok(OwnerProfile {
            name: OwnerName(name),
        })
    }

    pub(super) fn sessions(
        &mut self,
        token: SessionToken,
    ) -> Result<Vec<SessionMetadata>, OperationError> {
        self.authorize(&token)?;
        let mut statement = self.connection.prepare("SELECT id, created_at, expires_at FROM sessions WHERE expires_at > ?1 ORDER BY created_at, id")?;
        let rows = statement.query_map([self.now()], |row| {
            Ok((row.get::<_, String>(0)?, row.get(1)?, row.get(2)?))
        })?;
        rows.map(|row| {
            let (id, created_at, expires_at) = row?;
            Ok(SessionMetadata {
                id: SessionId(id.parse().map_err(|_| OperationError::Internal)?),
                created_at,
                expires_at,
            })
        })
        .collect()
    }

    pub(super) fn sign_out(&mut self, token: SessionToken) -> Result<(), OperationError> {
        let id = self.authorize(&token)?;
        self.connection
            .execute("DELETE FROM sessions WHERE id = ?1", [id.0.to_string()])?;
        Ok(())
    }
    pub(super) fn revoke_session(
        &mut self,
        token: SessionToken,
        target: SessionId,
    ) -> Result<(), OperationError> {
        self.authorize(&token)?;
        self.connection
            .execute("DELETE FROM sessions WHERE id = ?1", [target.0.to_string()])?;
        Ok(())
    }

    pub(super) fn create_bot(
        &mut self,
        token: SessionToken,
        input: NewBot,
    ) -> Result<Bot, OperationError> {
        self.authorize(&token)?;
        if let Some(bot) = self.find_bot(input.id)? {
            return if bot.name == input.name {
                Ok(bot)
            } else {
                Err(OperationError::Conflict)
            };
        }
        let bot = Bot {
            id: input.id,
            name: input.name,
            created_at: self.now(),
        };
        self.connection.execute(
            "INSERT INTO bots(id, name, created_at) VALUES(?1, ?2, ?3)",
            params![bot.id.0.to_string(), bot.name.as_str(), bot.created_at],
        )?;
        Ok(bot)
    }
    fn find_bot(&self, id: BotId) -> Result<Option<Bot>, OperationError> {
        Ok(self
            .connection
            .query_row(
                "SELECT name, created_at FROM bots WHERE id = ?1",
                [id.0.to_string()],
                |row| {
                    Ok(Bot {
                        id,
                        name: BotName(row.get(0)?),
                        created_at: row.get(1)?,
                    })
                },
            )
            .optional()?)
    }
    pub(super) fn get_bot(
        &mut self,
        token: SessionToken,
        id: BotId,
    ) -> Result<Bot, OperationError> {
        self.authorize(&token)?;
        self.find_bot(id)?.ok_or(OperationError::NotFound)
    }
    pub(super) fn list_bots(&mut self, token: SessionToken) -> Result<Vec<Bot>, OperationError> {
        self.authorize(&token)?;
        let mut statement = self
            .connection
            .prepare("SELECT id, name, created_at FROM bots ORDER BY created_at, id")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get(2)?,
            ))
        })?;
        rows.map(|row| {
            let (id, name, created_at) = row?;
            Ok(Bot {
                id: BotId(id.parse().map_err(|_| OperationError::Internal)?),
                name: BotName(name),
                created_at,
            })
        })
        .collect()
    }

    pub(super) fn connection_status(
        &mut self,
        token: SessionToken,
    ) -> Result<ConnectionStatus, OperationError> {
        self.authorize(&token)?;
        let vault = if matches!(
            self.installation,
            Installation::Configured {
                vault: VaultState::Unlocked { .. }
            }
        ) {
            VaultStatus::Unlocked
        } else {
            VaultStatus::Locked
        };
        let provider = self
            .connection
            .query_row(
                "SELECT updated_at FROM provider_keys WHERE provider = 'mistral'",
                [],
                |row| {
                    Ok(ProviderMetadata {
                        updated_at: row.get(0)?,
                    })
                },
            )
            .optional()?;
        Ok(ConnectionStatus { vault, provider })
    }
    pub(super) fn unlock_vault(
        &mut self,
        token: SessionToken,
        passphrase: VaultPassphrase,
    ) -> Result<(), OperationError> {
        self.authorize(&token)?;
        if !matches!(
            self.installation,
            Installation::Configured {
                vault: VaultState::Locked
            }
        ) {
            return Err(OperationError::Conflict);
        }
        self.unlock_attempts.admit()?;
        let ciphertext: Vec<u8> = self.connection.query_row(
            "SELECT identity_ciphertext FROM vault WHERE id = 1",
            [],
            |row| row.get(0),
        )?;
        let identity = crypto::unlock(&ciphertext, passphrase)?;
        self.installation = Installation::Configured {
            vault: VaultState::Unlocked { identity },
        };
        Ok(())
    }
    pub(super) fn lock_vault(&mut self, token: SessionToken) -> Result<(), OperationError> {
        self.authorize(&token)?;
        self.installation = Installation::Configured {
            vault: VaultState::Locked,
        };
        Ok(())
    }
    pub(super) fn replace_provider_key(
        &mut self,
        token: SessionToken,
        secret: ProviderSecret,
    ) -> Result<ProviderMetadata, OperationError> {
        self.authorize(&token)?;
        let Installation::Configured {
            vault: VaultState::Unlocked { identity },
        } = &self.installation
        else {
            return Err(OperationError::Locked);
        };
        let ciphertext = crypto::encrypt_provider(identity, secret)?;
        let metadata = ProviderMetadata {
            updated_at: self.now(),
        };
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("INSERT INTO provider_keys(provider, format_version, ciphertext, updated_at) VALUES('mistral', 1, ?1, ?2) ON CONFLICT(provider) DO UPDATE SET ciphertext = excluded.ciphertext, updated_at = excluded.updated_at",
            params![ciphertext, metadata.updated_at])?;
        tx.commit()?;
        Ok(metadata)
    }
}

fn remove_bootstrap(directory: &std::path::Path) -> Result<(), OperationError> {
    match fs::remove_file(directory.join("bootstrap.token")) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(OperationError::Internal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_input() -> OwnerSetup {
        OwnerSetup {
            name: OwnerName::new("Synthetic owner".into()).unwrap(),
            login_password: LoginPassword::new("synthetic login password".into()).unwrap(),
            vault_passphrase: VaultPassphrase::new("synthetic vault passphrase".into()).unwrap(),
            recovery: RecoveryAcknowledgment::OfflinePassphraseCopy,
        }
    }
    fn password() -> LoginPassword {
        LoginPassword::new("synthetic login password".into()).unwrap()
    }
    fn passphrase() -> VaultPassphrase {
        VaultPassphrase::new("synthetic vault passphrase".into()).unwrap()
    }
    fn token(grant: &SessionGrant) -> SessionToken {
        SessionToken::new(grant.token.0.expose_secret().into()).unwrap()
    }
    fn state() -> (tempfile::TempDir, State, BootstrapToken) {
        let temp = tempfile::tempdir().unwrap();
        let (configured, _) = watch::channel(false);
        let state = State::from_connection(
            Connection::open_in_memory().unwrap(),
            temp.path().into(),
            configured,
        )
        .unwrap();
        let proof =
            BootstrapToken::new(fs::read_to_string(temp.path().join("bootstrap.token")).unwrap())
                .unwrap();
        (temp, state, proof)
    }

    #[test]
    fn migrations_are_atomic_and_refuse_future_versions() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE vault(id INTEGER)").unwrap();
        assert!(migrate(&mut db).is_err());
        let count: i64 = db
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name = 'owner'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        assert_eq!(
            db.pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            0
        );
        db.pragma_update(None, "user_version", 3).unwrap();
        assert!(migrate(&mut db).is_err());
        assert_eq!(
            db.pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            3
        );
    }

    #[test]
    fn setup_and_provider_replacement_roll_back_without_losing_values() {
        let (_temp, mut state, proof) = state();
        state.connection.execute_batch("CREATE TRIGGER reject_vault BEFORE INSERT ON vault BEGIN SELECT RAISE(ABORT, 'test'); END;").unwrap();
        assert_eq!(
            state.create_owner(proof, setup_input()),
            Err(OperationError::Internal)
        );
        assert!(!state.is_configured());
        assert_eq!(
            state
                .connection
                .query_row::<i64, _, _>("SELECT count(*) FROM owner", [], |row| row.get(0))
                .unwrap(),
            0
        );
        state
            .connection
            .execute_batch("DROP TRIGGER reject_vault")
            .unwrap();
        let proof = BootstrapToken::new(
            fs::read_to_string(state.directory.join("bootstrap.token")).unwrap(),
        )
        .unwrap();
        state.create_owner(proof, setup_input()).unwrap();
        assert_eq!(
            state.create_owner(BootstrapToken::new("x".repeat(43)).unwrap(), setup_input()),
            Err(OperationError::Conflict)
        );
        assert!(
            state
                .connection
                .execute(
                    "INSERT INTO owner(id, name, password_hash) VALUES(2, 'other', 'unused')",
                    []
                )
                .is_err()
        );
        let grant = state.sign_in(password()).unwrap();
        state.unlock_vault(token(&grant), passphrase()).unwrap();
        state
            .replace_provider_key(
                token(&grant),
                ProviderSecret::new("synthetic original".into()).unwrap(),
            )
            .unwrap();
        let before: (Vec<u8>, i64) = state
            .connection
            .query_row(
                "SELECT ciphertext, updated_at FROM provider_keys",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        state.connection.execute_batch("CREATE TRIGGER reject_key BEFORE UPDATE ON provider_keys BEGIN SELECT RAISE(ABORT, 'test'); END;").unwrap();
        assert_eq!(
            state.replace_provider_key(
                token(&grant),
                ProviderSecret::new("synthetic replacement".into()).unwrap()
            ),
            Err(OperationError::Internal)
        );
        let after: (Vec<u8>, i64) = state
            .connection
            .query_row(
                "SELECT ciphertext, updated_at FROM provider_keys",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(before, after);
        let Installation::Configured {
            vault: VaultState::Unlocked { identity },
        } = &state.installation
        else {
            panic!("vault unlocked");
        };
        assert_eq!(
            &*crypto::decrypt_provider(identity, &after.0).unwrap(),
            b"synthetic original"
        );
        state
            .connection
            .execute_batch("DROP TRIGGER reject_key")
            .unwrap();
        state
            .replace_provider_key(
                token(&grant),
                ProviderSecret::new("synthetic replacement".into()).unwrap(),
            )
            .unwrap();
        let after: Vec<u8> = state
            .connection
            .query_row("SELECT ciphertext FROM provider_keys", [], |row| row.get(0))
            .unwrap();
        state.lock_vault(token(&grant)).unwrap();
        state.unlock_vault(token(&grant), passphrase()).unwrap();
        let Installation::Configured {
            vault: VaultState::Unlocked { identity },
        } = &state.installation
        else {
            panic!("vault unlocked");
        };
        assert_eq!(
            &*crypto::decrypt_provider(identity, &after).unwrap(),
            b"synthetic replacement"
        );
    }

    #[test]
    fn expiry_and_revocation_are_checked_at_operation_time() {
        let (_temp, mut state, proof) = state();
        state.create_owner(proof, setup_input()).unwrap();
        state.test_time = Some(1000);
        let first = state.sign_in(password()).unwrap();
        let second = state.sign_in(password()).unwrap();
        assert_eq!(first.session.expires_at, 1000 + 24 * 60 * 60);
        state.test_time = Some(first.session.expires_at - 1);
        assert!(state.owner(token(&first)).is_ok());
        state.test_time = Some(first.session.expires_at);
        assert_eq!(
            state.owner(token(&first)),
            Err(OperationError::Unauthorized)
        );
        state.test_time = Some(1001);
        state
            .revoke_session(token(&second), first.session.id)
            .unwrap();
        assert_eq!(
            state.get_bot(token(&first), BotId(Uuid::new_v4())),
            Err(OperationError::Unauthorized)
        );
        state.sign_out(token(&second)).unwrap();
        assert_eq!(
            state.owner(token(&second)),
            Err(OperationError::Unauthorized)
        );
    }

    #[test]
    fn credential_budget_resets_only_after_window() {
        let mut attempts = Attempts::new();
        for _ in 0..5 {
            attempts.admit().unwrap();
        }
        assert_eq!(attempts.admit(), Err(OperationError::RateLimited));
        attempts.start = Instant::now() - Duration::from_secs(61);
        assert!(attempts.admit().is_ok());
    }
}

#[cfg(test)]
mod migration_tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn populated_v1_preserves_owner_bot_and_encrypted_credentials() {
        let temp = tempfile::tempdir().unwrap();
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../../migrations/0001.sql"))
            .unwrap();
        let password = || LoginPassword::new("synthetic migration password".into()).unwrap();
        let passphrase = || VaultPassphrase::new("synthetic migration passphrase".into()).unwrap();
        let vault = crypto::new_identity(passphrase()).unwrap();
        let identity = crypto::unlock(&vault, passphrase()).unwrap();
        let credential = crypto::encrypt_provider(
            &identity,
            ProviderSecret::new("synthetic migration provider key".into()).unwrap(),
        )
        .unwrap();
        let bot = BotId::new();
        db.execute(
            "INSERT INTO owner VALUES(1, 'Migration owner', ?1)",
            [crypto::hash_password(&password()).unwrap()],
        )
        .unwrap();
        db.execute("INSERT INTO vault VALUES(1, 1, ?1)", [&vault])
            .unwrap();
        db.execute(
            "INSERT INTO provider_keys VALUES('mistral', 1, ?1, 123)",
            [&credential],
        )
        .unwrap();
        db.execute(
            "INSERT INTO bots VALUES(?1, 'Preserved bot', 123)",
            [bot.to_string()],
        )
        .unwrap();
        db.execute(
            "INSERT INTO sessions VALUES(?1, ?2, 1, 1, 9999999999)",
            params![
                Uuid::new_v4().to_string(),
                crypto::token_hash(&"s".repeat(43)).as_slice()
            ],
        )
        .unwrap();
        let (configured, _) = watch::channel(false);
        let mut state = State::from_connection(db, temp.path().into(), configured).unwrap();
        assert_eq!(
            state.owner(SessionToken::new("s".repeat(43)).unwrap()),
            Err(OperationError::Unauthorized)
        );
        let grant = state.sign_in(password()).unwrap();
        let token = || SessionToken::new(grant.token.0.expose_secret().into()).unwrap();
        assert_eq!(
            state.owner(token()).unwrap().name.as_str(),
            "Migration owner"
        );
        let preserved = state.get_bot(token(), bot).unwrap();
        assert_eq!(preserved.name.as_str(), "Preserved bot");
        assert_eq!(preserved.created_at, 123);
        assert_eq!(
            state.connection_status(token()).unwrap().vault,
            VaultStatus::Locked
        );
        state.unlock_vault(token(), passphrase()).unwrap();
        let after_vault: Vec<u8> = state
            .connection
            .query_row("SELECT identity_ciphertext FROM vault", [], |r| r.get(0))
            .unwrap();
        let after_credential: Vec<u8> = state
            .connection
            .query_row("SELECT ciphertext FROM provider_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(after_vault, vault);
        assert_eq!(after_credential, credential);
        let Installation::Configured {
            vault: VaultState::Unlocked { identity },
        } = &state.installation
        else {
            panic!("unlocked")
        };
        let mut plaintext = Vec::new();
        age::Decryptor::new(after_credential.as_slice())
            .unwrap()
            .decrypt(std::iter::once(identity as &dyn age::Identity))
            .unwrap()
            .read_to_end(&mut plaintext)
            .unwrap();
        assert_eq!(
            plaintext,
            b"botstral-provider-v1\0mistral\0synthetic migration provider key"
        );
        let fingerprint = botstral_link::CertificateFingerprint::from_der(b"synthetic certificate");
        let invitation = state
            .invite_worker(token(), fingerprint.clone(), 600)
            .unwrap();
        let stored: Vec<u8> = state
            .connection
            .query_row("SELECT token_hash FROM worker_invitations", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(
            stored,
            crypto::token_hash(invitation.secret.expose_secret())
        );
        assert!(
            !stored
                .windows(invitation.secret.expose_secret().len())
                .any(|v| v == invitation.secret.expose_secret().as_bytes())
        );
        state.sign_out(token()).unwrap();
        assert!(matches!(
            state.invite_worker(token(), fingerprint, 600),
            Err(OperationError::Unauthorized)
        ));
    }
}
