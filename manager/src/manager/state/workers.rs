use super::*;
use botstral_link::{
    CertificateFingerprint, ComputerSession, ComputerSessionId, ControlStatus, Envelope, ErrorCode,
    Generation, Hello, InputOwner, MAX_SESSIONS, ManagerMessage, RequestId, Revision, WorkerId,
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
    generation: Generation,
    phase: WorkerPhase,
    confirmed: std::collections::HashMap<ComputerSessionId, Revision>,
    snapshot: std::collections::HashMap<ComputerSessionId, Revision>,
    outbound: mpsc::Sender<Envelope<ManagerMessage>>,
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
            "SELECT fingerprint, expires_at, consumed_at FROM worker_invitations WHERE token_hash = ?1", [hash.as_slice()],
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
                phase: WorkerPhase::Synchronizing,
                confirmed: Default::default(),
                snapshot: Default::default(),
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

    fn deliver(
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

    fn check_lease(&self, lease: ConnectionLease) -> Result<&WorkerConnection, OperationError> {
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
            WorkerMessage::SnapshotApplied { generation }
                if generation == lease.generation
                    && connection.phase == WorkerPhase::Synchronizing =>
            {
                let connection = self.worker_connections.get_mut(&lease.worker).unwrap();
                connection.phase = WorkerPhase::Ready;
                connection.confirmed = std::mem::take(&mut connection.snapshot);
            }
            WorkerMessage::ControlApplied { fence } if connection.phase == WorkerPhase::Ready => {
                if fence.generation != lease.generation {
                    return Err(ErrorCode::StaleGeneration.into());
                }
                let session = self.find_computer_session(fence.session_id)?;
                if session.worker_id != lease.worker || session.fence(lease.generation) != fence {
                    return Err(ErrorCode::StaleRevision.into());
                }
                self.worker_connections
                    .get_mut(&lease.worker)
                    .unwrap()
                    .confirmed
                    .insert(session.id, session.revision);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker_http::VerifiedIdentity;

    fn fixture() -> (tempfile::TempDir, State) {
        let temp = tempfile::tempdir().unwrap();
        let mut db = Connection::open_in_memory().unwrap();
        migrate(&mut db).unwrap();
        db.execute_batch("INSERT INTO owner VALUES(1, 'Worker test owner', 'unused'); INSERT INTO vault VALUES(1, 1, X'00');").unwrap();
        let (configured, _) = watch::channel(false);
        let mut state = State::from_connection(db, temp.path().into(), configured).unwrap();
        state.test_time = Some(1000);
        state
            .connection
            .execute(
                "INSERT INTO sessions VALUES(?1, ?2, 1, 999, 2000)",
                params![
                    Uuid::new_v4().to_string(),
                    crypto::token_hash(&"o".repeat(43)).as_slice()
                ],
            )
            .unwrap();
        (temp, state)
    }
    fn owner() -> SessionToken {
        SessionToken::new("o".repeat(43)).unwrap()
    }
    fn peer() -> VerifiedIdentity {
        VerifiedIdentity::for_test(b"certificate A")
    }
    fn enroll(state: &mut State) -> WorkerId {
        let grant = state
            .invite_worker(owner(), peer().fingerprint().clone(), 600)
            .unwrap();
        state.enroll_worker(peer(), grant.secret).unwrap()
    }

    #[test]
    fn invitation_binding_expiry_atomicity_and_replay() {
        let (_temp, mut state) = fixture();
        let grant = state
            .invite_worker(owner(), peer().fingerprint().clone(), 1)
            .unwrap();
        let secret = || SecretString::from(grant.secret.expose_secret());
        assert_eq!(
            state.enroll_worker(VerifiedIdentity::for_test(b"certificate B"), secret()),
            Err(ErrorCode::IdentityMismatch.into())
        );
        state.test_time = Some(1001);
        assert_eq!(
            state.enroll_worker(peer(), secret()),
            Err(ErrorCode::InvitationExpired.into())
        );
        state.test_time = Some(1000);
        state.connection.execute_batch("CREATE TRIGGER reject_worker BEFORE INSERT ON workers BEGIN SELECT RAISE(ABORT, 'test'); END;").unwrap();
        assert!(state.enroll_worker(peer(), secret()).is_err());
        state
            .connection
            .execute_batch("DROP TRIGGER reject_worker")
            .unwrap();
        let id = state.enroll_worker(peer(), secret()).unwrap();
        assert_eq!(
            state.enroll_worker(peer(), secret()),
            Err(ErrorCode::InvitationUsed.into())
        );
        assert_eq!(state.workers(owner()).unwrap()[0].id, id);
    }

    #[test]
    fn snapshots_generations_late_disconnect_and_full_channels() {
        let (_temp, mut state) = fixture();
        let worker = enroll(&mut state);
        let bot = BotId::new();
        state
            .create_bot(
                owner(),
                NewBot {
                    id: bot,
                    name: BotName::new("A".into()).unwrap(),
                },
            )
            .unwrap();
        let session = state.open_computer_session(owner(), bot, worker).unwrap();
        let connect = |state: &mut State| {
            let (out, receive) = mpsc::channel(1);
            let (stop, stopped) = watch::channel(false);
            let lease = state
                .connect_worker(peer(), Hello::current(false), out, stop, RequestId::new())
                .unwrap();
            (lease, receive, stopped)
        };
        let (old, mut receive, mut stopped) = connect(&mut state);
        receive.try_recv().unwrap();
        assert_eq!(
            state.find_computer_session(session.id).unwrap().status,
            ControlStatus::Pending
        );
        state
            .worker_report(
                old,
                Envelope::new(WorkerMessage::SnapshotApplied {
                    generation: old.generation,
                }),
            )
            .unwrap();
        assert_eq!(
            state.find_computer_session(session.id).unwrap().status,
            ControlStatus::Confirmed
        );
        let (new, mut receive, _) = connect(&mut state);
        assert!(*stopped.borrow_and_update());
        assert!(new.generation > old.generation);
        state.disconnect_worker(old);
        assert!(state.check_lease(new).is_ok());
        assert!(matches!(
            state.worker_report(
                old,
                Envelope::new(WorkerMessage::Heartbeat {
                    generation: old.generation
                })
            ),
            Err(OperationError::Protocol(ErrorCode::StaleGeneration))
        ));
        receive.try_recv().unwrap();
        state
            .worker_report(
                new,
                Envelope::new(WorkerMessage::SnapshotApplied {
                    generation: new.generation,
                }),
            )
            .unwrap();
        assert!(state.find_computer_session(session.id).unwrap().revision > session.revision);
        state
            .deliver(worker, Envelope::new(ManagerMessage::Pong))
            .unwrap();
        assert_eq!(
            state.deliver(worker, Envelope::new(ManagerMessage::Pong)),
            Err(ErrorCode::QueueFull.into())
        );
        assert_eq!(
            state.find_computer_session(session.id).unwrap().status,
            ControlStatus::Disconnected
        );
        state.test_time = Some(2000);
        assert_eq!(
            state.revoke_worker(owner(), worker),
            Err(OperationError::Unauthorized)
        );
    }
}
