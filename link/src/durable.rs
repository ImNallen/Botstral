use super::*;

identifiers!(OwnerId, ConversationId, MessageId, ExecutionId, JournalId);
pub const MAX_TASKS: usize = 128;
pub const MAX_RECORDS: usize = 256;
pub const REPORT_PAGE: usize = 16;
pub const PAGE_BYTES: usize = 48 * 1024;
pub const EVENT_RETENTION: u64 = 128;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskSettings {
    pub model: String,
    pub max_turns: u32,
    pub max_tokens: u32,
    pub deadline: i64,
    pub max_cost_micros: u64,
}
impl TaskSettings {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        if self.model.is_empty()
            || self.model.len() > 128
            || self.model.chars().any(char::is_control)
            || !(1..=1000).contains(&self.max_turns)
            || !(1..=1_000_000).contains(&self.max_tokens)
            || self.deadline <= 0
            || self.max_cost_micros > MAX_COUNTER
        {
            return Err(ErrorCode::InvalidMessage);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SubmitTask {
    pub id: TaskId,
    pub session_id: ComputerSessionId,
    pub conversation_id: Option<ConversationId>,
    pub settings: TaskSettings,
    pub work: WorkSpec,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Queued,
    Running,
    AwaitingApproval,
    Cancelling,
    Completed,
    Failed,
    Cancelled,
    AwaitingAttention,
}
impl TaskState {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
    pub fn reduce(self, event: &ExecutionEvent) -> Self {
        if self.terminal() {
            return self;
        }
        if self == Self::Cancelling {
            return if matches!(event, ExecutionEvent::Stopped { .. }) {
                Self::Cancelled
            } else {
                self
            };
        }
        match event {
            ExecutionEvent::Accepted => {
                if self == Self::Queued {
                    Self::Running
                } else {
                    self
                }
            }
            ExecutionEvent::ApprovalRequired => {
                if matches!(self, Self::Queued | Self::Running) {
                    Self::AwaitingApproval
                } else {
                    self
                }
            }
            ExecutionEvent::Admitted => {
                if self == Self::AwaitingAttention {
                    self
                } else {
                    Self::Running
                }
            }
            ExecutionEvent::Progress { .. } | ExecutionEvent::ProcessResult { .. } => self,
            ExecutionEvent::ProcessFinished { success, .. } => {
                if *success {
                    Self::Completed
                } else {
                    Self::Failed
                }
            }
            ExecutionEvent::Completed { .. } => Self::Completed,
            ExecutionEvent::Failed { .. } => Self::Failed,
            ExecutionEvent::Uncertain { .. } => Self::AwaitingAttention,
            ExecutionEvent::Stopped { .. } => self,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommandBody {
    pub id: CommandId,
    pub owner: OwnerId,
    pub task: TaskId,
    pub bot: BotId,
    pub worker: WorkerId,
    pub session: ComputerSessionId,
    pub operation: OperationId,
    pub execution: ExecutionId,
    pub work: WorkSpec,
}
impl CommandBody {
    pub fn matches_authority(&self, binding: &ExecutionBinding) -> bool {
        self.owner == binding.owner
            && self.bot == binding.bot
            && self.task == binding.task
            && self.operation == binding.operation
            && self.execution == binding.execution
            && self.argument_digest() == binding.arguments
    }
    pub fn argument_digest(&self) -> ArgumentDigest {
        ArgumentDigest(format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&self.work).expect("typed work"))
        ))
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "String", into = "String")]
pub struct ArgumentDigest(String);
impl TryFrom<String> for ArgumentDigest {
    type Error = ErrorCode;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            Ok(Self(value))
        } else {
            Err(ErrorCode::InvalidMessage)
        }
    }
}
impl From<ArgumentDigest> for String {
    fn from(v: ArgumentDigest) -> String {
        v.0
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExecutionBinding {
    pub owner: OwnerId,
    pub bot: BotId,
    pub task: TaskId,
    pub operation: OperationId,
    pub arguments: ArgumentDigest,
    pub permission_revision: u64,
    pub approval: ApprovalId,
    pub execution: ExecutionId,
    pub expires_at: i64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Delivery {
    pub body: CommandBody,
    pub fence: InputFence,
    pub authority: Option<ExecutionBinding>,
    pub cancel: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StopEvidence {
    NoProcessGroup,
    NotLaunched,
    SystemdScopeEmpty { scope: ProcessScope },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExecutionEvent {
    ProcessResult {
        evidence: ProcessEvidence,
    },
    ProcessFinished {
        reason: ProcessCompletionReason,
        success: bool,
        scope: ProcessScope,
    },
    Accepted,
    ApprovalRequired,
    Admitted,
    Progress {
        applied: u32,
    },
    Completed {
        applied: u32,
    },
    Failed {
        code: ErrorCode,
    },
    Uncertain {
        reason: String,
    },
    Stopped {
        applied: u32,
        evidence: StopEvidence,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DurableReport {
    pub sequence: u64,
    pub command: CommandId,
    pub execution: ExecutionId,
    pub event: ExecutionEvent,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApprovalView {
    pub id: ApprovalId,
    pub binding: Option<ExecutionBinding>,
    pub permission_revision: u64,
    pub revoked: bool,
    pub consumed: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskView {
    pub id: TaskId,
    pub request: SubmitTask,
    pub command: CommandBody,
    pub state: TaskState,
    pub progress: u32,
    pub outcome: Option<ExecutionEvent>,
    pub effect_evidence: Vec<ExecutionEvent>,
    #[serde(default)]
    pub process: Option<ProcessEvidence>,
    #[serde(default)]
    pub outputs: Vec<OutputView>,
    pub approval: ApprovalView,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Conversation {
    pub id: ConversationId,
    pub bot: BotId,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConversationMessage {
    pub id: MessageId,
    pub conversation: ConversationId,
    pub text: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    Instructions,
    Memory,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceReference {
    Task { id: TaskId },
    Message { id: MessageId },
    External { reference: String },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub bot: BotId,
    pub kind: DocumentKind,
    pub revision: u64,
    pub sources: Vec<SourceReference>,
    pub markdown: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WriteDocument {
    pub bot: BotId,
    pub kind: DocumentKind,
    pub expected_revision: u64,
    pub sources: Vec<SourceReference>,
    pub markdown: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AuthoritativeEvent {
    pub sequence: u64,
    pub kind: String,
    pub entity: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OwnerSnapshot {
    pub tasks: Vec<TaskView>,
    pub inferences: Vec<crate::InferenceView>,
    pub inference_configurations: Vec<crate::InferenceConfiguration>,
    pub conversations: Vec<Conversation>,
    pub messages: Vec<ConversationMessage>,
    pub documents: Vec<Document>,
    pub permissions: Vec<Permission>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Permission {
    pub bot: BotId,
    pub revision: u64,
    pub allow: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EventReplay {
    Events {
        entries: Vec<AuthoritativeEvent>,
        cursor: u64,
    },
    Snapshot {
        state: OwnerSnapshot,
        cursor: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_and_terminal_states_do_not_regress() {
        let states = [
            TaskState::Queued,
            TaskState::Running,
            TaskState::AwaitingApproval,
            TaskState::Cancelling,
            TaskState::Completed,
            TaskState::Failed,
            TaskState::Cancelled,
            TaskState::AwaitingAttention,
        ];
        let events = [
            ExecutionEvent::Accepted,
            ExecutionEvent::ApprovalRequired,
            ExecutionEvent::Admitted,
            ExecutionEvent::Progress { applied: 1 },
            ExecutionEvent::Completed { applied: 1 },
            ExecutionEvent::Failed {
                code: ErrorCode::UnsupportedExecution,
            },
            ExecutionEvent::Uncertain {
                reason: "unknown".into(),
            },
            ExecutionEvent::Stopped {
                applied: 1,
                evidence: StopEvidence::NoProcessGroup,
            },
        ];
        for state in states {
            for event in &events {
                let next = state.reduce(event);
                if state.terminal() {
                    assert_eq!(next, state);
                }
                if state == TaskState::Cancelling {
                    assert_eq!(
                        next,
                        if matches!(event, ExecutionEvent::Stopped { .. }) {
                            TaskState::Cancelled
                        } else {
                            state
                        }
                    );
                }
                if state == TaskState::AwaitingAttention
                    && matches!(
                        event,
                        ExecutionEvent::Accepted
                            | ExecutionEvent::Admitted
                            | ExecutionEvent::ApprovalRequired
                            | ExecutionEvent::Progress { .. }
                    )
                {
                    assert_eq!(next, state);
                }
            }
        }
    }
    #[test]
    fn typed_argument_normalization_rejects_unknown_fields() {
        let a: WorkSpec =
            serde_json::from_str(r#"{"type":"test_effect","value":3,"observable":true}"#).unwrap();
        let b: WorkSpec =
            serde_json::from_str(r#"{"observable":true,"value":3,"type":"test_effect"}"#).unwrap();
        assert_eq!(a, b);
        assert!(
            serde_json::from_str::<WorkSpec>(
                r#"{"type":"test_effect","value":3.0,"observable":true}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<WorkSpec>(
                r#"{"type":"test_effect","value":3,"observable":true,"grant":true}"#
            )
            .is_err()
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JournalPhase {
    Prepared,
    ClaimPending,
    Admitted,
    Resolved,
    Uncertain,
    Stopped,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JournalEntry {
    pub body: CommandBody,
    pub phase: JournalPhase,
}

#[cfg(test)]
mod binding_tests {
    use super::*;
    #[test]
    fn authority_binds_the_actual_adapter_arguments() {
        let mut body = CommandBody {
            id: CommandId::new(),
            owner: OwnerId::new(),
            task: TaskId::new(),
            bot: BotId::new(),
            worker: WorkerId::new(),
            session: ComputerSessionId::new(),
            operation: OperationId::new(),
            execution: ExecutionId::new(),
            work: WorkSpec::TestEffect {
                value: 1,
                observable: true,
            },
        };
        let binding = ExecutionBinding {
            owner: body.owner,
            bot: body.bot,
            task: body.task,
            operation: body.operation,
            arguments: body.argument_digest(),
            permission_revision: 1,
            approval: ApprovalId::new(),
            execution: body.execution,
            expires_at: 4000000000,
        };
        assert!(body.matches_authority(&binding));
        body.work = WorkSpec::TestEffect {
            value: 2,
            observable: true,
        };
        assert!(!body.matches_authority(&binding));
    }
}
