use crate::{BotId, InferenceAttemptId, InferenceId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MAX_INFERENCE_PROMPT_BYTES: usize = 4096;
pub const MAX_INFERENCE_OUTPUT_BYTES: usize = 32768;
pub const MAX_INFERENCE_OUTPUT_TOKENS: u32 = 16384;
pub const MAX_INFERENCE_CONTEXT_TOKENS: u32 = 2_000_000;
pub const MAX_INFERENCE_SECONDS: i64 = 120;
pub const MAX_INFERENCES: usize = 128;
pub const MAX_INFERENCE_CONFIGURATIONS: usize = 256;
pub const MAX_INFERENCE_CALLS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InferenceEndpoint {
    Global,
    Eu,
    Us,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelReleaseAcknowledgment {
    Stable,
    Preview { terms_url: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InferenceAcknowledgment {
    pub model_developer: String,
    pub developer_terms_url: String,
    pub privacy_terms_url: String,
    pub acknowledged_at: i64,
    pub release: ModelReleaseAcknowledgment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InferenceBilling {
    TextTokensOnlyAtAcknowledgedCeilings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InferencePrices {
    pub input_microusd_per_million_tokens: u64,
    pub output_microusd_per_million_tokens: u64,
    pub effective_at: i64,
    pub expires_at: i64,
    pub billing: InferenceBilling,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InferencePolicy {
    pub endpoint: InferenceEndpoint,
    pub model_id: String,
    pub context_tokens: u32,
    pub acknowledgment: InferenceAcknowledgment,
    pub prices: InferencePrices,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigureInference {
    pub bot: BotId,
    pub expected_revision: u64,
    pub policy: InferencePolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum InferenceVerification {
    Unverified,
    Verified { checked_at: i64, expires_at: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InferenceConfiguration {
    pub bot: BotId,
    pub revision: u64,
    pub policy: InferencePolicy,
    pub verification: InferenceVerification,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VerifyInference {
    pub bot: BotId,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SubmitInference {
    pub id: InferenceId,
    pub bot: BotId,
    pub configuration_revision: u64,
    pub prompt: String,
    pub deadline: i64,
    pub max_output_tokens: u32,
    pub max_total_tokens: u64,
    pub max_cost_microusd: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InferenceNotSentReason {
    Deadline,
    ConfigurationChanged,
    VaultLocked,
    Shutdown,
    Restart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InferenceAttentionReason {
    Interrupted,
    ProviderRejected,
    InvalidResponse,
    InvalidUsage,
    UnexpectedModel,
    ResponseTooLarge,
    Shutdown,
    Restart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InferenceFinishReason {
    Stop,
    Length,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum InferencePhase {
    Admitted,
    Sending {
        attempt: InferenceAttemptId,
        started_at: i64,
    },
    Completed {
        attempt: InferenceAttemptId,
        finish_reason: InferenceFinishReason,
    },
    Cancelled,
    NotSent {
        reason: InferenceNotSentReason,
    },
    AwaitingAttention {
        attempt: InferenceAttemptId,
        reason: InferenceAttentionReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InferenceReservation {
    pub total_tokens: u64,
    pub cost_microusd: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum InferenceAccounting {
    Held,
    Settled { charged_microusd: u64 },
    Released,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InferenceUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InferenceView {
    pub request: SubmitInference,
    pub configuration: InferenceConfiguration,
    pub phase: InferencePhase,
    pub reservation: InferenceReservation,
    pub accounting: InferenceAccounting,
    pub created_at: i64,
    pub updated_at: i64,
    pub cancellation_requested_at: Option<i64>,
    pub usage: Option<InferenceUsage>,
    pub provider_request_id: Option<String>,
    pub output: Option<String>,
}
