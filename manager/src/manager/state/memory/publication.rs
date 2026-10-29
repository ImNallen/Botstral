use super::{
    entries::{MemoryFile, digest, entry_filename},
    *,
};

pub(in crate::manager::state) struct PreparedMemoryPublication {
    pub(super) entry: MemoryId,
    pub(super) expected_digest: Option<String>,
    pub(super) intended_bytes: Vec<u8>,
}

impl PreparedMemoryPublication {
    pub(in crate::manager::state) fn commit(
        &self,
        db: &Connection,
        run: RunId,
        turn: &RunTurn,
        receipt: &ManagerToolResult,
    ) -> Result<(), OperationError> {
        db.execute(
            "INSERT INTO memory_publications VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'pending')",
            params![
                turn.id.to_string(),
                run.to_string(),
                turn.attempt.to_string(),
                receipt.call.id,
                self.entry.to_string(),
                self.expected_digest,
                self.intended_bytes,
                json(receipt)?
            ],
        )?;
        Ok(())
    }
}

impl State {
    pub(super) fn sync_memory_directory(&self) -> Result<(), OperationError> {
        fs::File::open(&self.directory)
            .and_then(|d| d.sync_all())
            .map_err(|_| OperationError::Internal)
    }

    pub(super) fn publish_memory_bytes(
        &self,
        id: MemoryId,
        expected: Option<&str>,
        bytes: &[u8],
    ) -> Result<(), OperationError> {
        let current = self.memory_file_bytes(id)?;
        let current_digest = current.as_deref().map(digest);
        if current_digest.as_deref() == Some(digest(bytes).as_str()) {
            return self.sync_memory_directory();
        }
        if current_digest.as_deref() != expected {
            return Err(OperationError::Conflict);
        }
        let name = entry_filename(id);
        let temporary = self.directory.join(format!(".{name}.tmp"));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temporary)
            .map_err(|_| OperationError::Internal)?;
        botstral_link::tls::check_private_file(&temporary).map_err(|_| OperationError::Internal)?;
        file.set_len(0).map_err(|_| OperationError::Internal)?;
        file.write_all(bytes)
            .map_err(|_| OperationError::Internal)?;
        file.sync_all().map_err(|_| OperationError::Internal)?;
        self.fault("memory_before_rename");
        if self
            .memory_file_bytes(id)?
            .as_deref()
            .map(digest)
            .as_deref()
            != expected
        {
            return Err(OperationError::Conflict);
        }
        fs::rename(&temporary, self.directory.join(name)).map_err(|_| OperationError::Internal)?;
        self.fault("memory_after_rename");
        self.sync_memory_directory()?;
        Ok(())
    }

    pub(in crate::manager::state) fn recover_memory_publications(
        &mut self,
    ) -> Result<(), OperationError> {
        let rows = self.connection.prepare("SELECT turn,run,attempt,call,entry,expected_digest,intended_bytes,receipt,state FROM memory_publications WHERE state!='applied' ORDER BY rowid")?
            .query_map([], |r| Ok((r.get::<_, String>(0)?,r.get::<_, String>(1)?,r.get::<_, String>(2)?,r.get::<_, String>(3)?,r.get::<_, String>(4)?,r.get::<_, Option<String>>(5)?,r.get::<_, Vec<u8>>(6)?,r.get::<_, String>(7)?,r.get::<_, String>(8)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        for (turn_id, run, attempt, call, entry, expected, bytes, receipt, state) in rows {
            let run = run.parse().map_err(|_| OperationError::Internal)?;
            let mut view = self.load_run(run)?;
            let turn = view
                .turns
                .iter_mut()
                .find(|t| t.id.to_string() == turn_id && t.attempt.to_string() == attempt)
                .ok_or(OperationError::Internal)?;
            let receipt: ManagerToolResult = parse(receipt)?;
            let file = MemoryFile::decode(&bytes)?;
            if file.id().to_string() != entry || receipt.call.id != call || turn.phase != RunTurnPhase::Responded
                || turn.response.as_ref().is_none_or(|response| !matches!(response, RunMessage::Assistant { tool_call: Some(c), .. } if c == &receipt.call)) {
                return Err(OperationError::Internal);
            }
            let published = self
                .reject_known_secrets(&file)
                .and_then(|()| self.reject_known_secrets(&receipt))
                .and_then(|()| self.publish_memory_bytes(file.id(), expected.as_deref(), &bytes));
            if matches!(published, Err(OperationError::Locked)) {
                return Err(OperationError::Locked);
            }
            if published.is_err() {
                if state == "attention" {
                    return Err(OperationError::Conflict);
                }
                view.summary.phase = RunPhase::AwaitingAttention {
                    reason: RunAttentionReason::MemoryPublication,
                };
                view.summary.updated_at = self.now();
                let tx = self
                    .connection
                    .transaction_with_behavior(TransactionBehavior::Immediate)?;
                tx.execute(
                    "UPDATE memory_publications SET state='attention' WHERE turn=?1",
                    [&turn_id],
                )?;
                runs::save_summary(&tx, &view.summary)?;
                tx.commit()?;
                return Err(OperationError::Conflict);
            }
            turn.manager_result = Some(receipt);
            view.summary.phase = if view.summary.cancellation_requested_at.is_some() {
                RunPhase::Cancelled
            } else if view.request.limits.deadline <= self.now() {
                RunPhase::Stopped {
                    reason: RunStopReason::Deadline,
                }
            } else {
                RunPhase::Ready
            };
            view.summary.updated_at = self.now();
            self.fault("memory_before_receipt");
            let tx = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute(
                "UPDATE memory_publications SET state='applied' WHERE turn=?1",
                [&turn_id],
            )?;
            event(&tx, "memory_published", &entry)?;
            runs::save_turn(&tx, turn)?;
            runs::save_summary(&tx, &view.summary)?;
            tx.commit()?;
            self.fault("memory_receipt_committed");
        }
        Ok(())
    }
}
