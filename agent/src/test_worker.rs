use crate::session::Sessions;
use botstral_link::*;
use std::collections::{HashMap, VecDeque};
struct OwnerIntent {
    request_id: RequestId,
    id: CommandId,
    fence: InputFence,
    value: u32,
}
pub(crate) struct Executor {
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
impl Executor {
    pub(crate) fn new() -> Self {
        Self {
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
        let ManagerMessage::Input {
            command_id,
            fence,
            value,
        } = message
        else {
            return vec![];
        };
        let outcome = if fence.owner != InputOwner::Owner {
            Acknowledgment::Rejected {
                code: ErrorCode::WrongInputOwner,
            }
        } else if self.owner_input.len() >= CHANNEL_CAPACITY {
            Acknowledgment::Rejected {
                code: ErrorCode::QueueFull,
            }
        } else {
            self.owner_input.push_back(OwnerIntent {
                request_id,
                id: *command_id,
                fence: *fence,
                value: *value,
            });
            Acknowledgment::Accepted
        };
        vec![acknowledgment(
            request_id,
            sessions.generation,
            *command_id,
            outcome,
        )]
    }
    pub(crate) fn control(&mut self, fence: InputFence) -> Vec<Envelope<WorkerMessage>> {
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
        self.owner_input
            .drain(..)
            .map(|input| {
                let outcome = match sessions.check(input.fence) {
                    Ok(()) => {
                        self.owner_values
                            .insert(input.fence.session_id, input.value);
                        Acknowledgment::Applied
                    }
                    Err(code) => Acknowledgment::Rejected { code },
                };
                acknowledgment(input.request_id, sessions.generation, input.id, outcome)
            })
            .collect()
    }
}
