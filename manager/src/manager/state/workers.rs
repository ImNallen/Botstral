mod dispatch;

use super::*;
use botstral_link::{
    Acknowledgment, Capability, CertificateFingerprint, CommandAction, CommandId, ComputerSession,
    ComputerSessionId, ControlStatus, Envelope, ErrorCode, Generation, Hello, InputFence,
    InputOwner, MAX_PENDING, MAX_SESSIONS, ManagerMessage, RequestId, Revision, WorkerId,
    WorkerMessage,
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
        self.connection.execute("INSERT INTO worker_invitations(token_hash, fingerprint, expires_at, manager) VALUES(?1, ?2, ?3, ?4)",
            params![crypto::token_hash(secret.expose_secret()).as_slice(), fingerprint.as_str(), expires_at, self.manager_identity()?.to_string()])?;
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
                phase: self
                    .worker_connections
                    .get(&id.parse().map_err(|_| OperationError::Internal)?)
                    .map_or(WorkerPhase::Disconnected, |c| c.phase),
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
        if let Some(connection) = self.worker_connections.remove(&id) {
            connection.stop.send_replace(true);
        }
        Ok(())
    }

    pub(in crate::manager) fn open_computer_session(
        &mut self,
        token: impl Into<OwnerAuthority>,
        bot: BotId,
        worker: WorkerId,
    ) -> Result<ComputerSession, OperationError> {
        self.authorize_owner(&token.into())?;
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
            if session.worker_id == worker {
                return Ok(session);
            }
            let revoked: bool = self.connection.query_row(
                "SELECT revoked FROM workers WHERE id = ?1",
                [session.worker_id.to_string()],
                |r| r.get(0),
            )?;
            if !revoked {
                return Err(OperationError::Conflict);
            }
            let count: i64 = self.connection.query_row(
                "SELECT count(*) FROM computer_sessions WHERE worker_id = ?1",
                [worker.to_string()],
                |r| r.get(0),
            )?;
            if count >= MAX_SESSIONS as i64 {
                return Err(OperationError::Busy);
            }
            self.connection.execute(
                "UPDATE computer_sessions SET worker_id = ?1, revision = ?2 WHERE id = ?3",
                params![
                    worker.to_string(),
                    session.revision.advance()?.get() as i64,
                    session.id.to_string()
                ],
            )?;
            let session = self.find_computer_session(session.id)?;
            if let Some(connection) = self.worker_connections.get(&worker) {
                self.deliver(
                    worker,
                    Envelope::new(ManagerMessage::Control {
                        generation: connection.generation,
                        session: session.clone(),
                    }),
                )?;
            }
            return self.find_computer_session(session.id);
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
        let session = self.find_computer_session(id)?;
        if let Some(connection) = self.worker_connections.get(&worker) {
            self.deliver(
                worker,
                Envelope::new(ManagerMessage::Control {
                    generation: connection.generation,
                    session,
                }),
            )?;
        }
        self.find_computer_session(id)
    }

    pub(in crate::manager) fn authorized_computer_session(
        &self,
        token: impl Into<OwnerAuthority>,
        id: ComputerSessionId,
    ) -> Result<ComputerSession, OperationError> {
        self.authorize_owner(&token.into())?;
        self.find_computer_session(id)
    }

    pub(super) fn find_computer_session(
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
        let worker_id = worker.parse().map_err(|_| OperationError::Internal)?;
        let revision = Revision::try_from(revision as u64).map_err(|_| OperationError::Internal)?;
        let status =
            self.worker_connections
                .get(&worker_id)
                .map_or(ControlStatus::Disconnected, |c| {
                    if c.phase == WorkerPhase::Ready && c.confirmed.get(&id) == Some(&revision) {
                        ControlStatus::Confirmed
                    } else {
                        ControlStatus::Pending
                    }
                });
        Ok(ComputerSession {
            id,
            bot_id: bot.parse().map_err(|_| OperationError::Internal)?,
            worker_id,
            owner: if owner == "bot" {
                InputOwner::Bot
            } else {
                InputOwner::Owner
            },
            revision,
            status,
        })
    }
}

pub(super) struct WorkerConnection {
    pub(super) generation: Generation,
    pub(super) recovered: bool,
    pub(super) recovery_high: u64,
    pub(super) inventory: Vec<CommandId>,
    pub(super) inventory_total: Option<u16>,
    pub(super) deliveries: std::collections::HashMap<CommandId, botstral_link::Delivery>,
    phase: WorkerPhase,
    confirmed: std::collections::HashMap<ComputerSessionId, Revision>,
    snapshot: std::collections::HashMap<ComputerSessionId, Revision>,
    commands: std::collections::HashMap<CommandId, PendingCommand>,
    pub(super) capabilities: Vec<Capability>,
    pub(super) outbound: mpsc::Sender<Envelope<ManagerMessage>>,
    stop: watch::Sender<bool>,
}

impl State {
    pub(in crate::manager) fn enroll_worker(
        &mut self,
        peer: crate::worker_http::VerifiedIdentity,
        secret: SecretString,
    ) -> Result<WorkerId, OperationError> {
        if secret.expose_secret().len() != 43 {
            return Err(ErrorCode::InvalidInvitation.into());
        }
        let now = self.now();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let hash = crypto::token_hash(secret.expose_secret());
        let (fingerprint, expires_at, consumed): (String, i64, Option<i64>) = tx.query_row(
            "SELECT fingerprint, expires_at, consumed_at FROM worker_invitations WHERE manager = (SELECT identity FROM manager_identity WHERE id=1) AND role='worker' AND token_hash = ?1", [hash.as_slice()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()?.ok_or(ErrorCode::InvalidInvitation)?;
        if peer.fingerprint().as_str() != fingerprint {
            return Err(ErrorCode::IdentityMismatch.into());
        }
        if expires_at <= now {
            return Err(ErrorCode::InvitationExpired.into());
        }
        if consumed.is_some() {
            return Err(ErrorCode::InvitationUsed.into());
        }
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM workers WHERE fingerprint = ?1)",
            [&fingerprint],
            |r| r.get(0),
        )?;
        if exists {
            return Err(ErrorCode::Conflict.into());
        }
        let id = WorkerId::new();
        tx.execute(
            "INSERT INTO workers(id, fingerprint) VALUES(?1, ?2)",
            params![id.to_string(), fingerprint],
        )?;
        tx.execute(
            "UPDATE worker_invitations SET consumed_at = ?1 WHERE token_hash = ?2",
            params![now, hash.as_slice()],
        )?;
        tx.commit()?;
        Ok(id)
    }

    fn worker_sessions(&self, worker: WorkerId) -> Result<Vec<ComputerSession>, OperationError> {
        let mut statement = self
            .connection
            .prepare("SELECT id FROM computer_sessions WHERE worker_id = ?1 ORDER BY id")?;
        let ids = statement.query_map([worker.to_string()], |r| r.get::<_, String>(0))?;
        ids.map(|id| self.find_computer_session(id?.parse().map_err(|_| OperationError::Internal)?))
            .collect()
    }

    pub(in crate::manager) fn connect_worker(
        &mut self,
        peer: crate::worker_http::VerifiedIdentity,
        hello: Hello,
        outbound: mpsc::Sender<Envelope<ManagerMessage>>,
        stop: watch::Sender<bool>,
        request_id: RequestId,
    ) -> Result<ConnectionLease, OperationError> {
        hello.negotiate(cfg!(feature = "nonproduction-test-worker"))?;
        let (id, generation): (String, i64) = self
            .connection
            .query_row(
                "SELECT id, generation FROM workers WHERE fingerprint = ?1 AND revoked = 0",
                [peer.fingerprint().as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or(ErrorCode::Unauthorized)?;
        let worker: WorkerId = id.parse().map_err(|_| OperationError::Internal)?;
        let generation = Generation::try_from(generation as u64 + 1)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE workers SET generation = ?1 WHERE id = ?2",
            params![generation.get() as i64, id],
        )?;
        tx.execute(
            "UPDATE computer_sessions SET revision = revision + 1 WHERE worker_id = ?1",
            [worker.to_string()],
        )?;
        tx.commit()?;
        if let Some(old) = self.worker_connections.remove(&worker) {
            old.stop.send_replace(true);
        }
        self.worker_connections.insert(
            worker,
            WorkerConnection {
                generation,
                recovered: false,
                recovery_high: 0,
                deliveries: Default::default(),
                inventory: Vec::new(),
                inventory_total: None,
                phase: WorkerPhase::Synchronizing,
                confirmed: Default::default(),
                snapshot: Default::default(),
                commands: Default::default(),
                capabilities: hello.capabilities,
                outbound,
                stop,
            },
        );
        let sessions = self.worker_sessions(worker)?;
        self.worker_connections.get_mut(&worker).unwrap().snapshot =
            sessions.iter().map(|s| (s.id, s.revision)).collect();
        self.deliver(
            worker,
            Envelope {
                version: botstral_link::VERSION,
                request_id,
                message: ManagerMessage::Welcome {
                    worker_id: worker,
                    generation,
                    sessions,
                },
            },
        )?;
        Ok(ConnectionLease { worker, generation })
    }

    pub(super) fn deliver(
        &mut self,
        worker: WorkerId,
        message: Envelope<ManagerMessage>,
    ) -> Result<(), OperationError> {
        let connection = self
            .worker_connections
            .get(&worker)
            .ok_or(ErrorCode::Disconnected)?;
        if connection.outbound.try_send(message).is_err() {
            let connection = self
                .worker_connections
                .remove(&worker)
                .expect("connection exists");
            connection.stop.send_replace(true);
            return Err(ErrorCode::QueueFull.into());
        }
        Ok(())
    }

    pub(super) fn check_lease(
        &self,
        lease: ConnectionLease,
    ) -> Result<&WorkerConnection, OperationError> {
        let connection = self
            .worker_connections
            .get(&lease.worker)
            .ok_or(ErrorCode::Disconnected)?;
        if connection.generation != lease.generation {
            return Err(ErrorCode::StaleGeneration.into());
        }
        Ok(connection)
    }

    pub(in crate::manager) fn worker_report(
        &mut self,
        lease: ConnectionLease,
        report: Envelope<WorkerMessage>,
    ) -> Result<(), OperationError> {
        let connection = self.check_lease(lease)?;
        match report.message {
            WorkerMessage::ArtifactFailed { offer }
                if connection.phase == WorkerPhase::Ready && connection.recovered =>
            {
                self.artifact_failed(lease, offer)?
            }
            WorkerMessage::ArtifactOffer { offer }
                if connection.phase == WorkerPhase::Ready && connection.recovered =>
            {
                self.artifact_offer(lease, offer)?
            }
            WorkerMessage::ArtifactChunk { chunk }
                if connection.phase == WorkerPhase::Ready && connection.recovered =>
            {
                self.artifact_chunk(lease, chunk)?
            }
            WorkerMessage::Inventory {
                generation,
                incarnation,
                offset,
                total,
                entries,
            } if generation == lease.generation
                && connection.phase == WorkerPhase::Ready
                && !connection.recovered =>
            {
                self.reconcile_inventory(lease, incarnation, offset, total, entries)?
            }
            WorkerMessage::RecoveryHello {
                generation,
                incarnation,
                high_water,
            } if generation == lease.generation && connection.phase == WorkerPhase::Ready => {
                self.recovery_hello(lease, incarnation, high_water)?
            }
            WorkerMessage::Reports {
                generation,
                incarnation,
                high_water,
                reports,
            } if generation == lease.generation && connection.phase == WorkerPhase::Ready => {
                self.durable_reports(lease, incarnation, high_water, reports)?
            }
            WorkerMessage::Claim {
                incarnation,
                fence,
                binding,
            } if connection.phase == WorkerPhase::Ready => {
                self.claim(lease, incarnation, fence, binding)?
            }
            WorkerMessage::SnapshotApplied { generation }
                if generation == lease.generation
                    && connection.phase == WorkerPhase::Synchronizing =>
            {
                let pending = self.process_control_status(lease.worker)?;
                let connection = self.worker_connections.get_mut(&lease.worker).unwrap();
                connection.phase = WorkerPhase::Ready;
                connection.confirmed = std::mem::take(&mut connection.snapshot)
                    .into_iter()
                    .filter(|(session, _)| !pending.contains_key(session))
                    .collect();
            }
            WorkerMessage::ControlApplied { fence } if connection.phase == WorkerPhase::Ready => {
                if fence.generation != lease.generation {
                    return Err(ErrorCode::StaleGeneration.into());
                }
                let session = self.find_computer_session(fence.session_id)?;
                if session.worker_id == lease.worker && fence.revision < session.revision {
                    return Ok(());
                }
                if session.worker_id != lease.worker || session.fence(lease.generation) != fence {
                    return Err(ErrorCode::StaleRevision.into());
                }
                if self
                    .process_control_status(lease.worker)?
                    .get(&fence.session_id)
                    == Some(&super::tasks::ProcessControlStatus::Unproved)
                {
                    return Ok(());
                }
                self.worker_connections
                    .get_mut(&lease.worker)
                    .unwrap()
                    .confirmed
                    .insert(session.id, session.revision);
                self.pump_tasks(lease.worker)?;
            }
            WorkerMessage::Acknowledgment {
                generation,
                command_id,
                outcome,
            } if connection.phase == WorkerPhase::Ready => {
                if generation != lease.generation {
                    return Err(ErrorCode::StaleGeneration.into());
                }
                let command = connection
                    .commands
                    .get(&command_id)
                    .ok_or(ErrorCode::InvalidMessage)?;
                let fence = dispatch_fence(&command.message).unwrap().1;
                let outcome = match self.check_input_fence(lease.worker, fence) {
                    Ok(()) => outcome,
                    Err(error) => Acknowledgment::Rejected {
                        code: crate::worker_http::protocol_error(error),
                    },
                };
                self.worker_connections
                    .get_mut(&lease.worker)
                    .unwrap()
                    .commands
                    .get_mut(&command_id)
                    .unwrap()
                    .receipt
                    .acknowledgment = outcome;
            }
            WorkerMessage::Event {
                command_id,
                task_id,
                fence,
                event,
            } if connection.phase == WorkerPhase::Ready => {
                let command = connection
                    .commands
                    .get(&command_id)
                    .ok_or(ErrorCode::InvalidMessage)?;
                let ManagerMessage::Command(issued) = &command.message else {
                    return Err(ErrorCode::InvalidMessage.into());
                };
                if issued.task_id != task_id || issued.fence != fence {
                    return Err(ErrorCode::InvalidMessage.into());
                }
                if self.check_input_fence(lease.worker, fence).is_ok() {
                    self.worker_connections
                        .get_mut(&lease.worker)
                        .unwrap()
                        .commands
                        .get_mut(&command_id)
                        .unwrap()
                        .receipt
                        .last_event = Some(event);
                }
            }
            WorkerMessage::BotInput {
                command_id,
                task_id,
                fence,
                value: _,
            } if connection.phase == WorkerPhase::Ready => {
                let command = connection
                    .commands
                    .get(&command_id)
                    .ok_or(ErrorCode::InvalidMessage)?;
                let ManagerMessage::Command(issued) = &command.message else {
                    return Err(ErrorCode::InvalidMessage.into());
                };
                if issued.task_id != task_id
                    || issued.fence != fence
                    || fence.owner != InputOwner::Bot
                    || !matches!(issued.action, CommandAction::Start { .. })
                {
                    return Err(ErrorCode::InvalidMessage.into());
                }
                let outcome = match self.check_input_fence(lease.worker, fence) {
                    Ok(()) => {
                        let receipt = &mut self
                            .worker_connections
                            .get_mut(&lease.worker)
                            .unwrap()
                            .commands
                            .get_mut(&command_id)
                            .unwrap()
                            .receipt;
                        receipt.bot_inputs = receipt
                            .bot_inputs
                            .checked_add(1)
                            .ok_or(ErrorCode::QueueFull)?;
                        Acknowledgment::Accepted
                    }
                    Err(error) => Acknowledgment::Rejected {
                        code: crate::worker_http::protocol_error(error),
                    },
                };
                self.deliver(
                    lease.worker,
                    Envelope {
                        version: botstral_link::VERSION,
                        request_id: report.request_id,
                        message: ManagerMessage::Receipt {
                            command_id,
                            outcome,
                        },
                    },
                )?;
            }
            WorkerMessage::Heartbeat { generation }
                if generation == lease.generation && connection.phase == WorkerPhase::Ready =>
            {
                self.deliver(
                    lease.worker,
                    Envelope {
                        version: botstral_link::VERSION,
                        request_id: report.request_id,
                        message: ManagerMessage::Pong,
                    },
                )?;
                self.pump_tasks(lease.worker)?;
            }
            _ => return Err(ErrorCode::WrongPhase.into()),
        }
        Ok(())
    }

    pub(in crate::manager) fn disconnect_worker(&mut self, lease: ConnectionLease) {
        if self
            .worker_connections
            .get(&lease.worker)
            .is_some_and(|c| c.generation == lease.generation)
        {
            self.worker_connections.remove(&lease.worker);
        }
    }
}

struct PendingCommand {
    message: ManagerMessage,
    receipt: DispatchReceipt,
}

fn dispatch_fence(message: &ManagerMessage) -> Option<(CommandId, InputFence)> {
    match message {
        ManagerMessage::Command(command) => Some((command.id, command.fence)),
        ManagerMessage::Input {
            command_id, fence, ..
        } => Some((*command_id, *fence)),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
