use crate::*;
use std::collections::BTreeMap;

pub const MAX_RUNS: usize = 16;
pub const MAX_RUN_TURNS: u32 = 8;
pub const MAX_RUN_SECONDS: i64 = 3600;
pub const MAX_RUN_TRANSCRIPT_BYTES: usize = 32 * 1024;
pub const MAX_RUN_RESULT_BYTES: usize = 8 * 1024;
pub const MAX_RUN_TEXT_BYTES: usize = 4096;
pub const MAX_RUN_VIEW_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComputerToolPolicy {
    pub profile: ComputerProfile,
    pub account: ProcessAccount,
    pub runtime: ComputerRuntime,
    pub limits: ProcessLimits,
    pub viewport: Viewport,
    pub outputs: Vec<OutputDeclaration>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProcessToolPolicy {
    pub profile: ProcessProfile,
    pub account: ProcessAccount,
    pub environment: BTreeMap<String, String>,
    pub limits: ProcessLimits,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunExecutionPolicy {
    #[serde(default)]
    pub connectors: Option<ConnectorToolPolicy>,
    #[serde(default)]
    pub vision: Option<VisionPolicy>,
    #[serde(default)]
    pub vibe: Option<VibeToolPolicy>,
    pub computer: Option<ComputerToolPolicy>,
    pub process: Option<ProcessToolPolicy>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunLimits {
    pub deadline: i64,
    pub max_turns: u32,
    pub max_total_tokens: u64,
    pub max_cost_microusd: u64,
    pub max_output_tokens: u32,
    pub max_operations: u32,
    pub max_concurrent_operations: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SubmitRun {
    #[serde(default)]
    pub images: Vec<ImageReference>,
    pub id: RunId,
    pub bot: BotId,
    pub session: ComputerSessionId,
    pub configuration_revision: u64,
    pub prompt: String,
    pub execution_policy: RunExecutionPolicy,
    pub limits: RunLimits,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComputerProposal {
    pub operations: Vec<ComputerOperation>,
    pub outputs: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProcessProposal {
    pub executable: String,
    pub args: Vec<String>,
    pub cwd: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "tool",
    content = "arguments",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ToolProposal {
    Computer(ComputerProposal),
    Process(ProcessProposal),
    Vibe(VibeProposal),
    Connector(ConnectorProposal),
    RequestWakeup(RequestWakeup),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunToolCall {
    pub id: String,
    pub proposal: ToolProposal,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "role", rename_all = "snake_case", deny_unknown_fields)]
pub enum RunMessage {
    UserMultimodal {
        content: String,
        images: Vec<AuthorizedImage>,
    },
    User {
        content: String,
    },
    Assistant {
        content: String,
        tool_call: Option<RunToolCall>,
    },
    Tool {
        tool_call_id: String,
        content: String,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunStopReason {
    TurnLimit,
    OutputLimit,
    TokenLimit,
    CostLimit,
    OperationLimit,
    Deadline,
    ConfigurationChanged,
    VaultLocked,
    UnsupportedProposal,
    TranscriptLimit,
    ChildUnavailable,
    Shutdown,
    Restart,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunAttentionReason {
    Provider,
    Worker,
    ResultInvalid,
    Restart,
    Shutdown,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RunPhase {
    Ready,
    Calling { turn: RunTurnId },
    WaitingForTool { turn: RunTurnId, task: TaskId },
    Completed,
    Cancelling,
    Cancelled,
    Stopped { reason: RunStopReason },
    AwaitingAttention { reason: RunAttentionReason },
}
impl RunPhase {
    pub fn terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed
                | Self::Cancelled
                | Self::Stopped { .. }
                | Self::AwaitingAttention { .. }
        )
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunAccounting {
    #[serde(default)]
    pub admitted_coding_calls: u32,
    pub admitted_turns: u32,
    pub admitted_operations: u32,
    pub settled_tokens: u64,
    pub settled_cost_microusd: u64,
    pub held_tokens: u64,
    pub held_cost_microusd: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunSummary {
    pub id: RunId,
    pub bot: BotId,
    pub phase: RunPhase,
    pub accounting: RunAccounting,
    pub cancellation_requested_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RunTurnPhase {
    Sending,
    Responded,
    NotSent { reason: InferenceNotSentReason },
    AwaitingAttention { reason: InferenceAttentionReason },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunToolIntent {
    pub call: RunToolCall,
    pub task: TaskId,
    pub command: CommandId,
    pub operation: OperationId,
    pub execution: ExecutionId,
    pub work: WorkSpec,
    pub result: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunTurn {
    #[serde(default)]
    pub manager_result: Option<ManagerToolResult>,
    pub id: RunTurnId,
    pub ordinal: u32,
    pub attempt: RunAttemptId,
    pub messages: Vec<RunMessage>,
    pub reservation: InferenceReservation,
    pub accounting: InferenceAccounting,
    pub phase: RunTurnPhase,
    pub finish_reason: Option<InferenceFinishReason>,
    pub usage: Option<InferenceUsage>,
    pub provider_request_id: Option<String>,
    pub response: Option<RunMessage>,
    pub intent: Option<RunToolIntent>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunView {
    pub summary: RunSummary,
    pub request: SubmitRun,
    pub configuration: InferenceConfiguration,
    pub turns: Vec<RunTurn>,
}
