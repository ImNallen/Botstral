use crate::*;

identifiers!(ManagerId, DeviceId, OwnerConnectionId, TransferId);
pub const OWNER_VERSION: u16 = 1;
pub const OWNER_CHUNK_BYTES: usize = 24 * 1024;
pub const OWNER_TRANSFER_BYTES: usize = 80 * 1024 * 1024;
pub const OWNER_TRANSFER_SECONDS: u64 = 120;
pub const MAX_OWNER_DEVICES: usize = 128;
pub const MAX_OWNER_CONNECTIONS: usize = 8;
pub const OWNER_SNAPSHOT_SERIALIZED_BOUND: usize = MAX_TASKS * 24 * 1024
    + MAX_RUNS * 4096
    + MAX_INFERENCES
        * (6 * (MAX_INFERENCE_PROMPT_BYTES + MAX_INFERENCE_OUTPUT_BYTES + 200) + 32768 + 4096)
    + MAX_INFERENCE_CONFIGURATIONS * 32768
    + MAX_RECORDS * (128 + (6 * 4096 + 256) + (6 * (4096 + 8 * 512) + 2048) + 128)
    + 4096;
pub const OWNER_SNAPSHOT_ALLOCATION_BOUND: usize = MAX_TASKS * 256 * 1024
    + MAX_RUNS * 4096
    + MAX_INFERENCES * 128 * 1024
    + MAX_INFERENCE_CONFIGURATIONS * 16 * 1024
    + MAX_RECORDS * (256 + 16 * 1024 + 32 * 1024 + 128);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeviceRole {
    Owner,
    Worker,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "String", into = "String")]
pub struct DeviceFingerprint(String);
impl DeviceFingerprint {
    pub fn from_credential(credential: &str) -> Self {
        let mut hash = Sha256::new();
        hash.update(b"botstral-owner-device-v1\0");
        hash.update(credential.as_bytes());
        Self(format!("{:x}", hash.finalize()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for DeviceFingerprint {
    type Error = ErrorCode;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        CertificateFingerprint::try_from(value.clone())?;
        Ok(Self(value))
    }
}
impl From<DeviceFingerprint> for String {
    fn from(value: DeviceFingerprint) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PairingRequest {
    pub fingerprint: DeviceFingerprint,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ManagerTrust {
    pub manager: ManagerId,
    pub endpoint: String,
    pub server_name: String,
    pub ca_pem: String,
    pub leaf: CertificateFingerprint,
}

#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OwnerInvitation {
    pub trust: ManagerTrust,
    pub role: DeviceRole,
    pub fingerprint: DeviceFingerprint,
    pub expires_at: i64,
    pub invitation: String,
}

#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum OwnerHello {
    Authenticate {
        version: u16,
        manager: ManagerId,
        role: DeviceRole,
        credential: String,
    },
    Enroll {
        version: u16,
        manager: ManagerId,
        role: DeviceRole,
        credential: String,
        invitation: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum OwnerOperation {
    Resume {
        cursor: u64,
        limit: u16,
    },
    SubmitTask {
        input: SubmitTask,
    },
    Task {
        id: TaskId,
    },
    CancelTask {
        id: TaskId,
    },
    GrantApproval {
        id: TaskId,
        expires_at: i64,
    },
    RevokeApproval {
        id: TaskId,
    },
    SetPermission {
        bot: BotId,
        expected_revision: u64,
        allow: bool,
    },
    Conversation {
        input: Conversation,
    },
    AppendMessage {
        input: ConversationMessage,
    },
    WriteDocument {
        input: WriteDocument,
    },
    SearchDocuments {
        bot: BotId,
        query: String,
    },
    RebuildDocuments,
    InferenceConfiguration {
        bot: BotId,
    },
    ConfigureInference {
        input: ConfigureInference,
    },
    SubmitInference {
        input: SubmitInference,
    },
    Inference {
        id: InferenceId,
    },
    CancelInference {
        id: InferenceId,
    },
    SubmitRun {
        input: Box<SubmitRun>,
    },
    Run {
        id: RunId,
    },
    CancelRun {
        id: RunId,
    },
    ComputerSession {
        id: ComputerSessionId,
    },
    OpenComputerSession {
        bot: BotId,
        worker: WorkerId,
    },
    SetInputOwner {
        id: ComputerSessionId,
        expected: Revision,
        owner: InputOwner,
    },
    Input {
        id: CommandId,
        fence: InputFence,
        value: u32,
    },
    Receipt {
        worker: WorkerId,
        id: CommandId,
    },
    Artifact {
        task: TaskId,
        id: ArtifactId,
    },
    CodingCalls {
        task: TaskId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum OwnerClientMessage {
    Call {
        id: RequestId,
        operation: Box<OwnerOperation>,
    },
    Watch {
        cursor: u64,
    },
    Applied {
        cursor: u64,
    },
    Ping,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OwnerReceipt {
    pub command_id: CommandId,
    pub generation: Generation,
    pub acknowledgment: Acknowledgment,
    pub last_event: Option<TaskEvent>,
    pub bot_inputs: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum OwnerReply {
    Replay(Box<EventReplay>),
    Task(Box<TaskView>),
    Permission(Permission),
    Conversation(Conversation),
    Message(ConversationMessage),
    Document(Document),
    Documents(Vec<Document>),
    Configuration(InferenceConfiguration),
    Inference(Box<InferenceView>),
    Run(Box<RunView>),
    ComputerSession(ComputerSession),
    Receipt(OwnerReceipt),
    CodingCalls(Vec<CodingCallView>),
    Artifact {
        declaration: OutputDeclaration,
        base64: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum OwnerServerMessage {
    Welcome {
        version: u16,
        manager: ManagerId,
        device: DeviceId,
    },
    Error {
        id: Option<RequestId>,
        code: ErrorCode,
    },
    Notification {
        head: u64,
    },
    Pong,
    TransferStart {
        id: TransferId,
        request: Option<RequestId>,
        bytes: u64,
        chunks: u32,
        digest: String,
    },
    TransferChunk {
        id: TransferId,
        index: u32,
        data: String,
    },
    TransferComplete {
        id: TransferId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OwnerDevice {
    pub id: DeviceId,
    pub label: String,
    pub fingerprint: DeviceFingerprint,
    pub role: DeviceRole,
    pub revoked: bool,
    pub created_at: i64,
}
