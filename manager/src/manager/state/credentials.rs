use super::{
    tasks::{event, json, parse},
    *,
};
use botstral_link::{
    protected::{LoginValues, ProtectedOwnerOperation, ProtectedOwnerRequest},
    *,
};
use zeroize::Zeroizing;

#[cfg(test)]
mod tests;

fn advance(revision: CredentialRevision) -> Result<CredentialRevision, OperationError> {
    revision.advance().map_err(|_| OperationError::Conflict)
}

pub(super) fn canonical_origin(value: &str) -> Result<String, OperationError> {
    let url = reqwest::Url::parse(value).map_err(|_| OperationError::InvalidInput)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || url.host_str().is_some_and(|h| h.contains('*'))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(OperationError::InvalidInput);
    }
    let origin = url.origin().ascii_serialization();
    if value != origin {
        return Err(OperationError::InvalidInput);
    }
    Ok(origin)
}

impl State {
    fn credential_public_context(&mut self) -> Result<serde_json::Value, OperationError> {
        Ok(serde_json::json!({
            "scaffold":crate::manager::provider::credential_public_scaffold(),
            "configurations":self.inference_configurations()?,
            "memory":self.all_memory_entries()?,
            "memory_preface":runs::MEMORY_PREFACE,
            "screenshot_preface":runs::SCREENSHOT_PREFACE,
        }))
    }
    fn credential_identity(&self) -> Result<&age::x25519::Identity, OperationError> {
        match &self.installation {
            Installation::Configured {
                vault: VaultState::Unlocked { identity },
            } => Ok(identity),
            _ => Err(OperationError::Locked),
        }
    }
    fn credential_context(
        &self,
        id: CredentialId,
        epoch: CredentialEpoch,
    ) -> Result<String, OperationError> {
        Ok(format!(
            "{}\0{}\0{}\0login",
            self.manager_identity()?,
            id,
            epoch.get()
        ))
    }
    pub(super) fn load_login(&self, id: CredentialId) -> Result<LoginItemView, OperationError> {
        parse(self.connection.query_row(
            "SELECT view FROM credential_items WHERE id=?1",
            [id.to_string()],
            |r| r.get(0),
        )?)
    }
    pub(super) fn login_items(
        &self,
        authority: impl Into<OwnerAuthority>,
    ) -> Result<Vec<LoginItemView>, OperationError> {
        self.authorize_owner(&authority.into())?;
        let items = self
            .connection
            .prepare("SELECT view FROM credential_items ORDER BY id")?
            .query_map([], |r| r.get::<_, String>(0))?
            .map(|r| parse(r?))
            .collect::<Result<Vec<_>, _>>()?;
        self.reject_known_secrets(&items)?;
        Ok(items)
    }
    pub(super) fn accessible_login_items(
        &self,
        bot: BotId,
    ) -> Result<Vec<LoginItemView>, OperationError> {
        let items = self
            .connection
            .prepare("SELECT view FROM credential_items ORDER BY id")?
            .query_map([], |r| r.get::<_, String>(0))?
            .map(|r| parse::<LoginItemView>(r?))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|item| item.state == CredentialState::Active && item.metadata.bots.permits(bot))
            .collect::<Vec<_>>();
        self.reject_known_secrets(&items)?;
        Ok(items)
    }
    fn validate_login_metadata(&self, metadata: &LoginMetadata) -> Result<(), OperationError> {
        if metadata.label.trim().is_empty()
            || metadata.label.len() > 100
            || metadata.label.chars().any(char::is_control)
            || metadata.allowed_origins.is_empty()
            || metadata.allowed_origins.len() > 16
        {
            return Err(OperationError::InvalidInput);
        }
        let mut origins = std::collections::HashSet::new();
        for origin in &metadata.allowed_origins {
            if !origins.insert(canonical_origin(origin)?) {
                return Err(OperationError::InvalidInput);
            }
        }
        if let BotAccess::Selected { bots } = &metadata.bots {
            let mut seen = std::collections::HashSet::new();
            if bots.is_empty() || bots.len() > MAX_RECORDS {
                return Err(OperationError::InvalidInput);
            }
            for bot in bots {
                if !seen.insert(bot)
                    || !self.connection.query_row(
                        "SELECT EXISTS(SELECT 1 FROM bots WHERE id=?1)",
                        [bot.to_string()],
                        |r| r.get::<_, bool>(0),
                    )?
                {
                    return Err(OperationError::InvalidInput);
                }
            }
        }
        self.reject_known_secrets(metadata)
    }
    fn credential_capacity(&self, table: &str, maximum: usize) -> Result<(), OperationError> {
        let count: i64 =
            self.connection
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
        if count >= maximum as i64 {
            return Err(OperationError::Busy);
        }
        Ok(())
    }
    pub(super) fn credential_write_status(
        &mut self,
        authority: impl Into<OwnerAuthority>,
        request: RequestId,
        close: bool,
    ) -> Result<CredentialWriteStatus, OperationError> {
        self.authorize_owner(&authority.into())?;
        if let Some(status) = self
            .connection
            .query_row(
                "SELECT status FROM credential_writes WHERE request=?1",
                [request.to_string()],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            let status = parse(status)?;
            self.reject_known_secrets(&status)?;
            return Ok(status);
        }
        if !close {
            return Ok(CredentialWriteStatus::Unresolved { request });
        }
        self.credential_capacity("credential_writes", MAX_CREDENTIAL_REQUESTS)?;
        let status = CredentialWriteStatus::ClosedWithoutCommit { request };
        self.connection.execute(
            "INSERT INTO credential_writes VALUES(?1,?2)",
            params![request.to_string(), json(&status)?],
        )?;
        Ok(status)
    }
    pub(in crate::manager) fn protected_owner(
        &mut self,
        authority: impl Into<OwnerAuthority>,
        request: ProtectedOwnerRequest,
    ) -> Result<CredentialWriteStatus, OperationError> {
        let authority = authority.into();
        self.authorize_owner(&authority)?;
        match request.operation {
            ProtectedOwnerOperation::Unlock { passphrase } => {
                self.unlock_protected(&authority, VaultPassphrase::new(passphrase.to_string())?)?;
                Ok(CredentialWriteStatus::ClosedWithoutCommit {
                    request: request.id,
                })
            }
            ProtectedOwnerOperation::Write { target, values } => {
                self.write_login(authority, request.id, target, values)
            }
        }
    }
    pub(super) fn write_login(
        &mut self,
        authority: impl Into<OwnerAuthority>,
        request: RequestId,
        target: CredentialWriteTarget,
        values: LoginValues,
    ) -> Result<CredentialWriteStatus, OperationError> {
        let status = self.credential_write_status(authority, request, false)?;
        if !matches!(status, CredentialWriteStatus::Unresolved { .. }) {
            return Ok(status);
        }
        self.credential_identity()?;
        if !values.valid() {
            return Err(OperationError::InvalidInput);
        }
        if !values.totp_seed.is_empty() {
            super::credentials::totp::decode_seed(&values.totp_seed)?;
        }
        self.credential_capacity("credential_writes", MAX_CREDENTIAL_REQUESTS)?;
        self.credential_capacity("credential_epochs", MAX_CREDENTIAL_EPOCHS)?;
        let view = match target {
            CredentialWriteTarget::Create { metadata } => {
                self.credential_capacity("credential_items", MAX_CREDENTIAL_ITEMS)?;
                self.validate_login_metadata(&metadata)?;
                LoginItemView {
                    id: CredentialId::new(),
                    revision: CredentialRevision::INITIAL,
                    epoch: CredentialEpoch::INITIAL,
                    state: CredentialState::Active,
                    metadata,
                    has_totp: !values.totp_seed.is_empty(),
                }
            }
            CredentialWriteTarget::Rotate {
                id,
                expected_revision,
            } => {
                let mut view = self.load_login(id)?;
                if view.revision != expected_revision || view.state != CredentialState::Active {
                    return Err(OperationError::Conflict);
                }
                view.revision = advance(view.revision)?;
                view.epoch = view.epoch.advance().map_err(|_| OperationError::Conflict)?;
                view.has_totp = !values.totp_seed.is_empty();
                view
            }
        };
        let public =
            serde_json::json!({ "item": view, "context": self.credential_public_context()? });
        for secret in [&values.username, &values.password, &values.totp_seed] {
            if !secret.is_empty() && crate::manager::provider::reflects_secret(&public, secret) {
                let status = CredentialWriteStatus::Rejected {
                    request,
                    reason: CredentialWriteRejection::PublicValueCollision,
                };
                self.connection.execute(
                    "INSERT INTO credential_writes VALUES(?1,?2)",
                    params![request.to_string(), json(&status)?],
                )?;
                return Ok(status);
            }
        }
        let payload =
            Zeroizing::new(serde_json::to_vec(&values).map_err(|_| OperationError::Internal)?);
        let ciphertext = crypto::encrypt_credential(
            self.credential_identity()?,
            &self.credential_context(view.id, view.epoch)?,
            &payload,
        )?;
        let status = CredentialWriteStatus::Committed {
            request,
            item: view.clone(),
        };
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("INSERT INTO credential_items VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET view=excluded.view", params![view.id.to_string(), json(&view)?])?;
        tx.execute(
            "INSERT INTO credential_epochs VALUES(?1,?2,?3)",
            params![view.id.to_string(), view.epoch.get() as i64, ciphertext],
        )?;
        tx.execute(
            "INSERT INTO credential_writes VALUES(?1,?2)",
            params![request.to_string(), json(&status)?],
        )?;
        uses::invalidate(&tx, Some(view.id))?;
        event(&tx, "credential_changed", view.id)?;
        tx.commit()?;
        Ok(status)
    }
    pub(super) fn update_login(
        &mut self,
        authority: impl Into<OwnerAuthority>,
        id: CredentialId,
        expected_revision: CredentialRevision,
        metadata: Option<LoginMetadata>,
    ) -> Result<LoginItemView, OperationError> {
        self.authorize_owner(&authority.into())?;
        self.credential_identity()?;
        let mut view = self.load_login(id)?;
        if view.revision != expected_revision || view.state != CredentialState::Active {
            return Err(OperationError::Conflict);
        }
        match metadata {
            Some(metadata) => {
                self.validate_login_metadata(&metadata)?;
                view.metadata = metadata;
            }
            None => view.state = CredentialState::Removed,
        }
        view.revision = advance(view.revision)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE credential_items SET view=?2 WHERE id=?1",
            params![id.to_string(), json(&view)?],
        )?;
        uses::invalidate(&tx, Some(id))?;
        tx.execute(
            "DELETE FROM credential_rules WHERE item=?1",
            [id.to_string()],
        )?;
        event(&tx, "credential_changed", id)?;
        tx.commit()?;
        Ok(view)
    }
    fn login_values(
        &self,
        id: CredentialId,
        epoch: CredentialEpoch,
        ciphertext: &[u8],
    ) -> Result<LoginValues, OperationError> {
        let plaintext = crypto::decrypt_credential(
            self.credential_identity()?,
            &self.credential_context(id, epoch)?,
            ciphertext,
        )?;
        let values: LoginValues =
            serde_json::from_slice(&plaintext).map_err(|_| OperationError::Internal)?;
        if !values.valid() {
            return Err(OperationError::Internal);
        }
        Ok(values)
    }
    pub(super) fn known_login_secrets(&self) -> Result<Vec<SecretString>, OperationError> {
        let mut secrets = Vec::new();
        let mut statement = self
            .connection
            .prepare("SELECT item,epoch,ciphertext FROM credential_epochs")?;
        let rows = statement.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Vec<u8>>(2)?,
            ))
        })?;
        for row in rows {
            let (id, epoch, ciphertext) = row?;
            let values = self.login_values(
                id.parse().map_err(|_| OperationError::Internal)?,
                CredentialEpoch::try_from(
                    u64::try_from(epoch).map_err(|_| OperationError::Internal)?,
                )
                .map_err(|_| OperationError::Internal)?,
                &ciphertext,
            )?;
            secrets.extend(
                [&values.username, &values.password, &values.totp_seed]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .map(|s| SecretString::from(s.clone())),
            );
        }
        secrets.extend(self.known_issued_codes()?);
        Ok(secrets)
    }
}

mod totp;
pub(super) mod uses;
