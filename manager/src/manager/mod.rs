mod crypto;
mod state;
mod workers;
pub use workers::*;

use age::secrecy::{ExposeSecret, SecretString};
use std::{path::PathBuf, sync::Arc, thread};
use tokio::sync::{Semaphore, mpsc, oneshot, watch};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationError {
    InvalidInput,
    Unauthorized,
    NotFound,
    Conflict,
    Locked,
    UnlockFailed,
    RateLimited,
    Busy,
    Internal,
}

macro_rules! secret_input {
    ($name:ident, $min:expr, $max:expr) => {
        pub struct $name(SecretString);
        impl $name {
            pub fn new(value: String) -> Result<Self, OperationError> {
                Self::from_secret(SecretString::from(value))
            }
            pub(crate) fn from_secret(value: SecretString) -> Result<Self, OperationError> {
                if !($min..=$max).contains(&value.expose_secret().len()) {
                    return Err(OperationError::InvalidInput);
                }
                Ok(Self(value))
            }
        }
    };
}
secret_input!(LoginPassword, 16, 1024);
secret_input!(VaultPassphrase, 16, 1024);
secret_input!(ProviderSecret, 1, 8192);
secret_input!(SessionToken, 43, 43);
secret_input!(BootstrapToken, 43, 43);

macro_rules! name_input {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct $name(String);
        impl $name {
            pub fn new(value: String) -> Result<Self, OperationError> {
                if !(1..=100).contains(&value.chars().count())
                    || value.chars().any(char::is_control)
                {
                    return Err(OperationError::InvalidInput);
                }
                Ok(Self(value))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}
name_input!(OwnerName);
name_input!(BotName);

pub use botstral_link::BotId;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionId(pub Uuid);

pub struct OwnerSetup {
    pub name: OwnerName,
    pub login_password: LoginPassword,
    pub vault_passphrase: VaultPassphrase,
    pub recovery: RecoveryAcknowledgment,
}
pub enum RecoveryAcknowledgment {
    OfflinePassphraseCopy,
}
#[derive(Debug, PartialEq, Eq)]
pub struct OwnerProfile {
    pub name: OwnerName,
}
#[derive(Debug, PartialEq, Eq)]
pub struct Bot {
    pub id: BotId,
    pub name: BotName,
    pub created_at: i64,
}
pub struct NewBot {
    pub id: BotId,
    pub name: BotName,
}
pub struct SessionGrant {
    pub token: SessionToken,
    pub session: SessionMetadata,
}
impl SessionGrant {
    pub(crate) fn into_response_parts(self) -> (SecretString, SessionMetadata) {
        (self.token.0, self.session)
    }
}
#[derive(Debug)]
pub struct SessionMetadata {
    pub id: SessionId,
    pub created_at: i64,
    pub expires_at: i64,
}
#[derive(Debug, PartialEq, Eq)]
pub struct ProviderMetadata {
    pub updated_at: i64,
}
#[derive(Debug, PartialEq, Eq)]
pub enum VaultStatus {
    Locked,
    Unlocked,
}
pub struct ConnectionStatus {
    pub vault: VaultStatus,
    pub provider: Option<ProviderMetadata>,
}

type Job = Box<dyn FnOnce(&mut state::State) + Send>;
#[derive(Clone)]
struct Queue {
    sender: mpsc::Sender<Job>,
    credential: Arc<Semaphore>,
}

impl Queue {
    async fn call<T: Send + 'static>(
        &self,
        expensive: bool,
        operation: impl FnOnce(&mut state::State) -> Result<T, OperationError> + Send + 'static,
    ) -> Result<T, OperationError> {
        let permit = if expensive {
            Some(
                self.credential
                    .clone()
                    .try_acquire_owned()
                    .map_err(|_| OperationError::Busy)?,
            )
        } else {
            None
        };
        let (reply, receive) = oneshot::channel();
        self.sender
            .try_send(Box::new(move |state| {
                let _permit = permit;
                let _ = reply.send(operation(state));
            }))
            .map_err(|_| OperationError::Busy)?;
        receive.await.map_err(|_| OperationError::Internal)?
    }
}

#[derive(Clone)]
pub struct Manager {
    queue: Queue,
}
#[derive(Clone)]
pub struct LocalSetup {
    queue: Queue,
    pub(crate) configured: watch::Receiver<bool>,
}

pub(crate) struct Runtime {
    pub manager: Manager,
    pub setup: Option<LocalSetup>,
    pub thread: thread::JoinHandle<()>,
}

impl Manager {
    pub(crate) fn start(directory: PathBuf) -> Result<Runtime, OperationError> {
        let (sender, mut receiver) = mpsc::channel::<Job>(32);
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let (configured_tx, configured) = watch::channel(false);
        let thread = thread::Builder::new()
            .name("botstral-state".into())
            .spawn(move || match state::State::open(directory, configured_tx) {
                Ok(mut state) => {
                    let _ = ready_tx.send(Ok(state.is_configured()));
                    while let Some(job) = receiver.blocking_recv() {
                        job(&mut state);
                    }
                }
                Err(error) => {
                    let _ = ready_tx.send(Err(error));
                }
            })
            .map_err(|_| OperationError::Internal)?;
        let ready = ready_rx.recv().map_err(|_| OperationError::Internal)?;
        match ready {
            Ok(is_configured) => {
                let queue = Queue {
                    sender,
                    credential: Arc::new(Semaphore::new(1)),
                };
                let setup = (!is_configured).then(|| LocalSetup {
                    queue: queue.clone(),
                    configured,
                });
                Ok(Runtime {
                    manager: Self { queue },
                    setup,
                    thread,
                })
            }
            Err(error) => {
                let _ = thread.join();
                Err(error)
            }
        }
    }

    pub async fn sign_in(&self, password: LoginPassword) -> Result<SessionGrant, OperationError> {
        self.queue
            .call(true, move |state| state.sign_in(password))
            .await
    }
    pub async fn owner(&self, token: SessionToken) -> Result<OwnerProfile, OperationError> {
        self.queue
            .call(false, move |state| state.owner(token))
            .await
    }
    pub async fn sessions(
        &self,
        token: SessionToken,
    ) -> Result<Vec<SessionMetadata>, OperationError> {
        self.queue
            .call(false, move |state| state.sessions(token))
            .await
    }
    pub async fn sign_out(&self, token: SessionToken) -> Result<(), OperationError> {
        self.queue
            .call(false, move |state| state.sign_out(token))
            .await
    }
    pub async fn revoke_session(
        &self,
        token: SessionToken,
        target: SessionId,
    ) -> Result<(), OperationError> {
        self.queue
            .call(false, move |state| state.revoke_session(token, target))
            .await
    }
    pub async fn create_bot(
        &self,
        token: SessionToken,
        input: NewBot,
    ) -> Result<Bot, OperationError> {
        self.queue
            .call(false, move |state| state.create_bot(token, input))
            .await
    }
    pub async fn get_bot(&self, token: SessionToken, id: BotId) -> Result<Bot, OperationError> {
        self.queue
            .call(false, move |state| state.get_bot(token, id))
            .await
    }
    pub async fn list_bots(&self, token: SessionToken) -> Result<Vec<Bot>, OperationError> {
        self.queue
            .call(false, move |state| state.list_bots(token))
            .await
    }
    pub async fn connection_status(
        &self,
        token: SessionToken,
    ) -> Result<ConnectionStatus, OperationError> {
        self.queue
            .call(false, move |state| state.connection_status(token))
            .await
    }
    pub async fn unlock_vault(
        &self,
        token: SessionToken,
        passphrase: VaultPassphrase,
    ) -> Result<(), OperationError> {
        self.queue
            .call(true, move |state| state.unlock_vault(token, passphrase))
            .await
    }
    pub async fn lock_vault(&self, token: SessionToken) -> Result<(), OperationError> {
        self.queue
            .call(false, move |state| state.lock_vault(token))
            .await
    }
    pub async fn replace_provider_key(
        &self,
        token: SessionToken,
        secret: ProviderSecret,
    ) -> Result<ProviderMetadata, OperationError> {
        self.queue
            .call(false, move |state| {
                state.replace_provider_key(token, secret)
            })
            .await
    }
}

impl LocalSetup {
    pub async fn create_owner(
        &self,
        proof: BootstrapToken,
        input: OwnerSetup,
    ) -> Result<OwnerProfile, OperationError> {
        self.queue
            .call(true, move |state| state.create_owner(proof, input))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn queue_saturation_and_credential_admission_are_bounded() {
        let temp = tempfile::tempdir().unwrap();
        let local = crate::LocalState::open(&temp.path().join("state")).unwrap();
        let Runtime {
            manager,
            setup,
            thread,
        } = Manager::start(local.directory().to_owned()).unwrap();
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let queue = manager.queue.clone();
        let blocker = tokio::spawn(async move {
            queue
                .call(true, move |_| {
                    let _ = entered_tx.send(());
                    let _ = release_rx.recv();
                    Ok(())
                })
                .await
        });
        entered_rx.await.unwrap();
        assert!(matches!(
            manager
                .sign_in(LoginPassword::new("synthetic login password".into()).unwrap())
                .await,
            Err(OperationError::Busy)
        ));
        let mut pending = tokio::task::JoinSet::new();
        for _ in 0..32 {
            let manager = manager.clone();
            pending.spawn(async move {
                manager
                    .owner(SessionToken::new("x".repeat(43)).unwrap())
                    .await
            });
        }
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while manager.queue.sender.capacity() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            manager
                .owner(SessionToken::new("x".repeat(43)).unwrap())
                .await,
            Err(OperationError::Busy)
        );
        release_tx.send(()).unwrap();
        blocker.await.unwrap().unwrap();
        while let Some(result) = pending.join_next().await {
            assert_eq!(result.unwrap(), Err(OperationError::Unauthorized));
        }
        assert_eq!(
            manager
                .owner(SessionToken::new("x".repeat(43)).unwrap())
                .await,
            Err(OperationError::Unauthorized)
        );
        drop(setup);
        drop(manager);
        tokio::task::spawn_blocking(move || thread.join().unwrap())
            .await
            .unwrap();
    }
}
