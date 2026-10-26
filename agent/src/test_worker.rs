use crate::session::Sessions;
use botstral_link::*;
use std::collections::{HashMap, VecDeque};

struct Task {
    command: Command,
    total: u32,
    applied: u32,
    status: TaskStatus,
    pending: Option<RequestId>,
}
struct OwnerIntent {
    request_id: RequestId,
    id: CommandId,
    fence: InputFence,
    value: u32,
}

pub(crate) struct Executor {
    tasks: HashMap<TaskId, Task>,
    owner_input: VecDeque<OwnerIntent>,
    owner_values: HashMap<ComputerSessionId, u32>,
}

fn acknowledgment(
    request_id: RequestId,
    generation: Generation,
    command_id: CommandId,
    outcome: Acknowledgment,
) -> Envelope<WorkerMessage> {
    Envelope {
        version: VERSION,
        request_id,
        message: WorkerMessage::Acknowledgment {
            generation,
            command_id,
            outcome,
        },
    }
}
fn event(command: &Command, event: TaskEvent) -> Envelope<WorkerMessage> {
    Envelope::new(WorkerMessage::Event {
        command_id: command.id,
        task_id: command.task_id,
        fence: command.fence,
        event,
    })
}

impl Executor {
    pub(crate) fn new() -> Self {
        Self {
            tasks: HashMap::new(),
            owner_input: VecDeque::new(),
            owner_values: HashMap::new(),
        }
    }

    pub(crate) fn handle(
        &mut self,
        sessions: &Sessions,
        request_id: RequestId,
        message: &ManagerMessage,
    ) -> Vec<Envelope<WorkerMessage>> {
        let (id, fence) = match message {
            ManagerMessage::Command(command) => (command.id, command.fence),
            ManagerMessage::Input {
                command_id, fence, ..
            } => (*command_id, *fence),
            _ => unreachable!(),
        };
        let result = match message {
            ManagerMessage::Input { value, .. } => {
                if fence.owner != InputOwner::Owner {
                    Err(ErrorCode::WrongInputOwner)
                } else if self.owner_input.len() >= CHANNEL_CAPACITY {
                    Err(ErrorCode::QueueFull)
                } else {
                    self.owner_input.push_back(OwnerIntent {
                        request_id,
                        id,
                        fence,
                        value: *value,
                    });
                    Ok(vec![])
                }
            }
            ManagerMessage::Command(command) => self.command(command),
            _ => unreachable!(),
        };
        match result {
            Ok(mut reports) => {
                reports.insert(
                    0,
                    acknowledgment(
                        request_id,
                        sessions.generation,
                        id,
                        Acknowledgment::Accepted,
                    ),
                );
                reports
            }
            Err(code) => vec![acknowledgment(
                request_id,
                sessions.generation,
                id,
                Acknowledgment::Rejected { code },
            )],
        }
    }

    fn command(&mut self, command: &Command) -> Result<Vec<Envelope<WorkerMessage>>, ErrorCode> {
        match command.action {
            CommandAction::Start {
                work: WorkSpec::TestInput { steps },
            } => {
                if command.fence.owner != InputOwner::Bot {
                    return Err(ErrorCode::WrongInputOwner);
                }
                if !(1..=10000).contains(&steps) {
                    return Err(ErrorCode::InvalidMessage);
                }
                if self.tasks.contains_key(&command.task_id) {
                    return Err(ErrorCode::Conflict);
                }
                if self.tasks.len() >= 16 {
                    return Err(ErrorCode::QueueFull);
                }
                self.tasks.insert(
                    command.task_id,
                    Task {
                        command: command.clone(),
                        total: steps,
                        applied: 0,
                        status: TaskStatus::Running,
                        pending: None,
                    },
                );
                Ok(vec![
                    event(command, TaskEvent::Acceptance),
                    event(
                        command,
                        TaskEvent::RedactedLog {
                            code: LogCode::Started,
                        },
                    ),
                ])
            }
            CommandAction::Cancel | CommandAction::Status | CommandAction::Result => {
                let task = self
                    .tasks
                    .get_mut(&command.task_id)
                    .ok_or(ErrorCode::UnknownTask)?;
                if task.command.fence.session_id != command.fence.session_id {
                    return Err(ErrorCode::Unauthorized);
                }
                let report = match command.action {
                    CommandAction::Cancel => {
                        if matches!(task.status, TaskStatus::Running | TaskStatus::Suspended) {
                            task.status = TaskStatus::Cancelled;
                            task.pending = None;
                        }
                        if task.status == TaskStatus::Cancelled {
                            TaskEvent::Cancelled {
                                applied: task.applied,
                            }
                        } else {
                            TaskEvent::Status {
                                status: task.status,
                                applied: task.applied,
                            }
                        }
                    }
                    CommandAction::Result if task.status == TaskStatus::Completed => {
                        TaskEvent::Completed {
                            applied: task.applied,
                        }
                    }
                    _ => TaskEvent::Status {
                        status: task.status,
                        applied: task.applied,
                    },
                };
                Ok(vec![event(command, report)])
            }
            _ => Err(ErrorCode::UnsupportedExecution),
        }
    }

    pub(crate) fn control(&mut self, fence: InputFence) -> Vec<Envelope<WorkerMessage>> {
        for task in self
            .tasks
            .values_mut()
            .filter(|t| t.command.fence.session_id == fence.session_id && t.command.fence != fence)
        {
            task.pending = None;
            if task.status == TaskStatus::Running {
                task.status = TaskStatus::Suspended;
            }
        }
        let mut reports = Vec::new();
        self.owner_input.retain(|input| {
            if input.fence.session_id == fence.session_id && input.fence != fence {
                reports.push(acknowledgment(
                    input.request_id,
                    fence.generation,
                    input.id,
                    Acknowledgment::Rejected {
                        code: ErrorCode::StaleRevision,
                    },
                ));
                false
            } else {
                true
            }
        });
        reports
    }

    pub(crate) fn tick(&mut self, sessions: &Sessions) -> Vec<Envelope<WorkerMessage>> {
        let mut reports = Vec::new();
        while let Some(input) = self.owner_input.pop_front() {
            let outcome = match sessions.check(input.fence) {
                Ok(()) => {
                    self.owner_values
                        .insert(input.fence.session_id, input.value);
                    Acknowledgment::Applied
                }
                Err(code) => Acknowledgment::Rejected { code },
            };
            reports.push(acknowledgment(
                input.request_id,
                sessions.generation,
                input.id,
                outcome,
            ));
        }
        for task in self
            .tasks
            .values_mut()
            .filter(|t| t.status == TaskStatus::Running && t.pending.is_none())
        {
            if sessions.check(task.command.fence).is_err() {
                task.status = TaskStatus::Suspended;
                continue;
            }
            let request_id = RequestId::new();
            task.pending = Some(request_id);
            reports.push(Envelope {
                version: VERSION,
                request_id,
                message: WorkerMessage::BotInput {
                    command_id: task.command.id,
                    task_id: task.command.task_id,
                    fence: task.command.fence,
                    value: task.applied + 1,
                },
            });
        }
        reports
    }

    pub(crate) fn receipt(
        &mut self,
        sessions: &Sessions,
        request_id: RequestId,
        command_id: CommandId,
        outcome: &Acknowledgment,
    ) -> Vec<Envelope<WorkerMessage>> {
        let Some(task) = self
            .tasks
            .values_mut()
            .find(|t| t.pending == Some(request_id) && t.command.id == command_id)
        else {
            return vec![];
        };
        task.pending = None;
        if *outcome != Acknowledgment::Accepted
            || sessions.check(task.command.fence).is_err()
            || task.status != TaskStatus::Running
        {
            task.status = TaskStatus::Suspended;
            return vec![];
        }
        task.applied += 1;
        let report = if task.applied == task.total {
            task.status = TaskStatus::Completed;
            TaskEvent::Completed {
                applied: task.applied,
            }
        } else {
            TaskEvent::Progress {
                applied: task.applied,
                total: task.total,
            }
        };
        vec![event(&task.command, report)]
    }
}
