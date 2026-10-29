use crate::*;

identifiers!(
    CredentialId,
    CredentialUseId,
    BrowserInstanceId,
    BrowserPageId,
    BrowserDocumentId,
    BrowserFrameId,
    ProfileEpoch,
    ReleaseId
);
counters!(CredentialRevision, CredentialEpoch);

pub const MAX_CREDENTIAL_ITEMS: usize = 64;
pub const MAX_CREDENTIAL_EPOCHS: usize = 512;
pub const MAX_CREDENTIAL_REQUESTS: usize = 4096;
pub const MAX_CREDENTIAL_USES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum BotAccess {
    AllOwnerBots,
    Selected { bots: Vec<BotId> },
}
impl BotAccess {
    pub fn permits(&self, bot: BotId) -> bool {
        match self {
            Self::AllOwnerBots => true,
            Self::Selected { bots } => bots.contains(&bot),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PasswordPolicy {
    EveryFill,
    RememberedOrigins,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CredentialState {
    Active,
    Removed,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LoginMetadata {
    pub label: String,
    pub bots: BotAccess,
    pub allowed_origins: Vec<String>,
    pub password_policy: PasswordPolicy,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LoginItemView {
    pub id: CredentialId,
    pub revision: CredentialRevision,
    pub epoch: CredentialEpoch,
    pub state: CredentialState,
    pub metadata: LoginMetadata,
    pub has_totp: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CredentialWriteTarget {
    Create {
        metadata: LoginMetadata,
    },
    Rotate {
        id: CredentialId,
        expected_revision: CredentialRevision,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CredentialWriteStatus {
    Rejected {
        request: RequestId,
        reason: CredentialWriteRejection,
    },
    Committed {
        request: RequestId,
        item: LoginItemView,
    },
    ClosedWithoutCommit {
        request: RequestId,
    },
    Unresolved {
        request: RequestId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CredentialWriteRejection {
    PublicValueCollision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CredentialFields {
    Password,
    Totp,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UseDecision {
    AllowOnce,
    Deny,
    RememberPassword,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UseState {
    AwaitingApproval,
    Denied,
    Authorized,
    Released,
    Applied,
    Refused,
    Uncertain,
    Revoked,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LoginSitePlan {
    pub version: u16,
    pub origin: String,
    pub route: String,
    pub frame: Option<String>,
    pub form: String,
    pub username: String,
    pub password: String,
    pub totp: String,
    pub submitter: String,
    pub action: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LoginFieldRole {
    Username,
    Password,
    Totp,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LoginField {
    pub object: RequestId,
    pub role: LoginFieldRole,
    pub name: String,
    pub input_type: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LiveDestination {
    pub profile: ProfileEpoch,
    pub browser: BrowserInstanceId,
    pub page: BrowserPageId,
    pub top_document: BrowserDocumentId,
    pub frame: BrowserFrameId,
    pub frame_document: BrowserDocumentId,
    pub top_origin: String,
    pub frame_origin: String,
    pub plan: LoginSitePlan,
    pub form: String,
    pub fields: Vec<LoginField>,
    pub submitter: String,
    pub certain: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CredentialUseReview {
    pub id: CredentialUseId,
    pub release: ReleaseId,
    pub item: LoginItemView,
    pub purpose: CredentialFields,
    pub binding: ExecutionBinding,
    pub fence: InputFence,
    pub incarnation: JournalId,
    pub destination: LiveDestination,
    pub expires_at: i64,
    pub state: UseState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProfileProtection {
    Unknown,
    Tracked {
        profile: ProfileEpoch,
        releases: Vec<ReleaseId>,
        complete: bool,
    },
}

impl LoginSitePlan {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        let route = |s: &str| {
            s.starts_with('/')
                && !s.starts_with("//")
                && s.len() <= 256
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"/-_.".contains(&b))
                && !s.split('/').any(|p| matches!(p, "." | ".."))
        };
        let selector = |s: &str| {
            !s.is_empty()
                && s.len() <= 128
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"#._-".contains(&b))
        };
        if self.version != 1
            || !self.origin.starts_with("https://")
            || self.origin.len() > 256
            || !route(&self.route)
            || !route(&self.action)
            || self.frame.as_ref().is_some_and(|s| !selector(s))
            || [
                &self.form,
                &self.username,
                &self.password,
                &self.totp,
                &self.submitter,
            ]
            .iter()
            .any(|s| !selector(s))
        {
            return Err(ErrorCode::InvalidMessage);
        }
        Ok(())
    }
}
