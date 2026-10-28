use super::*;
use crate::manager::devices::{DeviceInvitationGrant, DeviceLease, OwnerAuthority};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use botstral_link::*;
use sha2::{Digest, Sha256};

pub(super) struct DeviceConnection {
    pub lease: DeviceLease,
    pub stop: watch::Sender<bool>,
}

fn invitation_hash(secret: &str) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"botstral-owner-invitation-v1\0");
    digest.update(secret.as_bytes());
    digest.finalize().into()
}

#[cfg(test)]
mod tests;

fn fingerprint_bytes(fingerprint: &DeviceFingerprint) -> [u8; 32] {
    let mut hash = [0; 32];
    for (i, byte) in hash.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&fingerprint.as_str()[i * 2..i * 2 + 2], 16)
            .expect("validated fingerprint");
    }
    hash
}
fn credential_hash(credential: &str) -> Result<[u8; 32], OperationError> {
    let decoded = URL_SAFE_NO_PAD
        .decode(credential)
        .map_err(|_| OperationError::Unauthorized)?;
    if decoded.len() != 32 || URL_SAFE_NO_PAD.encode(&decoded) != credential {
        return Err(OperationError::Unauthorized);
    }
    Ok(fingerprint_bytes(&DeviceFingerprint::from_credential(
        credential,
    )))
}

impl State {
    pub(in crate::manager) fn manager_identity(&self) -> Result<ManagerId, OperationError> {
        let id: String = self.connection.query_row(
            "SELECT identity FROM manager_identity WHERE id=1",
            [],
            |r| r.get(0),
        )?;
        id.parse().map_err(|_| OperationError::Internal)
    }
    pub(in crate::manager) fn authorize_owner(
        &self,
        authority: &OwnerAuthority,
    ) -> Result<(), OperationError> {
        match authority {
            OwnerAuthority::Local(token) => {
                self.authorize(token)?;
                Ok(())
            }
            OwnerAuthority::Device(lease) => {
                let registered = self
                    .owner_connections
                    .get(&lease.connection)
                    .is_some_and(|c| c.lease == *lease && !*c.stop.borrow());
                let durable: bool = self.connection.query_row("SELECT EXISTS(SELECT 1 FROM owner_devices WHERE id=?1 AND epoch=?2 AND revoked=0 AND role='owner')", params![lease.device.to_string(),lease.epoch], |r| r.get(0))?;
                if registered && durable {
                    Ok(())
                } else {
                    Err(OperationError::Unauthorized)
                }
            }
        }
    }
    pub(in crate::manager) fn invite_device(
        &mut self,
        token: SessionToken,
        request: PairingRequest,
        lifetime: u32,
    ) -> Result<DeviceInvitationGrant, OperationError> {
        self.authorize(&token)?;
        if !(1..=600).contains(&lifetime)
            || request.label.is_empty()
            || request.label.len() > 100
            || request.label.chars().any(char::is_control)
        {
            return Err(OperationError::InvalidInput);
        }
        let now = self.now();
        self.connection
            .execute("DELETE FROM owner_invitations WHERE expires_at<=?1", [now])?;
        let count: i64 =
            self.connection
                .query_row("SELECT count(*) FROM owner_invitations", [], |r| r.get(0))?;
        if count >= 128 {
            return Err(OperationError::Busy);
        }
        let hash = fingerprint_bytes(&request.fingerprint);
        let exists: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM owner_devices WHERE credential_hash=?1)",
            [hash.as_slice()],
            |r| r.get(0),
        )?;
        if exists {
            return Err(OperationError::Conflict);
        }
        let manager = self.manager_identity()?;
        let secret = SecretString::from(crypto::random_token());
        let expires_at = now + i64::from(lifetime);
        self.connection.execute("INSERT INTO owner_invitations(token_hash,manager,role,credential_hash,label,expires_at) VALUES(?1,?2,'owner',?3,?4,?5)", params![invitation_hash(secret.expose_secret()).as_slice(),manager.to_string(),hash.as_slice(),request.label,expires_at])?;
        Ok(DeviceInvitationGrant {
            manager,
            fingerprint: request.fingerprint,
            role: DeviceRole::Owner,
            expires_at,
            secret,
        })
    }
    pub(super) fn remote_attempt(&mut self) -> Result<(), OperationError> {
        let now = self.now();
        let (start, count): (i64, i64) = self.connection.query_row(
            "SELECT window_start,attempts FROM remote_attempts WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if now >= start.saturating_add(60) {
            self.connection.execute(
                "UPDATE remote_attempts SET window_start=?1,attempts=1 WHERE id=1",
                [now],
            )?;
        } else if count >= 120 {
            return Err(OperationError::RateLimited);
        } else {
            self.connection.execute(
                "UPDATE remote_attempts SET attempts=attempts+1 WHERE id=1",
                [],
            )?;
        }
        Ok(())
    }
    pub(in crate::manager) fn connect_owner(
        &mut self,
        hello: OwnerHello,
        stop: watch::Sender<bool>,
    ) -> Result<(ManagerId, DeviceLease), OperationError> {
        let result = self.authenticate_owner(hello, stop);
        if result.is_err() {
            self.remote_attempt()?;
        }
        result
    }
    fn authenticate_owner(
        &mut self,
        hello: OwnerHello,
        stop: watch::Sender<bool>,
    ) -> Result<(ManagerId, DeviceLease), OperationError> {
        if !self.is_configured() {
            return Err(OperationError::Unauthorized);
        }
        let (version, manager, role, credential, invitation) = match hello {
            OwnerHello::Authenticate {
                version,
                manager,
                role,
                credential,
            } => (version, manager, role, credential, None),
            OwnerHello::Enroll {
                version,
                manager,
                role,
                credential,
                invitation,
            } => (version, manager, role, credential, Some(invitation)),
        };
        let credential = SecretString::from(credential);
        if version != OWNER_VERSION {
            return Err(ErrorCode::IncompatibleVersion.into());
        }
        if manager != self.manager_identity()? || role != DeviceRole::Owner {
            return Err(OperationError::Unauthorized);
        }
        let hash = credential_hash(credential.expose_secret())?;
        if self.owner_connections.len() >= MAX_OWNER_CONNECTIONS {
            return Err(OperationError::Busy);
        }
        if let Some(invitation) = invitation {
            let invitation = SecretString::from(invitation);
            if invitation.expose_secret().len() != 43 {
                return Err(OperationError::Unauthorized);
            }
            let now = self.now();
            let tx = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let label: Option<String> = tx.query_row("SELECT label FROM owner_invitations WHERE token_hash=?1 AND manager=?2 AND role='owner' AND credential_hash=?3 AND expires_at>?4 AND consumed_at IS NULL", params![invitation_hash(invitation.expose_secret()).as_slice(),manager.to_string(),hash.as_slice(),now], |r| r.get(0)).optional()?;
            let label = label.ok_or(OperationError::Unauthorized)?;
            let existing: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM owner_devices WHERE credential_hash=?1)",
                [hash.as_slice()],
                |r| r.get(0),
            )?;
            if existing {
                return Err(OperationError::Unauthorized);
            }
            let count: i64 =
                tx.query_row("SELECT count(*) FROM owner_devices", [], |r| r.get(0))?;
            if count >= MAX_OWNER_DEVICES as i64 {
                return Err(OperationError::Busy);
            }
            tx.execute("INSERT INTO owner_devices(id,credential_hash,label,role,created_at) VALUES(?1,?2,?3,'owner',?4)", params![DeviceId::new().to_string(),hash.as_slice(),label,now])?;
            tx.execute(
                "UPDATE owner_invitations SET consumed_at=?1 WHERE token_hash=?2",
                params![now, invitation_hash(invitation.expose_secret()).as_slice()],
            )?;
            tx.commit()?;
        }
        let (id, epoch): (String,i64) = self.connection.query_row("SELECT id,epoch FROM owner_devices WHERE credential_hash=?1 AND role='owner' AND revoked=0", [hash.as_slice()], |r| Ok((r.get(0)?,r.get(1)?))).optional()?.ok_or(OperationError::Unauthorized)?;
        let lease = DeviceLease {
            device: id.parse().map_err(|_| OperationError::Internal)?,
            epoch,
            connection: OwnerConnectionId::new(),
        };
        self.owner_connections
            .insert(lease.connection, DeviceConnection { lease, stop });
        Ok((manager, lease))
    }
    pub(in crate::manager) fn disconnect_owner(&mut self, lease: DeviceLease) {
        if self
            .owner_connections
            .get(&lease.connection)
            .is_some_and(|c| c.lease == lease)
        {
            self.owner_connections.remove(&lease.connection);
        }
    }
    pub(in crate::manager) fn revoke_device(
        &mut self,
        token: SessionToken,
        device: DeviceId,
    ) -> Result<(), OperationError> {
        self.authorize(&token)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if tx.execute(
            "UPDATE owner_devices SET revoked=1,epoch=epoch+1 WHERE id=?1",
            [device.to_string()],
        )? == 0
        {
            return Err(OperationError::NotFound);
        }
        tx.commit()?;
        self.owner_connections.retain(|_, c| {
            if c.lease.device == device {
                c.stop.send_replace(true);
                false
            } else {
                true
            }
        });
        Ok(())
    }
    pub(in crate::manager) fn owner_devices(
        &self,
        token: SessionToken,
    ) -> Result<Vec<OwnerDevice>, OperationError> {
        self.authorize(&token)?;
        self.connection
            .prepare(
                "SELECT id,label,credential_hash,revoked,created_at FROM owner_devices ORDER BY id",
            )?
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                    r.get(3)?,
                    r.get(4)?,
                ))
            })?
            .map(|row| {
                let (id, label, hash, revoked, created_at) = row?;
                let digest: String = hash.iter().map(|b| format!("{b:02x}")).collect();
                Ok(OwnerDevice {
                    id: id.parse().map_err(|_| OperationError::Internal)?,
                    label,
                    fingerprint: digest.try_into().map_err(|_| OperationError::Internal)?,
                    role: DeviceRole::Owner,
                    revoked,
                    created_at,
                })
            })
            .collect()
    }
    pub(in crate::manager) fn owner_operation(
        &mut self,
        lease: DeviceLease,
        operation: OwnerOperation,
    ) -> Result<OwnerReply, OperationError> {
        match operation {
            OwnerOperation::Resume { cursor, limit } => Ok(OwnerReply::Replay(Box::new(
                self.resume(lease, cursor, limit)?,
            ))),
            OwnerOperation::SubmitTask { input } => {
                Ok(OwnerReply::Task(Box::new(self.submit_task(lease, input)?)))
            }
            OwnerOperation::Task { id } => Ok(OwnerReply::Task(Box::new(self.task(lease, id)?))),
            OwnerOperation::CancelTask { id } => {
                Ok(OwnerReply::Task(Box::new(self.cancel_task(lease, id)?)))
            }
            OwnerOperation::GrantApproval { id, expires_at } => Ok(OwnerReply::Task(Box::new(
                self.grant_approval(lease, id, expires_at)?,
            ))),
            OwnerOperation::RevokeApproval { id } => {
                Ok(OwnerReply::Task(Box::new(self.revoke_approval(lease, id)?)))
            }
            OwnerOperation::SetPermission {
                bot,
                expected_revision,
                allow,
            } => Ok(OwnerReply::Permission(self.set_permission(
                lease,
                bot,
                expected_revision,
                allow,
            )?)),
            OwnerOperation::Conversation { input } => {
                Ok(OwnerReply::Conversation(self.conversation(lease, input)?))
            }
            OwnerOperation::AppendMessage { input } => {
                Ok(OwnerReply::Message(self.append_message(lease, input)?))
            }
            OwnerOperation::WriteDocument { input } => {
                Ok(OwnerReply::Document(self.write_document(lease, input)?))
            }
            OwnerOperation::SearchDocuments { bot, query } => Ok(OwnerReply::Documents(
                self.search_documents(lease, bot, query)?,
            )),
            OwnerOperation::RebuildDocuments => {
                Ok(OwnerReply::Documents(self.rebuild_documents(lease)?))
            }
            OwnerOperation::ConfigureInference { input } => Ok(OwnerReply::Configuration(
                self.configure_inference(lease, input)?,
            )),
            OwnerOperation::InferenceConfiguration { bot } => Ok(OwnerReply::Configuration(
                self.inference_configuration(lease, bot)?,
            )),
            OwnerOperation::SubmitInference { input } => Ok(OwnerReply::Inference(Box::new(
                self.submit_inference(lease, input)?,
            ))),
            OwnerOperation::Inference { id } => {
                Ok(OwnerReply::Inference(Box::new(self.inference(lease, id)?)))
            }
            OwnerOperation::CancelInference { id } => Ok(OwnerReply::Inference(Box::new(
                self.cancel_inference(lease, id)?,
            ))),
            OwnerOperation::SubmitRun { input } => {
                Ok(OwnerReply::Run(Box::new(self.submit_run(lease, *input)?)))
            }
            OwnerOperation::Run { id } => Ok(OwnerReply::Run(Box::new(self.run(lease, id)?))),
            OwnerOperation::CancelRun { id } => {
                Ok(OwnerReply::Run(Box::new(self.cancel_run(lease, id)?)))
            }
            OwnerOperation::ComputerSession { id } => Ok(OwnerReply::ComputerSession(
                self.authorized_computer_session(lease, id)?,
            )),
            OwnerOperation::OpenComputerSession { bot, worker } => Ok(OwnerReply::ComputerSession(
                self.open_computer_session(lease, bot, worker)?,
            )),
            OwnerOperation::SetInputOwner {
                id,
                expected,
                owner,
            } => Ok(OwnerReply::ComputerSession(
                self.set_input_owner(lease, id, expected, owner)?,
            )),
            OwnerOperation::Input { id, fence, value } => Ok(OwnerReply::Receipt(self.dispatch(
                lease,
                ManagerMessage::Input {
                    command_id: id,
                    fence,
                    value,
                },
            )?)),
            OwnerOperation::Receipt { worker, id } => {
                Ok(OwnerReply::Receipt(self.receipt(lease, worker, id)?))
            }
            OwnerOperation::CodingCalls { task } => {
                Ok(OwnerReply::CodingCalls(self.coding_calls(lease, task)?))
            }
            OwnerOperation::Artifact { .. } => Err(ErrorCode::UnsupportedExecution.into()),
            OwnerOperation::VerifyInference { .. } => Err(OperationError::InvalidInput),
        }
    }
}
