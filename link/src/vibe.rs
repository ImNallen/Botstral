use crate::*;

pub const VIBE_VERSION: &str = "2.24.2";
pub const VIBE_TOOLS: &[&str] = &["read_file", "grep", "write_file", "edit"];
pub const MAX_CODING_REQUEST: usize = 256 * 1024;
pub const MAX_CODING_CALLS: u32 = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CodingWorkspace {
    pub id: CodingWorkspaceId,
    pub path: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum VibeSession {
    New,
    Resume {
        task: TaskId,
        native_session: RequestId,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VibeAccess {
    EditFilesAsGuestAccount,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VibePolicy {
    pub workspace: CodingWorkspace,
    pub runtime: CertificateFingerprint,
    pub account: ProcessAccount,
    pub resources: ProcessLimits,
    pub configuration_revision: u64,
    pub model_id: String,
    pub access: VibeAccess,
    pub outputs: Vec<OutputDeclaration>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VibeToolPolicy {
    pub policy: VibePolicy,
    pub limits: RunLimits,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VibeSpec {
    pub policy: VibePolicy,
    pub session: VibeSession,
    pub prompt: String,
    pub limits: RunLimits,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VibeProposal {
    pub prompt: String,
    pub session: VibeSession,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VibeResult {
    pub native_session: RequestId,
    pub exit_code: i32,
    pub calls: u32,
    pub approval_denied: u32,
    pub tool_errors: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CodingCallPhase {
    Reserved,
    Sent,
    Settled,
    Unknown,
    NotSent,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CodingCallView {
    pub id: InferenceAttemptId,
    pub task: TaskId,
    pub phase: CodingCallPhase,
    pub reservation: InferenceReservation,
    pub accounting: InferenceAccounting,
    pub usage: Option<InferenceUsage>,
    pub provider_request_id: Option<String>,
}
impl VibeSpec {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        let p = &self.policy;
        let l = &self.limits;
        p.resources.validate()?;
        if p.account.uid == 0
            || p.configuration_revision == 0
            || p.model_id.is_empty()
            || p.model_id.len() > 128
            || !canonical_guest_path(&p.workspace.path)
            || self.prompt.is_empty()
            || self.prompt.len() > MAX_RUN_TEXT_BYTES
            || l.deadline <= 0
            || !(1..=MAX_CODING_CALLS).contains(&l.max_turns)
            || !(1..=MAX_INFERENCE_OUTPUT_TOKENS).contains(&l.max_output_tokens)
            || l.max_total_tokens > MAX_COUNTER
            || l.max_cost_microusd > MAX_COUNTER
            || l.max_concurrent_operations != 1
            || l.max_operations != 1
            || p.outputs.len() > MAX_OUTPUTS
            || serde_json::to_vec(self)
                .map_err(|_| ErrorCode::InvalidMessage)?
                .len()
                > MAX_COMPUTER_SPEC
        {
            return Err(ErrorCode::InvalidMessage);
        }
        let mut slots = std::collections::HashSet::new();
        for o in &p.outputs {
            if !valid_slot(&o.slot)
                || !slots.insert(&o.slot)
                || o.name.is_empty()
                || o.name.len() > 80
                || !o
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
                || matches!(o.name.as_str(), "." | "..")
                || o.media_type != MediaType::TextPlain
                || !(1..=1_048_576).contains(&o.max_bytes)
            {
                return Err(ErrorCode::InvalidMessage);
            }
        }
        Ok(())
    }
}
pub fn canonical_guest_path(path: &str) -> bool {
    path.starts_with('/')
        && path.len() <= 512
        && path != "/"
        && !path.chars().any(char::is_control)
        && path[1..].split('/').all(|p| !matches!(p, "" | "." | ".."))
}
