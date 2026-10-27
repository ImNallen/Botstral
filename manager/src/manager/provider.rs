use super::*;
use botstral_link::*;
use serde::Deserialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

pub(super) const MAX_PROVIDER_BODY: usize = 65536;

pub(super) struct CallPermit {
    pub view: InferenceView,
    pub attempt: InferenceAttemptId,
    pub secret: SecretString,
}

pub(super) struct VerificationPermit {
    pub configuration: InferenceConfiguration,
    pub epoch: i64,
    pub secret: SecretString,
}

pub(super) enum ProviderOutcome {
    NotSent {
        reason: InferenceNotSentReason,
    },
    Completed {
        usage: InferenceUsage,
        output: String,
        request_id: String,
        finish_reason: InferenceFinishReason,
    },
    Unknown {
        reason: InferenceAttentionReason,
        request_id: Option<String>,
    },
}

#[derive(Clone)]
pub(crate) struct Provider {
    client: reqwest::Client,
    pub(super) calls: Arc<Semaphore>,
    #[cfg(test)]
    endpoint: Option<String>,
}

impl Provider {
    pub(super) fn new() -> Result<Self, OperationError> {
        Self::build(true)
    }

    fn build(https_only: bool) -> Result<Self, OperationError> {
        let client = reqwest::Client::builder()
            .https_only(https_only)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(MAX_INFERENCE_SECONDS as u64))
            .pool_max_idle_per_host(0)
            .build()
            .map_err(|_| OperationError::Internal)?;
        Ok(Self {
            client,
            calls: Arc::new(Semaphore::new(MAX_INFERENCE_CALLS)),
            #[cfg(test)]
            endpoint: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn fixture(endpoint: String) -> Self {
        let mut provider = Self::build(false).unwrap();
        let url = reqwest::Url::parse(&endpoint).unwrap();
        assert!(
            url.host_str()
                .unwrap()
                .parse::<std::net::IpAddr>()
                .unwrap()
                .is_loopback()
        );
        provider.endpoint = Some(endpoint);
        provider
    }

    fn base(&self, endpoint: InferenceEndpoint) -> &str {
        #[cfg(test)]
        if let Some(endpoint) = &self.endpoint {
            return endpoint;
        }
        match endpoint {
            InferenceEndpoint::Global => "https://api.mistral.ai",
            InferenceEndpoint::Eu => "https://api.eu.mistral.ai",
            InferenceEndpoint::Us => "https://api.us.mistral.ai",
        }
    }

    async fn body(
        response: reqwest::Response,
        secret: &SecretString,
    ) -> Result<Zeroizing<Vec<u8>>, InferenceAttentionReason> {
        if !response.status().is_success() {
            return Err(InferenceAttentionReason::ProviderRejected);
        }
        if response
            .content_length()
            .is_some_and(|n| n > MAX_PROVIDER_BODY as u64)
        {
            return Err(InferenceAttentionReason::ResponseTooLarge);
        }
        let mut response = response;
        let mut body = Zeroizing::new(Vec::new());
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| InferenceAttentionReason::Interrupted)?
        {
            if chunk.len() > MAX_PROVIDER_BODY - body.len() {
                return Err(InferenceAttentionReason::ResponseTooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        // Provider diagnostics and even successful payloads can reflect credentials.
        if body
            .windows(secret.expose_secret().len())
            .any(|window| window == secret.expose_secret().as_bytes())
        {
            return Err(InferenceAttentionReason::InvalidResponse);
        }
        Ok(body)
    }

    pub(super) async fn verify(&self, permit: &VerificationPermit) -> bool {
        #[derive(Deserialize)]
        struct Model {
            id: String,
            max_context_length: u32,
            capabilities: Capabilities,
            #[serde(default)]
            archived: bool,
        }
        #[derive(Deserialize)]
        struct Capabilities {
            completion_chat: bool,
        }
        let policy = &permit.configuration.policy;
        let verification = async {
            let response = self
                .client
                .get(format!(
                    "{}/v1/models/{}",
                    self.base(policy.endpoint),
                    policy.model_id
                ))
                .bearer_auth(permit.secret.expose_secret())
                .send()
                .await
                .ok()?;
            let body = Self::body(response, &permit.secret).await.ok()?;
            let model: Model = serde_json::from_slice(&body).ok()?;
            Some(
                model.id == policy.model_id
                    && model.max_context_length == policy.context_tokens
                    && model.capabilities.completion_chat
                    && !model.archived,
            )
        };
        matches!(
            tokio::time::timeout(Duration::from_secs(10), verification).await,
            Ok(Some(true))
        )
    }

    pub(super) async fn execute(&self, permit: &CallPermit) -> ProviderOutcome {
        let request = &permit.view.request;
        let policy = &permit.view.configuration.policy;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let remaining = Duration::from_secs(request.deadline as u64)
            .saturating_sub(now)
            .min(Duration::from_secs(MAX_INFERENCE_SECONDS as u64));
        if remaining.is_zero() {
            return ProviderOutcome::NotSent {
                reason: InferenceNotSentReason::Deadline,
            };
        }
        let deadline = tokio::time::Instant::now() + remaining;
        let _capacity = match tokio::time::timeout_at(deadline, self.calls.acquire()).await {
            Ok(Ok(capacity)) => capacity,
            Ok(Err(_)) => {
                return ProviderOutcome::NotSent {
                    reason: InferenceNotSentReason::Shutdown,
                };
            }
            Err(_) => {
                return ProviderOutcome::NotSent {
                    reason: InferenceNotSentReason::Deadline,
                };
            }
        };
        if tokio::time::Instant::now() >= deadline {
            return ProviderOutcome::NotSent {
                reason: InferenceNotSentReason::Deadline,
            };
        }
        let mut request_id = None;
        let execution = async {
            let response = self.client.post(format!("{}/v1/chat/completions", self.base(policy.endpoint)))
                .bearer_auth(permit.secret.expose_secret())
                .json(&serde_json::json!({
                    "model": policy.model_id, "messages": [{"role":"user","content":request.prompt}],
                    "max_tokens":request.max_output_tokens, "n":1, "stream":false, "tool_choice":"none",
                    "service_tier":"standard_only"
                }))
                .send().await.map_err(|_| InferenceAttentionReason::Interrupted)?;
            request_id = response
                .headers()
                .get("x-request-id")
                .and_then(|id| id.to_str().ok())
                .filter(|id| valid_request_id(id) && !id.contains(permit.secret.expose_secret()))
                .map(str::to_owned);
            let body = Self::body(response, &permit.secret).await?;
            #[derive(Deserialize)]
            struct Identity {
                id: String,
            }
            if let Ok(identity) = serde_json::from_slice::<Identity>(&body)
                && valid_request_id(&identity.id)
                && !identity.id.contains(permit.secret.expose_secret())
            {
                request_id = Some(identity.id);
            }
            let outcome = parse_completion(&body, &permit.view)?;
            if matches!(&outcome, ProviderOutcome::Completed { output, request_id, .. }
                if output.contains(permit.secret.expose_secret()) || request_id.contains(permit.secret.expose_secret()))
            {
                return Err(InferenceAttentionReason::InvalidResponse);
            }
            Ok(outcome)
        };
        match tokio::time::timeout_at(deadline, execution).await {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(reason)) => ProviderOutcome::Unknown { reason, request_id },
            Err(_) => ProviderOutcome::Unknown {
                reason: InferenceAttentionReason::Interrupted,
                request_id,
            },
        }
    }
}

fn valid_request_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 200
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
}

fn parse_completion(
    body: &[u8],
    view: &InferenceView,
) -> Result<ProviderOutcome, InferenceAttentionReason> {
    use InferenceAttentionReason as Reason;
    #[derive(Deserialize)]
    struct Completion {
        id: String,
        model: String,
        choices: Vec<Choice>,
        usage: serde_json::Value,
    }
    #[derive(Deserialize)]
    struct Choice {
        index: u32,
        message: Message,
        finish_reason: InferenceFinishReason,
    }
    #[derive(Deserialize)]
    struct Message {
        role: String,
        content: String,
        #[serde(default)]
        tool_calls: Option<Vec<serde_json::Value>>,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ProviderUsage {
        prompt_tokens: u64,
        completion_tokens: u64,
        total_tokens: u64,
        #[serde(default)]
        service_tier: Option<String>,
    }
    let response: Completion = serde_json::from_slice(body).map_err(|_| Reason::InvalidResponse)?;
    if response.model != view.configuration.policy.model_id {
        return Err(Reason::UnexpectedModel);
    }
    let report: ProviderUsage =
        serde_json::from_value(response.usage).map_err(|_| Reason::InvalidUsage)?;
    if !matches!(report.service_tier.as_deref(), None | Some("standard")) {
        return Err(Reason::InvalidUsage);
    }
    let usage = InferenceUsage {
        prompt_tokens: report.prompt_tokens,
        completion_tokens: report.completion_tokens,
        total_tokens: report.total_tokens,
    };
    if usage.prompt_tokens == 0
        || usage.prompt_tokens.checked_add(usage.completion_tokens) != Some(usage.total_tokens)
        || usage.total_tokens > u64::from(view.configuration.policy.context_tokens)
        || usage.total_tokens > view.request.max_total_tokens
        || usage.completion_tokens > u64::from(view.request.max_output_tokens)
    {
        return Err(Reason::InvalidUsage);
    }
    if !valid_request_id(&response.id) || response.choices.len() != 1 {
        return Err(Reason::InvalidResponse);
    }
    let choice = response
        .choices
        .into_iter()
        .next()
        .ok_or(Reason::InvalidResponse)?;
    if choice.index != 0
        || choice.message.role != "assistant"
        || choice.message.content.len() > MAX_INFERENCE_OUTPUT_BYTES
        || choice
            .message
            .tool_calls
            .is_some_and(|tools| !tools.is_empty())
    {
        return Err(Reason::InvalidResponse);
    }
    Ok(ProviderOutcome::Completed {
        usage,
        output: choice.message.content,
        request_id: response.id,
        finish_reason: choice.finish_reason,
    })
}
