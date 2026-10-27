use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactUnavailable {
    Missing,
    Rejected,
    ExecutionUncertain,
    Corrupt,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArtifactContent {
    pub sha256: CertificateFingerprint,
    pub length: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SealedOutput {
    Content { content: ArtifactContent },
    Unavailable { reason: ArtifactUnavailable },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArtifactOffer {
    pub journal: JournalId,
    pub task: TaskId,
    pub execution: ExecutionId,
    pub slot: String,
    pub output: SealedOutput,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ArtifactState {
    Pending,
    Transferring {
        id: ArtifactId,
        content: ArtifactContent,
    },
    Available {
        id: ArtifactId,
        content: ArtifactContent,
    },
    Unavailable {
        reason: ArtifactUnavailable,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OutputView {
    pub declaration: OutputDeclaration,
    pub state: ArtifactState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArtifactChunk {
    pub journal: JournalId,
    pub execution: ExecutionId,
    pub slot: String,
    pub id: ArtifactId,
    pub offset: u64,
    pub hex: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactTransferState {
    Receiving,
    Stored,
    Unavailable,
}
