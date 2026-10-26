use super::*;
use botstral_link::{CertificateFingerprint, ComputerSession, ComputerSessionId, WorkerId};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct WorkerSummary {
    pub id: WorkerId,
    pub fingerprint: CertificateFingerprint,
    pub revoked: bool,
    pub generation: u64,
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
