use super::*;
use botstral_link::{CertificateFingerprint, ComputerSession, ComputerSessionId, WorkerId};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct WorkerSummary {
    pub id: WorkerId,
    pub fingerprint: CertificateFingerprint,
    pub revoked: bool,
    pub generation: u64,
    pub phase: WorkerPhase,
}

pub struct InvitationGrant {
    pub secret: SecretString,
    pub expires_at: i64,
}

impl Manager {
    pub async fn invite_worker(
        &self,
        token: SessionToken,
        fingerprint: CertificateFingerprint,
        lifetime_seconds: u32,
    ) -> Result<InvitationGrant, OperationError> {
        self.queue
            .call(false, move |s| {
                s.invite_worker(token, fingerprint, lifetime_seconds)
            })
            .await
    }

    pub async fn workers(&self, token: SessionToken) -> Result<Vec<WorkerSummary>, OperationError> {
        self.queue.call(false, move |s| s.workers(token)).await
    }

    pub async fn revoke_worker(
        &self,
        token: SessionToken,
        id: WorkerId,
    ) -> Result<(), OperationError> {
        self.queue
            .call(false, move |s| s.revoke_worker(token, id))
            .await
    }

    pub async fn open_computer_session(
        &self,
        token: SessionToken,
        bot: BotId,
        worker: WorkerId,
    ) -> Result<ComputerSession, OperationError> {
        self.queue
            .call(false, move |s| s.open_computer_session(token, bot, worker))
            .await
    }

    pub async fn computer_session(
        &self,
        token: SessionToken,
        id: ComputerSessionId,
    ) -> Result<ComputerSession, OperationError> {
        self.queue
            .call(false, move |s| s.authorized_computer_session(token, id))
            .await
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerPhase {
    Disconnected,
    Synchronizing,
    Ready,
}

#[derive(Clone, Copy)]
pub(crate) struct ConnectionLease {
    pub worker: WorkerId,
    pub generation: botstral_link::Generation,
}

#[derive(Clone)]
pub(crate) struct WorkerGateway {
    queue: Queue,
}

impl Manager {
    pub(crate) fn worker_gateway(&self) -> WorkerGateway {
        WorkerGateway {
            queue: self.queue.clone(),
        }
    }
}

impl WorkerGateway {
    pub(crate) async fn enroll(
        &self,
        peer: crate::worker_http::VerifiedIdentity,
        secret: SecretString,
    ) -> Result<WorkerId, OperationError> {
        self.queue
            .call(false, move |s| s.enroll_worker(peer, secret))
            .await
    }
    pub(crate) async fn connect(
        &self,
        peer: crate::worker_http::VerifiedIdentity,
        hello: botstral_link::Hello,
        outbound: mpsc::Sender<botstral_link::Envelope<botstral_link::ManagerMessage>>,
        stop: watch::Sender<bool>,
        request_id: botstral_link::RequestId,
    ) -> Result<ConnectionLease, OperationError> {
        self.queue
            .call(false, move |s| {
                s.connect_worker(peer, hello, outbound, stop, request_id)
            })
            .await
    }
    pub(crate) async fn receive(
        &self,
        lease: ConnectionLease,
        report: botstral_link::Envelope<botstral_link::WorkerMessage>,
    ) -> Result<(), OperationError> {
        self.queue
            .call(false, move |s| s.worker_report(lease, report))
            .await
    }
    pub(crate) async fn disconnected(&self, lease: ConnectionLease) -> Result<(), OperationError> {
        let (reply, receive) = oneshot::channel();
        self.queue
            .sender
            .send(Box::new(move |state| {
                state.disconnect_worker(lease);
                let _ = reply.send(());
            }))
            .await
            .map_err(|_| OperationError::Internal)?;
        receive.await.map_err(|_| OperationError::Internal)
    }
}

impl From<botstral_link::ErrorCode> for OperationError {
    fn from(code: botstral_link::ErrorCode) -> Self {
        Self::Protocol(code)
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerInput {
    pub id: botstral_link::CommandId,
    pub fence: botstral_link::InputFence,
    pub value: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct DispatchReceipt {
    pub command_id: botstral_link::CommandId,
    pub generation: botstral_link::Generation,
    pub acknowledgment: botstral_link::Acknowledgment,
    pub last_event: Option<botstral_link::TaskEvent>,
    pub bot_inputs: u32,
}

impl Manager {
    pub async fn set_input_owner(
        &self,
        token: SessionToken,
        id: ComputerSessionId,
        expected: botstral_link::Revision,
        owner: botstral_link::InputOwner,
    ) -> Result<ComputerSession, OperationError> {
        self.queue
            .call(false, move |s| {
                s.set_input_owner(token, id, expected, owner)
            })
            .await
    }
    pub async fn command(
        &self,
        token: SessionToken,
        command: botstral_link::Command,
    ) -> Result<DispatchReceipt, OperationError> {
        self.queue
            .call(false, move |s| {
                s.dispatch(token, botstral_link::ManagerMessage::Command(command))
            })
            .await
    }
    pub async fn input(
        &self,
        token: SessionToken,
        input: OwnerInput,
    ) -> Result<DispatchReceipt, OperationError> {
        self.queue
            .call(false, move |s| {
                s.dispatch(
                    token,
                    botstral_link::ManagerMessage::Input {
                        command_id: input.id,
                        fence: input.fence,
                        value: input.value,
                    },
                )
            })
            .await
    }
    pub async fn receipt(
        &self,
        token: SessionToken,
        worker: WorkerId,
        id: botstral_link::CommandId,
    ) -> Result<DispatchReceipt, OperationError> {
        self.queue
            .call(false, move |s| s.receipt(token, worker, id))
            .await
    }
}

impl WorkerGateway {
    pub(crate) async fn admit_delivery(
        &self,
        lease: ConnectionLease,
        message: botstral_link::ManagerMessage,
    ) -> Result<(), OperationError> {
        let (reply, receive) = oneshot::channel();
        self.queue
            .sender
            .send(Box::new(move |state| {
                let _ = reply.send(state.admit_delivery(lease, &message));
            }))
            .await
            .map_err(|_| OperationError::Internal)?;
        receive.await.map_err(|_| OperationError::Internal)?
    }
}
