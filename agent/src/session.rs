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
    sessions: Sessions,
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
    pub(crate) fn has_executor(&self) -> bool {
        #[cfg(feature = "nonproduction-test-worker")]
        {
            self.executor.is_some()
        }
        #[cfg(not(feature = "nonproduction-test-worker"))]
        {
            false
        }
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
            ManagerMessage::Receipt {
                command_id,
                outcome,
            } => {
                #[cfg(feature = "nonproduction-test-worker")]
                if let Some(executor) = &mut self.executor {
                    return Ok(executor.receipt(&self.sessions, request_id, *command_id, outcome));
                }
                let _ = (command_id, outcome);
                return Ok(vec![]);
            }
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

    #[cfg(feature = "nonproduction-test-worker")]
    #[test]
    fn queued_input_is_discarded_before_control_confirmation() {
        let worker = WorkerId::new();
        let a = session(worker);
        let b = session(worker);
        let generation = Generation::INITIAL;
        let mut runtime =
            Runtime::new(generation, vec![a.clone(), b.clone()], worker, true).unwrap();
        let command = |session: &ComputerSession| Command {
            id: CommandId::new(),
            task_id: TaskId::new(),
            fence: session.fence(generation),
            action: CommandAction::Start {
                work: WorkSpec::TestInput { steps: 10 },
            },
        };
        let start_a = command(&a);
        let start_b = command(&b);
        runtime
            .handle(Envelope::new(ManagerMessage::Command(start_a.clone())))
            .unwrap();
        runtime
            .handle(Envelope::new(ManagerMessage::Command(start_b.clone())))
            .unwrap();
        let queued = runtime.tick();
        assert_eq!(queued.len(), 2);
        let mut owned = a.clone();
        owned.owner = InputOwner::Owner;
        owned.revision = owned.revision.advance().unwrap();
        let reports = runtime
            .handle(Envelope::new(ManagerMessage::Control {
                generation,
                session: owned.clone(),
            }))
            .unwrap();
        assert!(matches!(
            reports.last().unwrap().message,
            WorkerMessage::ControlApplied { .. }
        ));
        for request in queued {
            let WorkerMessage::BotInput { command_id, .. } = request.message else {
                panic!("input intent")
            };
            let reports = runtime
                .handle(Envelope {
                    version: VERSION,
                    request_id: request.request_id,
                    message: ManagerMessage::Receipt {
                        command_id,
                        outcome: Acknowledgment::Accepted,
                    },
                })
                .unwrap();
            if command_id == start_a.id {
                assert!(reports.is_empty());
            } else {
                assert!(matches!(
                    reports[0].message,
                    WorkerMessage::Event {
                        event: TaskEvent::Progress { applied: 1, .. },
                        ..
                    }
                ));
            }
        }
        let owner_id = CommandId::new();
        runtime
            .handle(Envelope::new(ManagerMessage::Input {
                command_id: owner_id,
                fence: owned.fence(generation),
                value: 1,
            }))
            .unwrap();
        let mut reclaimed = owned.clone();
        reclaimed.owner = InputOwner::Bot;
        reclaimed.revision = reclaimed.revision.advance().unwrap();
        let reports = runtime
            .handle(Envelope::new(ManagerMessage::Control {
                generation,
                session: reclaimed,
            }))
            .unwrap();
        assert!(reports.iter().any(|r| matches!(r.message, WorkerMessage::Acknowledgment { command_id, outcome: Acknowledgment::Rejected { code: ErrorCode::StaleRevision }, .. } if command_id == owner_id)));
        assert!(!runtime.tick().iter().any(|r| matches!(r.message, WorkerMessage::Acknowledgment { command_id, outcome: Acknowledgment::Applied, .. } if command_id == owner_id)));
        let status = Command {
            id: CommandId::new(),
            task_id: start_a.task_id,
            fence: owned.fence(generation),
            action: CommandAction::Status,
        };
        let reports = runtime
            .handle(Envelope::new(ManagerMessage::Command(status)))
            .unwrap();
        assert!(matches!(
            reports[0].message,
            WorkerMessage::Acknowledgment {
                outcome: Acknowledgment::Rejected {
                    code: ErrorCode::StaleRevision
                },
                ..
            }
        ));
    }
}
