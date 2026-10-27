pub(crate) mod coding;
mod crypto;
pub(crate) mod devices;
mod inference;
mod provider;
mod runs;
mod state;
mod tasks;
mod vision;
mod workers;
pub use devices::DeviceInvitationGrant;
use devices::OwnerAuthority;
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
    InferenceUnverified,
    InferenceBudget,
    Protocol(botstral_link::ErrorCode),
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
    async fn call_wait<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut state::State) -> Result<T, OperationError> + Send + 'static,
    ) -> Result<T, OperationError> {
        let (reply, receive) = oneshot::channel();
        self.sender
            .send(Box::new(move |state| {
                let _ = reply.send(operation(state));
            }))
            .await
            .map_err(|_| OperationError::Internal)?;
        receive.await.map_err(|_| OperationError::Internal)?
    }

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
                let result = operation(state);
                drop(permit);
                let _ = reply.send(result);
            }))
            .map_err(|_| OperationError::Busy)?;
        receive.await.map_err(|_| OperationError::Internal)?
    }
}

#[derive(Clone)]
pub struct Manager {
    queue: Queue,
    inference: Arc<inference::InferenceControl>,
    provider: provider::Provider,
    events: watch::Receiver<u64>,
    pub(crate) snapshots: Arc<Semaphore>,
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
    pub inference: inference::InferenceRuntime,
}

impl Manager {
    pub(crate) fn start(directory: PathBuf) -> Result<Runtime, OperationError> {
        Self::start_with_provider(directory, provider::Provider::new()?)
    }

    pub(crate) fn start_with_provider(
        directory: PathBuf,
        provider: provider::Provider,
    ) -> Result<Runtime, OperationError> {
        let (sender, mut receiver) = mpsc::channel::<Job>(32);
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let (configured_tx, configured) = watch::channel(false);
        let (event_tx, events) = watch::channel(0);
        let control = Arc::new(inference::InferenceControl::default());
        let state_wake = control.clone();
        let thread = thread::Builder::new()
            .name("botstral-state".into())
            .spawn(move || match state::State::open(directory, configured_tx) {
                Ok(mut state) => {
                    event_tx.send_replace(state.change_sequence().unwrap_or(0) as u64);
                    state.set_inference_control(state_wake.clone());
                    let _ = ready_tx.send(Ok(state.is_configured()));
                    while let Some(job) = receiver.blocking_recv() {
                        let before = state.change_sequence();
                        job(&mut state);
                        if before != state.change_sequence() {
                            event_tx.send_replace(state.change_sequence().unwrap_or(0) as u64);
                            state_wake.run_wake.notify_one();
                        }
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
                let inference = inference::InferenceRuntime::start(
                    queue.clone(),
                    provider.clone(),
                    control.clone(),
                );
                Ok(Runtime {
                    manager: Self {
                        queue,
                        inference: control,
                        provider,
                        events,
                        snapshots: Arc::new(Semaphore::new(1)),
                    },
                    setup,
                    thread,
                    inference,
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
    async fn credential_admission_is_released_before_completion_wakes_the_caller() {
        use std::{
            future::Future,
            sync::atomic::{AtomicUsize, Ordering},
            task::{Context, Wake, Waker},
        };
        struct AdmissionWake {
            credential: Arc<Semaphore>,
            observed: AtomicUsize,
        }
        impl Wake for AdmissionWake {
            fn wake(self: Arc<Self>) {
                self.observed
                    .store(self.credential.available_permits(), Ordering::SeqCst);
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let local = crate::LocalState::open(&temp.path().join("state")).unwrap();
        let Runtime {
            manager,
            setup,
            thread,
            inference,
        } = Manager::start(local.directory().to_owned()).unwrap();
        let observer = Arc::new(AdmissionWake {
            credential: manager.queue.credential.clone(),
            observed: AtomicUsize::new(usize::MAX),
        });
        let waker = Waker::from(observer.clone());
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let mut call = Box::pin(manager.queue.call(true, move |_| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(())
        }));
        assert!(
            call.as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        entered_rx.await.unwrap();
        release_tx.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while observer.observed.load(Ordering::SeqCst) == usize::MAX {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            observer.observed.load(Ordering::SeqCst),
            1,
            "completed credential work still holds admission when its caller wakes"
        );
        call.await.unwrap();
        drop(observer);
        drop(manager);
        drop(setup);
        inference.shutdown().await.unwrap();
        thread.join().unwrap();
    }

    #[tokio::test]
    async fn queue_saturation_and_credential_admission_are_bounded() {
        let temp = tempfile::tempdir().unwrap();
        let local = crate::LocalState::open(&temp.path().join("state")).unwrap();
        let Runtime {
            manager,
            setup,
            thread,
            inference,
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
        let (waited, outcome) = {
            let gateway = manager.worker_gateway();
            let delivery = gateway.admit_delivery(
                ConnectionLease {
                    worker: botstral_link::WorkerId::new(),
                    generation: botstral_link::Generation::INITIAL,
                },
                botstral_link::ManagerMessage::Pong,
            );
            tokio::pin!(delivery);
            let early =
                tokio::time::timeout(std::time::Duration::from_millis(20), &mut delivery).await;
            let waited = early.is_err();
            release_tx.send(()).unwrap();
            blocker.await.unwrap().unwrap();
            let outcome = match early {
                Ok(outcome) => outcome,
                Err(_) => delivery.await,
            };
            (waited, outcome)
        };
        while let Some(result) = pending.join_next().await {
            assert!(matches!(
                result.unwrap(),
                Err(OperationError::Unauthorized | OperationError::Busy)
            ));
        }
        assert!(waited, "delivery bypassed queue admission with {outcome:?}");
        assert_eq!(
            outcome,
            Err(OperationError::Protocol(
                botstral_link::ErrorCode::Disconnected
            ))
        );
        assert_eq!(
            manager
                .owner(SessionToken::new("x".repeat(43)).unwrap())
                .await,
            Err(OperationError::Unauthorized)
        );
        drop(setup);
        drop(manager);
        inference.shutdown().await.unwrap();
        tokio::task::spawn_blocking(move || thread.join().unwrap())
            .await
            .unwrap();
    }
}
