use crate::*;

identifiers!(ManagerId, DeviceId, OwnerConnectionId, TransferId);
pub const OWNER_VERSION: u16 = 3;
pub const OWNER_CHUNK_BYTES: usize = 24 * 1024;
pub const OWNER_TRANSFER_BYTES: usize = 80 * 1024 * 1024;
pub const OWNER_TRANSFER_SECONDS: u64 = 120;
pub const MAX_OWNER_DEVICES: usize = 128;
pub const MAX_OWNER_CONNECTIONS: usize = 8;
pub const OWNER_SNAPSHOT_SERIALIZED_BOUND: usize = MAX_ROUTINES * 32768
    + MAX_ROUTINE_PAGE * 4096
    + MAX_INBOX_PAGE * 32768
    + (MAX_PUBLISHERS + MAX_PUBLISHER_HISTORY) * 4096
    + MAX_CONNECTIONS * 1024
    + MAX_PROJECTED_TASKS * 24 * 1024
    + (MAX_RUNS + MAX_RUN_HISTORY) * 4096
    + MAX_INFERENCES
        * (6 * (MAX_INFERENCE_PROMPT_BYTES + MAX_INFERENCE_OUTPUT_BYTES + 200) + 32768 + 4096)
    + MAX_INFERENCE_CONFIGURATIONS * 32768
    + MAX_RECORDS * (128 + (6 * 4096 + 256) + (6 * (4096 + 8 * 512) + 2048) + 128)
    + 4096;
pub const OWNER_SNAPSHOT_ALLOCATION_BOUND: usize = MAX_ROUTINES * 65536
    + MAX_ROUTINE_PAGE * 8192
    + MAX_INBOX_PAGE * 32768
    + (MAX_PUBLISHERS + MAX_PUBLISHER_HISTORY) * 8192
    + MAX_CONNECTIONS * 2048
    + MAX_PROJECTED_TASKS * 256 * 1024
    + (MAX_RUNS + MAX_RUN_HISTORY) * 4096
    + MAX_INFERENCES * 128 * 1024
    + MAX_INFERENCE_CONFIGURATIONS * 16 * 1024
    + MAX_RECORDS * (256 + 16 * 1024 + 32 * 1024 + 128);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeviceRole {
    Owner,
    Worker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "type",
    content = "code",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum OwnerError {
    InvalidInput,
    Unauthorized,
    NotFound,
    Conflict,
    Locked,
    UnlockFailed,
    RateLimited,
    Busy,
    Internal,
    InferenceUnverified,
    InferenceBudget,
    Protocol(ErrorCode),
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
    pub pin: CertificateFingerprint,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UpdateBusyReason {
    ActiveTask,
    ActiveProcess,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum UpdateBusy {
    Busy { reason: UpdateBusyReason },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateIdentity {
    pub version: String,
    pub channel: String,
    pub revision: String,
    pub build_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateProtocols {
    pub worker: u16,
    pub owner: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VersionEnvelope {
    pub identity: UpdateIdentity,
    pub protocols: UpdateProtocols,
    pub link_schema_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UpdateTrigger {
    Timer,
    Now,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UpdateRejectReason {
    Signature,
    Tamper,
    Channel,
    Downgrade,
    Identity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RollbackMethod {
    ReinstallPackage,
    RestoreBackup,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum UpdateOutcome {
    Current,
    Deferred {
        busy: UpdateBusy,
    },
    Rejected {
        reason: UpdateRejectReason,
    },
    Applied {
        previous: UpdateIdentity,
        current: UpdateIdentity,
    },
    RolledBack {
        previous: UpdateIdentity,
        failed: UpdateIdentity,
        method: RollbackMethod,
    },
    Failed {
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateAttempt {
    pub started_at: i64,
    pub finished_at: i64,
    pub trigger: UpdateTrigger,
    pub outcome: UpdateOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateReport {
    pub installed: VersionEnvelope,
    pub last: Option<UpdateAttempt>,
}

#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum OwnerOperation {
    LockVault,
    CredentialUses,
    DecideCredentialUse {
        review: Box<CredentialUseReview>,
        decision: UseDecision,
    },
    LoginItems,
    CredentialWriteStatus {
        request: RequestId,
        close: bool,
    },
    UpdateLogin {
        id: CredentialId,
        expected_revision: CredentialRevision,
        metadata: Option<LoginMetadata>,
    },
    RoutineStatus {
        id: RequestId,
    },
    RoutineCommand {
        input: RoutineCommand,
    },
    RoutineConversation {
        bot: BotId,
        id: RequestId,
        text: String,
    },
    RoutineHistory {
        id: RoutineId,
        before: Option<OccurrenceId>,
        limit: u16,
    },
    RoutinePreview {
        trigger: RoutineTrigger,
        after: i64,
        limit: u16,
    },
    InstallConnection {
        input: InstallConnection,
    },
    Connections,
    Connection {
        id: ConnectionId,
    },
    DiscoverConnection {
        input: DiscoverConnection,
    },
    ReplaceConnectionSecret {
        id: ConnectionId,
        input: ReplaceConnectionSecret,
    },
    ReconnectConnection {
        id: ConnectionId,
        expected_revision: u64,
    },
    RevokeConnection {
        id: ConnectionId,
        expected_revision: u64,
    },
    ConnectorGrants {
        bot: BotId,
        connection: ConnectionId,
    },
    SetConnectorGrants {
        bot: BotId,
        input: SetConnectorGrants,
    },
    ConnectorTools {
        bot: BotId,
    },
    ConnectorCalls {
        task: TaskId,
    },
    ConnectorAudit {
        connection: ConnectionId,
        after: u64,
    },
    Workspace,
    GrantReviewedApproval {
        id: TaskId,
        command: Box<CommandBody>,
        permission_revision: u64,
        approval: ApprovalView,
        expires_at: i64,
    },
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
    VerifyInference {
        input: VerifyInference,
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
    StageRunFile {
        input: StageRunFile,
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
    Artifacts {
        query: ArtifactQuery,
    },
    UpdateStatus,
    ApplyUpdate,
}

#[derive(Clone, Serialize, Deserialize, JsonSchema)]
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
    VaultLocked,
    CredentialUses(Vec<CredentialUseReview>),
    CredentialUse(Box<CredentialUseReview>),
    LoginItems(Vec<LoginItemView>),
    LoginItem(LoginItemView),
    CredentialWriteStatus(CredentialWriteStatus),
    RoutineStatus(Option<RoutineControlResult>),
    RoutineControl(RoutineControlResult),
    RoutineHistory(Vec<RoutineOccurrence>),
    RoutinePreview(Vec<i64>),
    Connection(ConnectionView),
    Connections(Vec<ConnectionView>),
    ConnectorGrants(BotConnectorGrants),
    ConnectorTools(Vec<ConnectorToolDefinition>),
    ConnectorCalls(Vec<ConnectorCallView>),
    ConnectorAudit(Vec<ConnectorAudit>),
    Workspace(Box<OwnerWorkspace>),
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
    RunFileStaged,
    ComputerSession(ComputerSession),
    Receipt(OwnerReceipt),
    CodingCalls(Vec<CodingCallView>),
    Artifacts(ArtifactPage),
    Artifact {
        declaration: OutputDeclaration,
        base64: String,
    },
    Update(UpdateReport),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum OwnerServerMessage {
    ProtectedReply {
        id: RequestId,
        status: CredentialWriteStatus,
    },
    Welcome {
        version: u16,
        manager: ManagerId,
        device: DeviceId,
    },
    Error {
        id: Option<RequestId>,
        code: OwnerError,
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

pub const MAX_PROJECTED_TASKS: usize =
    MAX_TASKS + MAX_TASK_HISTORY + (MAX_RUNS + MAX_RUN_HISTORY) * MAX_RUN_TURNS as usize;
pub const MAX_WORKSPACE_BOTS: usize = 1024;
pub const MAX_WORKSPACE_WORKERS: usize = 128;
pub const OWNER_WORKSPACE_SERIALIZED_BOUND: usize = OWNER_SNAPSHOT_SERIALIZED_BOUND
    + (MAX_RUNS + MAX_RUN_HISTORY) * MAX_WORKSPACE_RUN_BYTES
    + MAX_WORKSPACE_BOTS * 1024
    + MAX_WORKSPACE_WORKERS * 256
    + MAX_WORKSPACE_BOTS * 512
    + 4096;
pub const OWNER_WORKSPACE_ALLOCATION_BOUND: usize = OWNER_SNAPSHOT_ALLOCATION_BOUND
    + (MAX_RUNS + MAX_RUN_HISTORY) * 2 * MAX_WORKSPACE_RUN_BYTES
    + MAX_WORKSPACE_BOTS * 1024
    + MAX_WORKSPACE_WORKERS * 256
    + MAX_WORKSPACE_BOTS * 512;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceBot {
    pub id: BotId,
    pub name: String,
    pub created_at: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceWorker {
    pub id: WorkerId,
    pub revoked: bool,
    pub phase: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceRun {
    pub summary: RunSummary,
    pub request: Option<SubmitRun>,
    pub turns: Vec<WorkspaceRunTurn>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceRunTurn {
    #[serde(default)]
    pub manager_outcome: Option<ManagerOutcome>,
    pub id: RunTurnId,
    pub ordinal: u32,
    pub text: Option<String>,
    pub task: Option<TaskId>,
    pub result: Option<String>,
    pub truncated: bool,
}
impl WorkspaceRun {
    pub fn project(view: RunView, preview: bool) -> Self {
        fn text(value: Option<String>, preview: bool) -> (Option<String>, bool) {
            let Some(value) = value else {
                return (None, false);
            };
            if !preview || value.len() <= 1024 {
                return (Some(value), false);
            }
            let mut end = 1024;
            while !value.is_char_boundary(end) {
                end -= 1;
            }
            (Some(value[..end].into()), true)
        }
        let mut projected = Self {
            summary: view.summary,
            request: Some(view.request),
            turns: view
                .turns
                .into_iter()
                .map(|turn| {
                    let (content, text_truncated) = text(
                        turn.response.and_then(|r| match r {
                            RunMessage::Assistant { content, .. } => Some(content),
                            _ => None,
                        }),
                        preview,
                    );
                    let result = turn
                        .manager_result
                        .as_ref()
                        .map(|r| r.result.clone())
                        .or_else(|| turn.intent.as_ref().and_then(|i| i.result.clone()));
                    let (result, result_truncated) = text(result, preview);
                    WorkspaceRunTurn {
                        manager_outcome: turn.manager_result.and_then(|r| r.outcome),
                        id: turn.id,
                        ordinal: turn.ordinal,
                        text: content,
                        task: turn.intent.map(|i| i.task),
                        result,
                        truncated: text_truncated || result_truncated,
                    }
                })
                .collect(),
        };
        projected
            .turns
            .extend(view.archived_turns.into_iter().map(|turn| {
                let (content, truncated) = text(turn.text, preview);
                let result = match &turn.manager_outcome {
                    Some(ManagerOutcome::Vault { outcome }) => outcome.notice().into(),
                    _ => "Older execution details were compacted.".into(),
                };
                WorkspaceRunTurn {
                    manager_outcome: turn.manager_outcome,
                    id: turn.id,
                    ordinal: turn.ordinal,
                    text: content,
                    task: None,
                    result: Some(result),
                    truncated,
                }
            }));
        projected.turns.sort_by_key(|turn| turn.ordinal);
        projected
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OwnerWorkspace {
    pub manager: ManagerId,
    pub cursor: u64,
    pub owner: String,
    pub bots: Vec<WorkspaceBot>,
    pub workers: Vec<WorkspaceWorker>,
    pub sessions: Vec<ComputerSession>,
    pub runs: Vec<WorkspaceRun>,
    pub run_ready_bots: Vec<BotId>,
    pub vault_unlocked: bool,
    pub state: OwnerSnapshot,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn notification_and_local_administration_cannot_become_remote_authority() {
        for value in [
            serde_json::json!({"type":"notification","head":1,"approve":true}),
            serde_json::json!({"type":"applied","cursor":1,"grant_approval":true}),
        ] {
            assert!(serde_json::from_value::<OwnerClientMessage>(value).is_err());
        }
        for method in [
            "create_owner",
            "sign_in",
            "unlock_vault",
            "replace_provider_key",
            "invite_device",
            "revoke_device",
            "invite_worker",
            "revoke_worker",
        ] {
            assert!(
                serde_json::from_value::<OwnerOperation>(serde_json::json!({"method":method}))
                    .is_err()
            );
        }
    }
}
