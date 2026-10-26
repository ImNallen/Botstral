mod crypto;
mod state;

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
                let value = SecretString::from(value);
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BotId(pub Uuid);
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
