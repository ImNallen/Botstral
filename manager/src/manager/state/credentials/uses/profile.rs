use super::*;
use std::collections::HashSet;

impl State {
    pub(super) fn profile_history(
        &self,
        worker: WorkerId,
        profile: ProfileEpoch,
    ) -> Result<Vec<CredentialUseReview>, OperationError> {
        let reviews =
            self.credential_public_rows::<CredentialUseReview>("SELECT view FROM credential_uses")?;
        let mut history = Vec::new();
        for review in reviews
            .into_iter()
            .filter(|r| r.destination.profile == profile)
        {
            let (assigned, body): (String, String) = self
                .connection
                .query_row(
                    "SELECT worker,view FROM tasks WHERE id=?1",
                    [review.binding.task.to_string()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
                .ok_or(OperationError::Conflict)?;
            let assigned: WorkerId = assigned.parse().map_err(|_| OperationError::Internal)?;
            let task: TaskView = parse(body)?;
            if assigned != worker || task.command.worker != assigned {
                return Err(OperationError::Unauthorized);
            }
            if task.id != review.binding.task
                || task.command.task != task.id
                || task.command.execution != review.binding.execution
                || task.command.bot != review.binding.bot
                || task.command.owner != review.binding.owner
                || task.command.operation != review.binding.operation
                || task.command.session != review.fence.session_id
                || task.approval.binding.as_ref() != Some(&review.binding)
            {
                return Err(OperationError::Conflict);
            }
            history.push(review);
        }
        Ok(history)
    }

    pub(super) fn screen_profile(
        &self,
        worker: WorkerId,
        protection: ProfileProtection,
        text: Zeroizing<String>,
    ) -> Result<ProtectedBrowserReply, OperationError> {
        let ProfileProtection::Tracked {
            profile,
            releases,
            complete: true,
        } = protection
        else {
            return Err(OperationError::Conflict);
        };
        if releases.len() > MAX_CREDENTIAL_USES || text.len() > 32768 {
            return Err(OperationError::InvalidInput);
        }
        self.credential_identity()?;
        let history = self.profile_history(worker, profile)?;
        if history.is_empty() {
            return Err(OperationError::Conflict);
        }
        let supplied: HashSet<_> = releases.iter().copied().collect();
        if supplied.len() != releases.len() {
            return Err(OperationError::Conflict);
        }
        let emitted: Vec<_> = history
            .into_iter()
            .filter(|r| match r.state {
                UseState::Released | UseState::Applied | UseState::Uncertain => true,
                UseState::AwaitingApproval
                | UseState::Denied
                | UseState::Authorized
                | UseState::Refused
                | UseState::Revoked
                | UseState::Expired => false,
            })
            .collect();
        let recorded: HashSet<_> = emitted.iter().map(|r| r.release).collect();
        if supplied != recorded {
            return Err(OperationError::Conflict);
        }
        let mut secrets = Vec::new();
        for review in emitted {
            let cipher: Vec<u8> = self.connection.query_row(
                "SELECT ciphertext FROM credential_epochs WHERE item=?1 AND epoch=?2",
                params![review.item.id.to_string(), review.item.epoch.get() as i64],
                |r| r.get(0),
            )?;
            let values = self.login_values(review.item.id, review.item.epoch, &cipher)?;
            match review.purpose {
                CredentialFields::Password => {
                    secrets.push(SecretString::from(values.username.clone()));
                    secrets.push(SecretString::from(values.password.clone()));
                }
                CredentialFields::Totp => {
                    let cipher: Vec<u8> = self
                        .connection
                        .query_row(
                            "SELECT ciphertext FROM credential_codes WHERE release=?1",
                            [review.release.to_string()],
                            |r| r.get(0),
                        )
                        .optional()?
                        .ok_or(OperationError::Conflict)?;
                    let code = crypto::decrypt_credential(
                        self.credential_identity()?,
                        &self.code_context(&review)?,
                        &cipher,
                    )?;
                    secrets.push(SecretString::from(
                        std::str::from_utf8(&code)
                            .map_err(|_| OperationError::Internal)?
                            .to_owned(),
                    ));
                }
            }
        }
        let value = serde_json::to_value(&*text).map_err(|_| OperationError::Internal)?;
        if secrets
            .iter()
            .any(|secret| crate::manager::provider::reflects_secret(&value, secret.expose_secret()))
        {
            return Err(OperationError::InvalidInput);
        }
        Ok(ProtectedBrowserReply::Screened { text })
    }
}
