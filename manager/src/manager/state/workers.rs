use super::*;
use botstral_link::{
    CertificateFingerprint, ComputerSession, ComputerSessionId, ControlStatus, InputOwner,
    MAX_SESSIONS, Revision, WorkerId,
};

impl State {
    pub(in crate::manager) fn invite_worker(
        &mut self,
        token: SessionToken,
        fingerprint: CertificateFingerprint,
        lifetime_seconds: u32,
    ) -> Result<InvitationGrant, OperationError> {
        self.authorize(&token)?;
        if !(1..=600).contains(&lifetime_seconds) {
            return Err(OperationError::InvalidInput);
        }
        let now = self.now();
        self.connection.execute(
            "DELETE FROM worker_invitations WHERE expires_at <= ?1",
            [now],
        )?;
        let count: i64 =
            self.connection
                .query_row("SELECT count(*) FROM worker_invitations", [], |r| r.get(0))?;
        if count >= 128 {
            return Err(OperationError::Busy);
        }
        let secret = SecretString::from(crypto::random_token());
        let expires_at = now + i64::from(lifetime_seconds);
        self.connection.execute("INSERT INTO worker_invitations(token_hash, fingerprint, expires_at) VALUES(?1, ?2, ?3)",
            params![crypto::token_hash(secret.expose_secret()).as_slice(), fingerprint.as_str(), expires_at])?;
        Ok(InvitationGrant { secret, expires_at })
    }

    pub(in crate::manager) fn workers(
        &mut self,
        token: SessionToken,
    ) -> Result<Vec<WorkerSummary>, OperationError> {
        self.authorize(&token)?;
        let mut statement = self
            .connection
            .prepare("SELECT id, fingerprint, revoked, generation FROM workers ORDER BY id")?;
        let rows = statement.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?;
        rows.map(|r| {
            let (id, fingerprint, revoked, generation) = r?;
            Ok(WorkerSummary {
                id: id.parse().map_err(|_| OperationError::Internal)?,
                fingerprint: fingerprint
                    .try_into()
                    .map_err(|_| OperationError::Internal)?,
                revoked,
                generation: generation as u64,
            })
        })
        .collect()
    }

    pub(in crate::manager) fn revoke_worker(
        &mut self,
        token: SessionToken,
        id: WorkerId,
    ) -> Result<(), OperationError> {
        self.authorize(&token)?;
        if self.connection.execute(
            "UPDATE workers SET revoked = 1 WHERE id = ?1",
            [id.to_string()],
        )? == 0
        {
            return Err(OperationError::NotFound);
        }
        Ok(())
    }

    pub(in crate::manager) fn open_computer_session(
        &mut self,
        token: SessionToken,
        bot: BotId,
        worker: WorkerId,
    ) -> Result<ComputerSession, OperationError> {
        self.authorize(&token)?;
        if self.find_bot(bot)?.is_none() {
            return Err(OperationError::NotFound);
        }
        let enrolled: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM workers WHERE id = ?1 AND revoked = 0)",
            [worker.to_string()],
            |r| r.get(0),
        )?;
        if !enrolled {
            return Err(OperationError::NotFound);
        }
        let existing: Option<String> = self
            .connection
            .query_row(
                "SELECT id FROM computer_sessions WHERE bot_id = ?1",
                [bot.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            let session =
                self.find_computer_session(id.parse().map_err(|_| OperationError::Internal)?)?;
            return if session.worker_id == worker {
                Ok(session)
            } else {
                Err(OperationError::Conflict)
            };
        }
        let count: i64 = self.connection.query_row(
            "SELECT count(*) FROM computer_sessions WHERE worker_id = ?1",
            [worker.to_string()],
            |r| r.get(0),
        )?;
        if count >= MAX_SESSIONS as i64 {
            return Err(OperationError::Busy);
        }
        let id = ComputerSessionId::new();
        self.connection.execute("INSERT INTO computer_sessions(id, bot_id, worker_id, owner, revision) VALUES(?1, ?2, ?3, 'bot', 1)", params![id.to_string(), bot.to_string(), worker.to_string()])?;
        self.find_computer_session(id)
    }

    pub(in crate::manager) fn authorized_computer_session(
        &self,
        token: SessionToken,
        id: ComputerSessionId,
    ) -> Result<ComputerSession, OperationError> {
        self.authorize(&token)?;
        self.find_computer_session(id)
    }

    fn find_computer_session(
        &self,
        id: ComputerSessionId,
    ) -> Result<ComputerSession, OperationError> {
        let (bot, worker, owner, revision): (String, String, String, i64) = self
            .connection
            .query_row(
                "SELECT bot_id, worker_id, owner, revision FROM computer_sessions WHERE id = ?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?
            .ok_or(OperationError::NotFound)?;
        Ok(ComputerSession {
            id,
            bot_id: bot.parse().map_err(|_| OperationError::Internal)?,
            worker_id: worker.parse().map_err(|_| OperationError::Internal)?,
            owner: if owner == "bot" {
                InputOwner::Bot
            } else {
                InputOwner::Owner
            },
            revision: Revision::try_from(revision as u64).map_err(|_| OperationError::Internal)?,
            status: ControlStatus::Disconnected,
        })
    }
}
