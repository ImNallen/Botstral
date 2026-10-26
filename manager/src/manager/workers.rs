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
        self.queue
            .call(false, move |s| {
                s.disconnect_worker(lease);
                Ok(())
            })
            .await
    }
}

impl From<botstral_link::ErrorCode> for OperationError {
    fn from(code: botstral_link::ErrorCode) -> Self {
        Self::Protocol(code)
    }
}
