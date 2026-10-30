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

pub const ARTIFACT_PAGE_SIZE: usize = 32;
pub const MAX_ARTIFACT_NAME_QUERY: usize = 80;
pub const MAX_ARTIFACT_CURSOR_BYTES: usize = 1024;
pub const ARTIFACT_QUERY_SERIALIZED_BOUND: usize = 8192;
pub const ARTIFACT_PAGE_SERIALIZED_BOUND: usize = 64 * 1024;
pub const ARTIFACT_CATALOG_ALLOCATION_BOUND: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArtifactQuery {
    pub bot: Option<BotId>,
    pub name: String,
    pub after: Option<String>,
    pub limit: u16,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SavedArtifact {
    pub id: ArtifactId,
    pub task: TaskId,
    pub bot: BotId,
    pub output: OutputView,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArtifactPage {
    pub entries: Vec<SavedArtifact>,
    pub next: Option<String>,
}

#[cfg(test)]
mod catalog_tests {
    use super::*;
    #[test]
    fn complete_catalog_envelopes_fit_owner_admission() {
        let query = ArtifactQuery {
            bot: Some(BotId::new()),
            name: "\\".repeat(MAX_ARTIFACT_NAME_QUERY),
            after: Some("\\".repeat(MAX_ARTIFACT_CURSOR_BYTES)),
            limit: ARTIFACT_PAGE_SIZE as u16,
        };
        let request = OwnerClientMessage::Call {
            id: RequestId::new(),
            operation: Box::new(OwnerOperation::Artifacts { query }),
        };
        assert!(serde_json::to_vec(&request).unwrap().len() <= ARTIFACT_QUERY_SERIALIZED_BOUND);
        const {
            assert!(ARTIFACT_QUERY_SERIALIZED_BOUND < MAX_MESSAGE);
        }
        let entry = SavedArtifact {
            id: ArtifactId::new(),
            task: TaskId::new(),
            bot: BotId::new(),
            output: OutputView {
                declaration: OutputDeclaration {
                    slot: "s".repeat(32),
                    name: "\\".repeat(80),
                    media_type: MediaType::OctetStream,
                    max_bytes: MAX_ARTIFACT_BYTES,
                },
                state: ArtifactState::Available {
                    id: ArtifactId::new(),
                    content: ArtifactContent {
                        length: MAX_ARTIFACT_BYTES,
                        sha256: CertificateFingerprint::from_der(b"maximum"),
                    },
                },
            },
        };
        let response = OwnerReply::Artifacts(ArtifactPage {
            entries: vec![entry; ARTIFACT_PAGE_SIZE],
            next: Some("\\".repeat(MAX_ARTIFACT_CURSOR_BYTES)),
        });
        assert!(serde_json::to_vec(&response).unwrap().len() <= ARTIFACT_PAGE_SERIALIZED_BOUND);
        const {
            assert!(ARTIFACT_PAGE_SERIALIZED_BOUND < OWNER_TRANSFER_BYTES);
        }
        const {
            assert!(
                256 * 1024 + MAX_ARTIFACT_RECORDS * 2048 + ARTIFACT_PAGE_SIZE * 2048
                    < ARTIFACT_CATALOG_ALLOCATION_BOUND
            );
            assert!(ARTIFACT_CATALOG_ALLOCATION_BOUND < OWNER_WORKSPACE_ALLOCATION_BOUND);
        }
    }
}
