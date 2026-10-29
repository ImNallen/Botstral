use crate::*;
use std::collections::BTreeMap;

identifiers!(RoutineId, OccurrenceId, PublisherId, InboxId);
pub const MAX_ROUTINES: usize = 128;
pub const MAX_PUBLISHERS: usize = 32;
pub const MAX_PUBLISHER_HISTORY: usize = 16;
pub const MAX_ROUTINE_PAGE: usize = 32;
pub const MAX_INBOX_PAGE: usize = 64;
pub const MAX_ROUTINE_BYTES: usize = 24 * 1024;
pub const MAX_EVENT_BYTES: usize = 8192;
pub const EVENT_REPLAY_SECONDS: i64 = 86400;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunTemplate {
    pub session: ComputerSessionId,
    pub session_revision: Revision,
    pub configuration_revision: u64,
    pub permission_revision: u64,
    pub prompt: String,
    pub execution_policy: RunExecutionPolicy,
    pub duration_seconds: u32,
    pub max_turns: u32,
    pub max_total_tokens: u64,
    pub max_cost_microusd: u64,
    pub max_output_tokens: u32,
    pub max_operations: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoutineTrigger {
    Once {
        at: i64,
    },
    Interval {
        anchor: i64,
        seconds: u32,
    },
    Daily {
        hour: u8,
        minute: u8,
        timezone: String,
    },
    Event {
        publisher: PublisherId,
        topic: String,
        equals: BTreeMap<String, EventScalar>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum EventScalar {
    Text(String),
    Integer(i64),
    Boolean(bool),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WakeupGrant {
    pub horizon_seconds: u32,
    pub minimum_spacing_seconds: u32,
    pub max_pending: u16,
    pub max_total: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RoutineDefinition {
    pub bot: BotId,
    pub name: String,
    pub trigger: RoutineTrigger,
    pub template: RunTemplate,
    pub catchup_seconds: u32,
    pub wakeups: Option<WakeupGrant>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RoutineState {
    Enabled,
    Paused,
    Deleted,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RoutineView {
    pub id: RoutineId,
    pub revision: u64,
    pub state: RoutineState,
    pub definition: RoutineDefinition,
    pub next_at: Option<i64>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum OccurrenceStatus {
    Pending,
    Running,
    Decision { reason: String },
    Finished { phase: RunPhase },
    Blocked { reason: String },
    Skipped { reason: String },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RoutineOccurrence {
    pub id: OccurrenceId,
    pub routine: RoutineId,
    pub revision: u64,
    pub trigger_key: String,
    pub scheduled_at: i64,
    pub run: Option<RunId>,
    pub status: OccurrenceStatus,
    pub skipped_count: u32,
    pub updated_at: i64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RoutineBudget {
    pub max_runs: u32,
    pub max_tokens: u64,
    pub max_cost_microusd: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublisherView {
    pub id: PublisherId,
    pub bot: BotId,
    pub name: String,
    pub topics: Vec<String>,
    pub revision: u64,
    pub revoked: bool,
    pub last_received_at: Option<i64>,
    pub last_error: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublisherEvent {
    pub id: String,
    pub timestamp: i64,
    pub topic: String,
    pub fields: BTreeMap<String, EventScalar>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EventReceipt {
    pub publisher: PublisherId,
    pub event_id: String,
    pub matched: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InboxEntry {
    pub message: String,
    pub id: InboxId,
    pub occurrence: OccurrenceId,
    pub bot: BotId,
    pub routine: RoutineId,
    pub run: Option<RunId>,
    pub status: OccurrenceStatus,
    pub created_at: i64,
    pub acknowledged: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RoutineCommand {
    pub id: RequestId,
    pub action: RoutineAction,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoutineAction {
    Create {
        id: RoutineId,
        definition: Box<RoutineDefinition>,
    },
    Replace {
        id: RoutineId,
        expected_revision: u64,
        definition: Box<RoutineDefinition>,
    },
    SetPaused {
        id: RoutineId,
        expected_revision: u64,
        paused: bool,
    },
    Test {
        id: RoutineId,
        expected_revision: u64,
    },
    Delete {
        id: RoutineId,
        expected_revision: u64,
    },
    SetBudget {
        budget: RoutineBudget,
    },
    InstallPublisher {
        id: PublisherId,
        bot: BotId,
        name: String,
        topics: Vec<String>,
        credential_sha256: String,
    },
    RevokePublisher {
        id: PublisherId,
        expected_revision: u64,
    },
    DeleteBot {
        bot: BotId,
    },
    DuplicateBot {
        bot: BotId,
        copy: BotId,
        name: String,
    },
    AcknowledgeInbox {
        id: InboxId,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RoutineControlResult {
    pub request: RequestId,
    pub message: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestWakeup {
    pub at: i64,
    pub prompt: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ManagerToolResult {
    pub call: RunToolCall,
    pub result: String,
    #[serde(default)]
    pub outcome: Option<ManagerOutcome>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RoutineSnapshot {
    pub routines: Vec<RoutineView>,
    pub occurrences: Vec<RoutineOccurrence>,
    pub inbox: Vec<InboxEntry>,
    pub publishers: Vec<PublisherView>,
    pub budget: Option<RoutineBudget>,
}
