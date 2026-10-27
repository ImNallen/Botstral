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

    pub(super) async fn verify(&self, permit: &VerificationPermit) -> Option<bool> {
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
            #[serde(default)]
            function_calling: bool,
        }
        let policy = &permit.configuration.policy;
        let secret = &permit.secret;
        let verification = async {
            let response = self
                .client
                .get(format!(
                    "{}/v1/models/{}",
                    self.base(policy.endpoint),
                    policy.model_id
                ))
                .bearer_auth(secret.expose_secret())
                .send()
                .await
                .ok()?;
            let body = Self::body(response, secret).await.ok()?;
            let model: Model = serde_json::from_slice(&body).ok()?;
            (model.id == policy.model_id
                && model.max_context_length == policy.context_tokens
                && model.capabilities.completion_chat
                && !model.archived)
                .then_some(model.capabilities.function_calling)
        };
        tokio::time::timeout(Duration::from_secs(10), verification)
            .await
            .ok()
            .flatten()
    }

    pub(super) async fn execute(&self, permit: &CallPermit) -> ProviderOutcome {
        let input = &permit.view.request;
        let messages = [RunMessage::User {
            content: input.prompt.clone(),
        }];
        match self
            .chat(
                ModelRequest {
                    policy: &permit.view.configuration.policy,
                    deadline: input.deadline,
                    max_output_tokens: input.max_output_tokens,
                    max_total_tokens: input.max_total_tokens,
                    messages: &messages,
                    tools: None,
                },
                &permit.secret,
                None,
            )
            .await
        {
            ModelOutcome::NotSent { reason } => ProviderOutcome::NotSent { reason },
            ModelOutcome::Unknown { reason, request_id } => {
                ProviderOutcome::Unknown { reason, request_id }
            }
            ModelOutcome::Billed {
                usage,
                request_id,
                response,
                finish_reason,
            } => match (response, finish_reason) {
                (
                    Ok(RunMessage::Assistant {
                        content,
                        tool_call: None,
                    }),
                    Some(finish_reason),
                ) => ProviderOutcome::Completed {
                    usage,
                    request_id,
                    output: content,
                    finish_reason,
                },
                _ => ProviderOutcome::Unknown {
                    reason: InferenceAttentionReason::InvalidResponse,
                    request_id: Some(request_id),
                },
            },
        }
    }

    pub(super) async fn chat(
        &self,
        request: ModelRequest<'_>,
        secret: &SecretString,
        capacity: Option<tokio::sync::OwnedSemaphorePermit>,
    ) -> ModelOutcome {
        let policy = request.policy;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let remaining = Duration::from_secs(request.deadline as u64)
            .saturating_sub(now)
            .min(Duration::from_secs(MAX_INFERENCE_SECONDS as u64));
        if remaining.is_zero() {
            return ModelOutcome::NotSent {
                reason: InferenceNotSentReason::Deadline,
            };
        }
        let deadline = tokio::time::Instant::now() + remaining;
        let _capacity = match capacity {
            Some(capacity) => capacity,
            None => {
                match tokio::time::timeout_at(deadline, self.calls.clone().acquire_owned()).await {
                    Ok(Ok(capacity)) => capacity,
                    Ok(Err(_)) => {
                        return ModelOutcome::NotSent {
                            reason: InferenceNotSentReason::Shutdown,
                        };
                    }
                    Err(_) => {
                        return ModelOutcome::NotSent {
                            reason: InferenceNotSentReason::Deadline,
                        };
                    }
                }
            }
        };
        if tokio::time::Instant::now() >= deadline {
            return ModelOutcome::NotSent {
                reason: InferenceNotSentReason::Deadline,
            };
        }
        let mut request_id = None;
        let execution = async {
            let mut body = serde_json::json!({
                "model":policy.model_id, "messages":request.messages.iter().map(message).collect::<Vec<_>>(),
                "max_tokens":request.max_output_tokens, "n":1, "stream":false,
                "tool_choice": if request.tools.is_some() { "auto" } else { "none" },
                "service_tier":"standard_only"
            });
            if let Some(tools) = request.tools {
                body["tools"] = serde_json::json!(super::runs::tools::definitions(tools));
                body["parallel_tool_calls"] = false.into();
            }
            if reflects_secret(&body, secret.expose_secret()) {
                return Err(InferenceAttentionReason::InvalidResponse);
            }
            let response = self
                .client
                .post(format!(
                    "{}/v1/chat/completions",
                    self.base(policy.endpoint)
                ))
                .bearer_auth(secret.expose_secret())
                .json(&body)
                .send()
                .await
                .map_err(|_| InferenceAttentionReason::Interrupted)?;
            request_id = response
                .headers()
                .get("x-request-id")
                .and_then(|id| id.to_str().ok())
                .filter(|id| valid_request_id(id) && !id.contains(secret.expose_secret()))
                .map(str::to_owned);
            let body = Self::body(response, secret).await?;
            #[derive(Deserialize)]
            struct Identity {
                id: String,
            }
            if let Ok(identity) = serde_json::from_slice::<Identity>(&body)
                && valid_request_id(&identity.id)
                && !identity.id.contains(secret.expose_secret())
            {
                request_id = Some(identity.id);
            }
            let decoded: serde_json::Value = serde_json::from_slice(&body)
                .map_err(|_| InferenceAttentionReason::InvalidResponse)?;
            if reflects_secret(&decoded, secret.expose_secret()) {
                return Err(InferenceAttentionReason::InvalidResponse);
            }
            let outcome = parse_completion(decoded, &request)?;
            Ok(outcome)
        };
        match tokio::time::timeout_at(deadline, execution).await {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(reason)) => ModelOutcome::Unknown { reason, request_id },
            Err(_) => ModelOutcome::Unknown {
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

pub(super) struct ModelRequest<'a> {
    pub policy: &'a InferencePolicy,
    pub deadline: i64,
    pub max_output_tokens: u32,
    pub max_total_tokens: u64,
    pub messages: &'a [RunMessage],
    pub tools: Option<&'a RunExecutionPolicy>,
}

pub(super) enum ModelOutcome {
    NotSent {
        reason: InferenceNotSentReason,
    },
    Billed {
        usage: InferenceUsage,
        request_id: String,
        response: Result<RunMessage, InferenceAttentionReason>,
        finish_reason: Option<InferenceFinishReason>,
    },
    Unknown {
        reason: InferenceAttentionReason,
        request_id: Option<String>,
    },
}

fn message(message: &RunMessage) -> serde_json::Value {
    use serde_json::json;
    match message {
        RunMessage::User { content } => json!({"role":"user","content":content}),
        RunMessage::Assistant { content, tool_call } => {
            let mut message = json!({"role":"assistant","content":content});
            if let Some(call) = tool_call {
                message["tool_calls"] = json!([{"id":call.id,"type":"function","function":super::runs::tools::function(&call.proposal)}]);
            }
            message
        }
        RunMessage::Tool {
            content,
            tool_call_id,
        } => json!({"role":"tool","content":content,"tool_call_id":tool_call_id}),
    }
}

pub(super) fn reflects_secret(value: &serde_json::Value, secret: &str) -> bool {
    match value {
        serde_json::Value::String(value) => {
            value.contains(secret)
                || serde_json::from_str::<serde_json::Value>(value)
                    .is_ok_and(|v| reflects_secret(&v, secret))
        }
        serde_json::Value::Array(values) => values.iter().any(|v| reflects_secret(v, secret)),
        serde_json::Value::Object(values) => values
            .iter()
            .any(|(k, v)| k.contains(secret) || reflects_secret(v, secret)),
        _ => false,
    }
}

fn parse_completion(
    value: serde_json::Value,
    request: &ModelRequest<'_>,
) -> Result<ModelOutcome, InferenceAttentionReason> {
    use InferenceAttentionReason as Reason;
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Usage {
        prompt_tokens: u64,
        completion_tokens: u64,
        total_tokens: u64,
        #[serde(default)]
        service_tier: Option<String>,
    }
    if value["model"].as_str() != Some(&request.policy.model_id) {
        return Err(Reason::UnexpectedModel);
    }
    let report: Usage =
        serde_json::from_value(value["usage"].clone()).map_err(|_| Reason::InvalidUsage)?;
    if !matches!(report.service_tier.as_deref(), None | Some("standard"))
        || report.prompt_tokens == 0
        || report.prompt_tokens.checked_add(report.completion_tokens) != Some(report.total_tokens)
        || report.total_tokens > u64::from(request.policy.context_tokens)
        || report.total_tokens > request.max_total_tokens
        || report.completion_tokens > u64::from(request.max_output_tokens)
    {
        return Err(Reason::InvalidUsage);
    }
    let id = value["id"]
        .as_str()
        .filter(|id| valid_request_id(id))
        .ok_or(Reason::InvalidResponse)?;
    let finish_reason = match value["choices"][0]["finish_reason"].as_str() {
        Some("stop") => Some(InferenceFinishReason::Stop),
        Some("length") => Some(InferenceFinishReason::Length),
        _ => None,
    };
    let response = parse_assistant(&value["choices"], request.tools.is_some());
    Ok(ModelOutcome::Billed {
        usage: InferenceUsage {
            prompt_tokens: report.prompt_tokens,
            completion_tokens: report.completion_tokens,
            total_tokens: report.total_tokens,
        },
        request_id: id.into(),
        response,
        finish_reason,
    })
}

fn parse_assistant(
    choices: &serde_json::Value,
    tools: bool,
) -> Result<RunMessage, InferenceAttentionReason> {
    use InferenceAttentionReason::InvalidResponse as Invalid;
    if choices.as_array().is_none_or(|choices| choices.len() != 1) {
        return Err(Invalid);
    }
    let choice = &choices[0];
    let message = &choice["message"];
    if choice["index"].as_u64() != Some(0) || message["role"] != "assistant" {
        return Err(Invalid);
    }
    let content = match &message["content"] {
        serde_json::Value::String(content) => content.clone(),
        serde_json::Value::Null if tools => String::new(),
        _ => return Err(Invalid),
    };
    if content.len()
        > if tools {
            MAX_RUN_TEXT_BYTES
        } else {
            MAX_INFERENCE_OUTPUT_BYTES
        }
    {
        return Err(Invalid);
    }
    let calls = match &message["tool_calls"] {
        serde_json::Value::Null => &[][..],
        serde_json::Value::Array(calls) => calls.as_slice(),
        _ => return Err(Invalid),
    };
    let tool_call = match calls {
        [] if matches!(choice["finish_reason"].as_str(), Some("stop" | "length")) => None,
        [call] if tools && choice["finish_reason"] == "tool_calls" => {
            let id = call["id"]
                .as_str()
                .filter(|id| {
                    !id.is_empty()
                        && id.len() <= 64
                        && id
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                })
                .ok_or(Invalid)?;
            if call["type"] != "function" {
                return Err(Invalid);
            }
            let function = &call["function"];
            let proposal = super::runs::tools::decode(
                function["name"].as_str().ok_or(Invalid)?,
                function["arguments"].as_str().ok_or(Invalid)?,
            )
            .map_err(|_| Invalid)?;
            Some(RunToolCall {
                id: id.into(),
                proposal,
            })
        }
        _ => return Err(Invalid),
    };
    Ok(RunMessage::Assistant { content, tool_call })
}
