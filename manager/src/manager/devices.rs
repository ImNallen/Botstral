use super::*;
use botstral_link::*;

pub(super) enum OwnerAuthority {
    Local(SessionToken),
    Device(DeviceLease),
}
impl From<SessionToken> for OwnerAuthority {
    fn from(token: SessionToken) -> Self {
        Self::Local(token)
    }
}
impl From<DeviceLease> for OwnerAuthority {
    fn from(lease: DeviceLease) -> Self {
        Self::Device(lease)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct DeviceLease {
    pub(super) device: DeviceId,
    pub(super) epoch: i64,
    pub(super) connection: OwnerConnectionId,
}
impl DeviceLease {
    pub(crate) fn device(self) -> DeviceId {
        self.device
    }
}

pub struct DeviceInvitationGrant {
    pub manager: ManagerId,
    pub fingerprint: DeviceFingerprint,
    pub role: DeviceRole,
    pub expires_at: i64,
    pub secret: SecretString,
}

impl Manager {
    pub async fn manager_identity(&self, token: SessionToken) -> Result<ManagerId, OperationError> {
        self.queue
            .call(false, move |s| {
                s.authorize(&token)?;
                s.manager_identity()
            })
            .await
    }
    pub async fn invite_device(
        &self,
        token: SessionToken,
        request: PairingRequest,
        lifetime: u32,
    ) -> Result<DeviceInvitationGrant, OperationError> {
        self.queue
            .call(false, move |s| s.invite_device(token, request, lifetime))
            .await
    }
    pub async fn owner_devices(
        &self,
        token: SessionToken,
    ) -> Result<Vec<OwnerDevice>, OperationError> {
        self.queue
            .call(false, move |s| s.owner_devices(token))
            .await
    }
    pub async fn revoke_device(
        &self,
        token: SessionToken,
        device: DeviceId,
    ) -> Result<(), OperationError> {
        self.queue
            .call(false, move |s| s.revoke_device(token, device))
            .await
    }
    pub(crate) fn subscribe_owner_events(&self) -> watch::Receiver<u64> {
        self.events.clone()
    }
    pub(crate) async fn connect_owner(
        &self,
        hello: OwnerHello,
        stop: watch::Sender<bool>,
    ) -> Result<(ManagerId, DeviceLease), OperationError> {
        self.queue
            .call(false, move |s| s.connect_owner(hello, stop))
            .await
    }
    pub(crate) async fn disconnect_owner(&self, lease: DeviceLease) -> Result<(), OperationError> {
        self.queue
            .call_wait(move |s| {
                s.disconnect_owner(lease);
                Ok(())
            })
            .await
    }
    pub(crate) async fn owner_operation(
        &self,
        lease: DeviceLease,
        operation: OwnerOperation,
    ) -> Result<OwnerReply, OperationError> {
        if let OwnerOperation::Artifact { task, id } = operation {
            let (mut file, declaration, length) = self
                .queue
                .call(false, move |s| s.download_artifact(lease, task, id))
                .await?;
            return tokio::task::spawn_blocking(move || {
                use base64::Engine;
                use std::io::Read;
                let mut bytes = Vec::new();
                file.by_ref()
                    .take(MAX_ARTIFACT_BYTES + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| OperationError::Internal)?;
                if bytes.len() as u64 != length || length > MAX_ARTIFACT_BYTES {
                    return Err(OperationError::Conflict);
                }
                Ok(OwnerReply::Artifact {
                    declaration,
                    base64: base64::engine::general_purpose::STANDARD.encode(bytes),
                })
            })
            .await
            .map_err(|_| OperationError::Internal)?;
        }
        let control = self.inference.clone();
        self.queue
            .call(false, move |s| {
                s.authorize_owner(&OwnerAuthority::Device(lease))?;
                let result = s.owner_operation(lease, operation);
                control.run_wake.notify_one();
                control.notify_inference();
                result
            })
            .await
    }
}
