#[cfg(feature = "tls")]
pub mod tls;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const VERSION: u16 = 1;
pub const MAX_MESSAGE: usize = 64 * 1024;
pub const MAX_SESSIONS: usize = 64;
pub const MAX_PENDING: usize = 128;
pub const CHANNEL_CAPACITY: usize = 32;
pub const MAX_COUNTER: u64 = (1 << 53) - 1;

macro_rules! identifiers {
    ($($name:ident),+ $(,)?) => {$(
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
        pub struct $name(pub Uuid);
        impl $name {
            pub fn new() -> Self { Self(Uuid::new_v4()) }
        }
        impl Default for $name {
            fn default() -> Self { Self::new() }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { self.0.fmt(f) }
        }
        impl std::str::FromStr for $name {
            type Err = uuid::Error;
            fn from_str(value: &str) -> Result<Self, Self::Err> { value.parse().map(Self) }
        }
    )+};
}
identifiers!(
    WorkerId,
    BotId,
    ComputerSessionId,
    RequestId,
    CommandId,
    TaskId,
    ApprovalId,
    OperationId,
    ArtifactId
);

macro_rules! counters {
    ($($name:ident),+) => {$(
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
        #[serde(try_from = "u64", into = "u64")]
        pub struct $name(u64);
        impl $name {
            pub const INITIAL: Self = Self(1);
            pub fn get(self) -> u64 { self.0 }
            pub fn advance(self) -> Result<Self, ErrorCode> { Self::try_from(self.0 + 1) }
        }
        impl TryFrom<u64> for $name {
            type Error = ErrorCode;
            fn try_from(value: u64) -> Result<Self, Self::Error> {
                if (1..=MAX_COUNTER).contains(&value) { Ok(Self(value)) }
                else { Err(ErrorCode::InvalidMessage) }
            }
        }
        impl From<$name> for u64 { fn from(value: $name) -> Self { value.0 } }
    )+};
}
counters!(Generation, Revision);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "String", into = "String")]
pub struct CertificateFingerprint(String);
impl CertificateFingerprint {
    pub fn from_der(der: &[u8]) -> Self {
        Self(format!("{:x}", Sha256::digest(der)))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for CertificateFingerprint {
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
impl From<CertificateFingerprint> for String {
    fn from(value: CertificateFingerprint) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Unauthorized,
    InvalidInvitation,
    IdentityMismatch,
    InvitationExpired,
    InvitationUsed,
    IncompatibleVersion,
    MissingCapability,
    IncompatibleChannel,
    WrongPhase,
    InvalidMessage,
    StaleGeneration,
    StaleRevision,
    WrongInputOwner,
    UnsupportedExecution,
    UnknownTask,
    QueueFull,
    Disconnected,
    Conflict,
}
impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unauthorized => "unauthorized",
            Self::InvalidInvitation => "invalid_invitation",
            Self::IdentityMismatch => "identity_mismatch",
            Self::InvitationExpired => "invitation_expired",
            Self::InvitationUsed => "invitation_used",
            Self::IncompatibleVersion => "incompatible_version",
            Self::MissingCapability => "missing_capability",
            Self::IncompatibleChannel => "incompatible_channel",
            Self::WrongPhase => "wrong_phase",
            Self::InvalidMessage => "invalid_message",
            Self::StaleGeneration => "stale_generation",
            Self::StaleRevision => "stale_revision",
            Self::WrongInputOwner => "wrong_input_owner",
            Self::UnsupportedExecution => "unsupported_execution",
            Self::UnknownTask => "unknown_task",
            Self::QueueFull => "queue_full",
            Self::Disconnected => "disconnected",
            Self::Conflict => "conflict",
        }
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ErrorCode {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    LogicalSessions,
    InputFencing,
    TestInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseChannel {
    Stable,
    Experimental,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    pub versions: Vec<u16>,
    pub channel: ReleaseChannel,
    pub capabilities: Vec<Capability>,
    pub required: Vec<Capability>,
}
impl Hello {
    pub fn current(test_worker: bool) -> Self {
        let mut capabilities = vec![Capability::LogicalSessions, Capability::InputFencing];
        if test_worker {
            capabilities.push(Capability::TestInput);
        }
        Self {
            versions: vec![VERSION],
            channel: ReleaseChannel::Stable,
            required: vec![Capability::LogicalSessions, Capability::InputFencing],
            capabilities,
        }
    }
    pub fn negotiate(&self, allow_test: bool) -> Result<(), ErrorCode> {
        if self.versions.len() > 8 || self.capabilities.len() > 8 || self.required.len() > 8 {
            return Err(ErrorCode::InvalidMessage);
        }
        if !self.versions.contains(&VERSION) {
            return Err(ErrorCode::IncompatibleVersion);
        }
        if self.channel != ReleaseChannel::Stable {
            return Err(ErrorCode::IncompatibleChannel);
        }
        let available = Self::current(allow_test).capabilities;
        if [Capability::LogicalSessions, Capability::InputFencing]
            .iter()
            .any(|c| !self.capabilities.contains(c))
            || self.required.iter().any(|c| !available.contains(c))
        {
            return Err(ErrorCode::MissingCapability);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InputOwner {
    Bot,
    Owner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ControlStatus {
    Pending,
    Confirmed,
    Disconnected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComputerSession {
    pub id: ComputerSessionId,
    pub bot_id: BotId,
    pub worker_id: WorkerId,
    pub owner: InputOwner,
    pub revision: Revision,
    pub status: ControlStatus,
}
impl ComputerSession {
    pub fn fence(&self, generation: Generation) -> InputFence {
        InputFence {
            generation,
            session_id: self.id,
            revision: self.revision,
            owner: self.owner,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InputFence {
    pub generation: Generation,
    pub session_id: ComputerSessionId,
    pub revision: Revision,
    pub owner: InputOwner,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Envelope<T> {
    pub version: u16,
    pub request_id: RequestId,
    pub message: T,
}
impl<T> Envelope<T> {
    pub fn new(message: T) -> Self {
        Self {
            version: VERSION,
            request_id: RequestId::new(),
            message,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkSpec {
    Unavailable,
    TestInput { steps: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommandAction {
    Start {
        work: WorkSpec,
    },
    Status,
    Cancel,
    Result,
    ExecuteApproved {
        approval_id: ApprovalId,
        operation_id: OperationId,
        argument_digest: CertificateFingerprint,
        expires_at: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub id: CommandId,
    pub task_id: TaskId,
    pub fence: InputFence,
    pub action: CommandAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Acknowledgment {
    Accepted,
    Applied,
    Rejected { code: ErrorCode },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Running,
    Suspended,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LogCode {
    Started,
    InputApplied,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MediaType {
    Png,
    TextPlain,
    OctetStream,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArtifactReference {
    pub id: ArtifactId,
    pub sha256: CertificateFingerprint,
    pub length: u64,
    pub media_type: MediaType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TaskEvent {
    Acceptance,
    Progress {
        applied: u32,
        total: u32,
    },
    RedactedLog {
        code: LogCode,
    },
    ApprovalRequest {
        operation_id: OperationId,
        argument_digest: CertificateFingerprint,
    },
    Screenshot {
        artifact: ArtifactReference,
    },
    Artifact {
        artifact: ArtifactReference,
    },
    Completed {
        applied: u32,
    },
    Failed {
        code: ErrorCode,
    },
    Cancelled {
        applied: u32,
    },
    Status {
        status: TaskStatus,
        applied: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkerMessage {
    Hello(Hello),
    SnapshotApplied {
        generation: Generation,
    },
    ControlApplied {
        fence: InputFence,
    },
    Acknowledgment {
        generation: Generation,
        command_id: CommandId,
        outcome: Acknowledgment,
    },
    Event {
        command_id: CommandId,
        task_id: TaskId,
        fence: InputFence,
        event: TaskEvent,
    },
    BotInput {
        command_id: CommandId,
        task_id: TaskId,
        fence: InputFence,
        value: u32,
    },
    Heartbeat {
        generation: Generation,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ManagerMessage {
    Welcome {
        worker_id: WorkerId,
        generation: Generation,
        sessions: Vec<ComputerSession>,
    },
    Control {
        generation: Generation,
        session: ComputerSession,
    },
    Command(Command),
    Input {
        command_id: CommandId,
        fence: InputFence,
        value: u32,
    },
    Receipt {
        command_id: CommandId,
        outcome: Acknowledgment,
    },
    Error {
        code: ErrorCode,
    },
    Pong,
}

pub fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<Envelope<T>, ErrorCode> {
    if bytes.len() > MAX_MESSAGE {
        return Err(ErrorCode::InvalidMessage);
    }
    let envelope: Envelope<T> =
        serde_json::from_slice(bytes).map_err(|_| ErrorCode::InvalidMessage)?;
    if envelope.version != VERSION {
        return Err(ErrorCode::IncompatibleVersion);
    }
    Ok(envelope)
}

pub fn schema() -> String {
    #[derive(JsonSchema)]
    #[allow(dead_code)]
    struct Protocol {
        worker: Envelope<WorkerMessage>,
        manager: Envelope<ManagerMessage>,
    }
    serde_json::to_string_pretty(&schemars::schema_for!(Protocol)).unwrap() + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiation_and_strict_messages() {
        assert_eq!(Hello::current(false).negotiate(false), Ok(()));
        let mut hello = Hello::current(false);
        hello.versions = vec![42];
        assert_eq!(hello.negotiate(false), Err(ErrorCode::IncompatibleVersion));
        hello = Hello::current(false);
        hello.required.push(Capability::TestInput);
        assert_eq!(hello.negotiate(false), Err(ErrorCode::MissingCapability));
        let envelope = Envelope::new(WorkerMessage::Hello(Hello::current(false)));
        let mut value = serde_json::to_value(envelope).unwrap();
        value["message"]["approve"] = true.into();
        assert!(decode::<WorkerMessage>(&serde_json::to_vec(&value).unwrap()).is_err());
        assert!(serde_json::from_str::<Revision>("9007199254740992").is_err());
        assert!(Revision::try_from(MAX_COUNTER).unwrap().advance().is_err());
    }

    #[test]
    fn schema_has_no_drift() {
        assert_eq!(schema(), include_str!("../schema/protocol.json"));
    }
}
