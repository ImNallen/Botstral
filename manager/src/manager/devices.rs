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

pub(crate) struct MaterializedOwnerReply {
    pub(crate) reply: OwnerReply,
    pub(crate) permit: Arc<tokio::sync::OwnedSemaphorePermit>,
}

pub(crate) fn owner_error(error: OperationError) -> OwnerError {
    match error {
        OperationError::InvalidInput => OwnerError::InvalidInput,
        OperationError::Unauthorized => OwnerError::Unauthorized,
        OperationError::NotFound => OwnerError::NotFound,
        OperationError::Conflict => OwnerError::Conflict,
        OperationError::Locked => OwnerError::Locked,
        OperationError::UnlockFailed => OwnerError::UnlockFailed,
        OperationError::RateLimited => OwnerError::RateLimited,
        OperationError::Busy => OwnerError::Busy,
        OperationError::Internal => OwnerError::Internal,
        OperationError::InferenceUnverified => OwnerError::InferenceUnverified,
        OperationError::InferenceBudget => OwnerError::InferenceBudget,
        OperationError::Protocol(code) => OwnerError::Protocol(code),
    }
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
        materialization: Arc<tokio::sync::OwnedSemaphorePermit>,
    ) -> Result<MaterializedOwnerReply, OperationError> {
        if let OwnerOperation::VerifyInference { input } = operation {
            drop(materialization);
            let permit = self
                .queue
                .call(false, move |s| {
                    s.prepare_inference_verification(lease, input)
                })
                .await?;
            let _capacity = self
                .provider
                .calls
                .clone()
                .try_acquire_owned()
                .map_err(|_| OperationError::Busy)?;
            let verified = self.provider.verify(&permit).await;
            drop(_capacity);
            let config = permit.configuration;
            let epoch = permit.epoch;
            let materialization = Arc::new(
                self.snapshots
                    .clone()
                    .acquire_owned()
                    .await
                    .map_err(|_| OperationError::Internal)?,
            );
            let reply = self
                .queue
                .call_wait(move |s| {
                    s.authorize_owner(&OwnerAuthority::Device(lease))?;
                    Ok(OwnerReply::Configuration(
                        s.finish_inference_verification(config, epoch, verified)?,
                    ))
                })
                .await?;
            return Ok(MaterializedOwnerReply {
                reply,
                permit: materialization,
            });
        }
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
                let reply = OwnerReply::Artifact {
                    declaration,
                    base64: base64::engine::general_purpose::STANDARD.encode(bytes),
                };
                Ok(MaterializedOwnerReply {
                    reply,
                    permit: materialization,
                })
            })
            .await
            .map_err(|_| OperationError::Internal)?;
        }
        let control = self.inference.clone();
        let reply = self
            .queue
            .call(false, move |s| {
                s.authorize_owner(&OwnerAuthority::Device(lease))?;
                if matches!(
                    &operation,
                    OwnerOperation::SubmitRun { .. } | OwnerOperation::SubmitInference { .. }
                ) && control.stopping.load(std::sync::atomic::Ordering::Acquire)
                {
                    return Err(OperationError::Busy);
                }
                let result = s.owner_operation(lease, operation);
                control.run_wake.notify_one();
                control.notify_inference();
                result
            })
            .await?;
        Ok(MaterializedOwnerReply {
            reply,
            permit: materialization,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn remote_model_admission_preserves_shutdown_and_revocation_fences() {
        let root = tempfile::tempdir().unwrap();
        let local = crate::LocalState::open(&root.path().join("state")).unwrap();
        let Runtime {
            manager,
            setup,
            thread,
            inference,
        } = Manager::start(local.directory().into()).unwrap();
        let setup = setup.unwrap();
        let proof = BootstrapToken::new(
            std::fs::read_to_string(local.directory().join("bootstrap.token")).unwrap(),
        )
        .unwrap();
        setup
            .create_owner(
                proof,
                OwnerSetup {
                    name: OwnerName::new("Synthetic owner".into()).unwrap(),
                    login_password: LoginPassword::new("synthetic login password".into()).unwrap(),
                    vault_passphrase: VaultPassphrase::new("synthetic vault passphrase".into())
                        .unwrap(),
                    recovery: RecoveryAcknowledgment::OfflinePassphraseCopy,
                },
            )
            .await
            .unwrap();
        let grant = manager
            .sign_in(LoginPassword::new("synthetic login password".into()).unwrap())
            .await
            .unwrap();
        let token = || SessionToken::new(grant.token.0.expose_secret().into()).unwrap();
        let credential = crypto::random_token();
        let invitation = manager
            .invite_device(
                token(),
                PairingRequest {
                    fingerprint: DeviceFingerprint::from_credential(&credential),
                    label: "Shutdown fixture".into(),
                },
                600,
            )
            .await
            .unwrap();
        let (_, lease) = manager
            .connect_owner(
                OwnerHello::Enroll {
                    version: OWNER_VERSION,
                    manager: invitation.manager,
                    role: DeviceRole::Owner,
                    credential,
                    invitation: invitation.secret.expose_secret().into(),
                },
                watch::channel(false).0,
            )
            .await
            .unwrap();
        manager
            .inference
            .stopping
            .store(true, std::sync::atomic::Ordering::Release);
        let operation = || OwnerOperation::SubmitInference {
            input: SubmitInference {
                id: InferenceId::new(),
                bot: BotId::new(),
                configuration_revision: 1,
                prompt: "Never admitted".into(),
                deadline: 1,
                max_output_tokens: 1,
                max_total_tokens: 1,
                max_cost_microusd: 0,
            },
        };
        assert!(matches!(
            manager
                .owner_operation(
                    lease,
                    operation(),
                    Arc::new(manager.snapshots.clone().acquire_owned().await.unwrap())
                )
                .await,
            Err(OperationError::Busy)
        ));
        manager
            .revoke_device(token(), lease.device())
            .await
            .unwrap();
        assert!(matches!(
            manager
                .owner_operation(
                    lease,
                    operation(),
                    Arc::new(manager.snapshots.clone().acquire_owned().await.unwrap())
                )
                .await,
            Err(OperationError::Unauthorized)
        ));
        drop(manager);
        drop(setup);
        inference.shutdown().await.unwrap();
        tokio::task::spawn_blocking(move || thread.join().unwrap())
            .await
            .unwrap();
    }
}
