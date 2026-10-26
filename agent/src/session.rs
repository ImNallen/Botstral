use botstral_link::*;
use std::collections::HashMap;

pub(crate) struct Sessions {
    pub generation: Generation,
    worker: WorkerId,
    sessions: HashMap<ComputerSessionId, ComputerSession>,
}
impl Sessions {
    pub(crate) fn new(
        generation: Generation,
        sessions: Vec<ComputerSession>,
        worker: WorkerId,
    ) -> Result<Self, ErrorCode> {
        if sessions.len() > MAX_SESSIONS || sessions.iter().any(|s| s.worker_id != worker) {
            return Err(ErrorCode::InvalidMessage);
        }
        let count = sessions.len();
        let sessions: HashMap<_, _> = sessions.into_iter().map(|s| (s.id, s)).collect();
        if sessions.len() != count {
            return Err(ErrorCode::InvalidMessage);
        }
        Ok(Self {
            generation,
            worker,
            sessions,
        })
    }
    pub(crate) fn control(
        &mut self,
        generation: Generation,
        session: ComputerSession,
    ) -> Result<InputFence, ErrorCode> {
        if session.worker_id != self.worker {
            return Err(ErrorCode::Unauthorized);
        }
        if generation != self.generation {
            return Err(ErrorCode::StaleGeneration);
        }
        if let Some(old) = self.sessions.get(&session.id) {
            if old.bot_id != session.bot_id || old.worker_id != session.worker_id {
                return Err(ErrorCode::Conflict);
            }
            if session.revision < old.revision
                || (session.revision == old.revision && session.owner != old.owner)
            {
                return Err(ErrorCode::StaleRevision);
            }
        } else if self.sessions.len() >= MAX_SESSIONS {
            return Err(ErrorCode::QueueFull);
        }
        let fence = session.fence(generation);
        self.sessions.insert(session.id, session);
        Ok(fence)
    }
    pub(crate) fn matches(&self, body: &CommandBody) -> bool {
        body.worker == self.worker
            && self
                .sessions
                .get(&body.session)
                .is_some_and(|s| s.bot_id == body.bot)
    }
    pub(crate) fn check(&self, fence: InputFence) -> Result<(), ErrorCode> {
        if fence.generation != self.generation {
            return Err(ErrorCode::StaleGeneration);
        }
        let session = self
            .sessions
            .get(&fence.session_id)
            .ok_or(ErrorCode::StaleRevision)?;
        if session.revision != fence.revision {
            return Err(ErrorCode::StaleRevision);
        }
        if session.owner != fence.owner {
            return Err(ErrorCode::WrongInputOwner);
        }
        Ok(())
    }
}

pub(crate) struct Runtime {
    pub(crate) sessions: Sessions,
    seen: HashMap<CommandId, (ManagerMessage, Vec<Envelope<WorkerMessage>>)>,
    #[cfg(feature = "nonproduction-test-worker")]
    executor: Option<crate::test_worker::Executor>,
}
impl Runtime {
    pub(crate) fn new(
        generation: Generation,
        sessions: Vec<ComputerSession>,
        worker: WorkerId,
        test_worker: bool,
    ) -> Result<Self, ErrorCode> {
        if test_worker && !cfg!(feature = "nonproduction-test-worker") {
            return Err(ErrorCode::UnsupportedExecution);
        }
        Ok(Self {
            sessions: Sessions::new(generation, sessions, worker)?,
            seen: HashMap::new(),
            #[cfg(feature = "nonproduction-test-worker")]
            executor: test_worker.then(crate::test_worker::Executor::new),
        })
    }
    pub(crate) fn handle(
        &mut self,
        envelope: Envelope<ManagerMessage>,
    ) -> Result<Vec<Envelope<WorkerMessage>>, ErrorCode> {
        let request_id = envelope.request_id;
        let message = envelope.message;
        if let ManagerMessage::Control {
            generation,
            session,
        } = message
        {
            let fence = self.sessions.control(generation, session)?;
            let confirmation = Envelope {
                version: VERSION,
                request_id,
                message: WorkerMessage::ControlApplied { fence },
            };
            #[cfg(feature = "nonproduction-test-worker")]
            if let Some(executor) = &mut self.executor {
                let mut reports = executor.control(fence);
                reports.push(confirmation);
                return Ok(reports);
            }
            return Ok(vec![confirmation]);
        }
        match &message {
            ManagerMessage::Pong => return Ok(vec![]),
            ManagerMessage::Receipt { .. } => return Ok(vec![]),
            _ => {}
        }
        let (id, fence) = match &message {
            ManagerMessage::Command(command) => (command.id, command.fence),
            ManagerMessage::Input {
                command_id, fence, ..
            } => (*command_id, *fence),
            _ => return Err(ErrorCode::WrongPhase),
        };
        let reject = |code| {
            vec![Envelope {
                version: VERSION,
                request_id,
                message: WorkerMessage::Acknowledgment {
                    generation: self.sessions.generation,
                    command_id: id,
                    outcome: Acknowledgment::Rejected { code },
                },
            }]
        };
        if let Err(code) = self.sessions.check(fence) {
            return Ok(reject(code));
        }
        if matches!(message, ManagerMessage::Command(_)) {
            return Ok(reject(ErrorCode::UnsupportedExecution));
        }
        if let Some((prior, reports)) = self.seen.get(&id) {
            return Ok(if prior == &message {
                reports.clone()
            } else {
                reject(ErrorCode::Conflict)
            });
        }
        if self.seen.len() >= MAX_PENDING {
            return Ok(reject(ErrorCode::QueueFull));
        }
        #[cfg(feature = "nonproduction-test-worker")]
        let reports = if let Some(executor) = &mut self.executor {
            executor.handle(&self.sessions, request_id, &message)
        } else {
            reject(ErrorCode::UnsupportedExecution)
        };
        #[cfg(not(feature = "nonproduction-test-worker"))]
        let reports = reject(ErrorCode::UnsupportedExecution);
        self.seen.insert(id, (message, reports.clone()));
        Ok(reports)
    }
    pub(crate) fn tick(&mut self) -> Vec<Envelope<WorkerMessage>> {
        #[cfg(feature = "nonproduction-test-worker")]
        if let Some(executor) = &mut self.executor {
            return executor.tick(&self.sessions);
        }
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(worker: WorkerId) -> ComputerSession {
        ComputerSession {
            id: ComputerSessionId::new(),
            bot_id: BotId::new(),
            worker_id: worker,
            owner: InputOwner::Bot,
            revision: Revision::INITIAL,
            status: ControlStatus::Confirmed,
        }
    }
    #[test]
    fn receiver_fences_generations_revisions_and_cross_worker_control() {
        let worker = WorkerId::new();
        let a = session(worker);
        let b = session(worker);
        let generation = Generation::INITIAL;
        let mut sessions = Sessions::new(generation, vec![a.clone(), b.clone()], worker).unwrap();
        let mut changed = a.clone();
        changed.owner = InputOwner::Owner;
        changed.revision = a.revision.advance().unwrap();
        sessions.control(generation, changed.clone()).unwrap();
        assert_eq!(
            sessions.check(a.fence(generation)),
            Err(ErrorCode::StaleRevision)
        );
        assert_eq!(sessions.check(b.fence(generation)), Ok(()));
        assert_eq!(
            sessions.check(changed.fence(generation.advance().unwrap())),
            Err(ErrorCode::StaleGeneration)
        );
        changed.worker_id = WorkerId::new();
        assert_eq!(
            sessions.control(generation, changed),
            Err(ErrorCode::Unauthorized)
        );
    }
}
