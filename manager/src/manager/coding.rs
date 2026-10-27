use super::{provider::*, *};
use botstral_link::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CodingRequest {
    pub model: String,
    pub messages: Vec<Value>,
    #[serde(default)]
    pub tools: Vec<Value>,
    #[serde(default)]
    pub stream: bool,
    pub stream_options: Option<StreamOptions>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub tool_choice: Option<Value>,
    pub parallel_tool_calls: Option<bool>,
    pub n: Option<u32>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StreamOptions {
    pub include_usage: bool,
}
impl CodingRequest {
    pub fn prepare(&self, spec: &VibeSpec) -> Result<PreparedChat, OperationError> {
        let invalid = OperationError::InvalidInput;
        if self.model != spec.policy.model_id
            || self.messages.is_empty()
            || self.messages.len() > 128
            || self.tools.len() > VIBE_TOOLS.len()
            || self.n.is_some_and(|n| n != 1)
            || self
                .temperature
                .is_some_and(|n| !n.is_finite() || !(0.0..=1.0).contains(&n))
            || self
                .tool_choice
                .as_ref()
                .is_some_and(|v| !matches!(v.as_str(), Some("auto" | "none")))
        {
            return Err(invalid);
        }
        for m in &self.messages {
            let object = m.as_object().ok_or(invalid)?;
            if object.keys().any(|k| {
                ![
                    "role",
                    "content",
                    "tool_calls",
                    "tool_call_id",
                    "name",
                    "reasoning_content",
                ]
                .contains(&k.as_str())
            }) || !matches!(
                m["role"].as_str(),
                Some("system" | "user" | "assistant" | "tool")
            ) || !(m["content"].is_null() || m["content"].is_string())
                || object
                    .get("reasoning_content")
                    .is_some_and(|v| !v.is_string())
            {
                return Err(invalid);
            }
            if let Some(calls) = object.get("tool_calls") {
                for call in calls.as_array().filter(|a| a.len() <= 4).ok_or(invalid)? {
                    validate_tool_call(call)?;
                }
            }
        }
        let mut names = std::collections::HashSet::new();
        for tool in &self.tools {
            let name = tool["function"]["name"].as_str().ok_or(invalid)?;
            if tool["type"] != "function"
                || !VIBE_TOOLS.contains(&name)
                || !names.insert(name)
                || !tool["function"]["parameters"].is_object()
            {
                return Err(invalid);
            }
        }
        let mut body = json!({"model":self.model,"messages":self.messages,"stream":false,"n":1,
            "max_tokens":spec.limits.max_output_tokens,"service_tier":"standard_only"});
        if !self.tools.is_empty() {
            body["tools"] = json!(self.tools);
            body["tool_choice"] = json!("auto");
            body["parallel_tool_calls"] = json!(false);
        }
        if let Some(temperature) = self.temperature {
            body["temperature"] = json!(temperature);
        }
        if body.to_string().len() > MAX_CODING_REQUEST {
            return Err(invalid);
        }
        Ok(PreparedChat::coding(body))
    }
}
fn validate_tool_call(call: &Value) -> Result<(), OperationError> {
    if call["type"] != "function"
        || call["id"]
            .as_str()
            .is_none_or(|s| s.is_empty() || s.len() > 128)
        || call["function"]["name"]
            .as_str()
            .is_none_or(|s| !VIBE_TOOLS.contains(&s))
        || call["function"]["arguments"]
            .as_str()
            .is_none_or(|s| s.len() > 32768 || serde_json::from_str::<Value>(s).is_err())
    {
        return Err(OperationError::InvalidInput);
    }
    Ok(())
}
pub(super) fn completion(value: &Value) -> Result<(), OperationError> {
    let invalid = OperationError::InvalidInput;
    if value["choices"].as_array().is_none_or(|v| v.len() != 1)
        || value["choices"][0]["index"] != 0
        || value["choices"][0]["message"]["role"] != "assistant"
        || !matches!(
            value["choices"][0]["finish_reason"].as_str(),
            Some("stop" | "length" | "tool_calls")
        )
    {
        return Err(invalid);
    }
    let message = &value["choices"][0]["message"];
    if !(message["content"].is_null() || message["content"].is_string()) {
        return Err(invalid);
    }
    if let Some(calls) = message.get("tool_calls").filter(|v| !v.is_null()) {
        for call in calls.as_array().filter(|a| a.len() <= 4).ok_or(invalid)? {
            validate_tool_call(call)?;
        }
    }
    Ok(())
}
pub(super) struct CodingPermit {
    pub view: CodingCallView,
    pub configuration: InferenceConfiguration,
    pub spec: VibeSpec,
    pub prepared: PreparedChat,
    pub secret: SecretString,
}
pub(super) enum CodingAdmission {
    Call(Box<CodingPermit>),
    Cached(Value),
}
impl Manager {
    pub async fn coding_calls(
        &self,
        token: SessionToken,
        task: TaskId,
    ) -> Result<Vec<CodingCallView>, OperationError> {
        self.queue
            .call(false, move |s| s.coding_calls(token, task))
            .await
    }
    pub(crate) async fn coding_chat(
        &self,
        peer: crate::worker_http::VerifiedIdentity,
        task: TaskId,
        execution: ExecutionId,
        id: InferenceAttemptId,
        bytes: Vec<u8>,
    ) -> Result<(bool, Value), OperationError> {
        if bytes.len() > MAX_CODING_REQUEST {
            return Err(OperationError::InvalidInput);
        }
        let request: CodingRequest =
            serde_json::from_slice(&bytes).map_err(|_| OperationError::InvalidInput)?;
        let stream = request.stream;
        let capacity = self
            .provider
            .calls
            .clone()
            .try_acquire_owned()
            .map_err(|_| OperationError::Busy)?;
        let mut registered_calls = self.inference.coding_calls.lock().await;
        if self
            .inference
            .stopping
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(OperationError::Busy);
        }
        while let Some(result) = registered_calls.try_join_next() {
            result.map_err(|_| OperationError::Internal)?;
        }
        let queue = self.queue.clone();
        let provider = self.provider.clone();
        let mut stopped = self.inference.stop.subscribe();
        let (reply, receive) = tokio::sync::oneshot::channel();
        registered_calls.spawn(async move {
            let result = async {
                let admission = queue
                    .call_wait(move |s| s.admit_coding(peer, task, execution, id, request))
                    .await?;
                let permit = match admission {
                    CodingAdmission::Cached(value) => return Ok((stream, value)),
                    CodingAdmission::Call(permit) => permit,
                };
                let sent =
                    !reply.is_closed() && queue.call_wait(move |s| s.send_coding(id)).await?;
                let outcome = if sent {
                    if *stopped.borrow() {
                        EnvelopeOutcome::Unknown {
                            reason: InferenceAttentionReason::Shutdown,
                            request_id: None,
                        }
                    } else {
                        tokio::select! {
                            biased;
                            outcome = provider.envelope(
                                ModelRequest {
                                    policy: &permit.configuration.policy,
                                    deadline: permit.spec.limits.deadline,
                                    max_output_tokens: permit.spec.limits.max_output_tokens,
                                    max_total_tokens: permit.view.reservation.total_tokens,
                                    prepared: &permit.prepared,
                                },
                                &permit.secret,
                                Some(capacity),
                            ) => outcome,
                            _ = stopped.changed() => EnvelopeOutcome::Unknown {
                                reason: InferenceAttentionReason::Shutdown,
                                request_id: None,
                            },
                        }
                    }
                } else {
                    EnvelopeOutcome::NotSent {
                        reason: InferenceNotSentReason::ConfigurationChanged,
                    }
                };
                queue
                    .call_wait(move |s| s.finish_coding(id, outcome))
                    .await
                    .map(|v| (stream, v))
            }
            .await;
            let _ = reply.send(result);
        });
        drop(registered_calls);
        receive.await.map_err(|_| OperationError::Internal)?
    }
}
pub(crate) fn sse(value: Value) -> String {
    let mut message = value["choices"][0]["message"].clone();
    if let Some(calls) = message["tool_calls"].as_array_mut() {
        for (i, call) in calls.iter_mut().enumerate() {
            call["index"] = json!(i);
        }
    }
    let chunk = json!({"id":value["id"],"object":"chat.completion.chunk","created":value["created"],
        "model":value["model"],"choices":[{"index":0,"delta":message,"finish_reason":value["choices"][0]["finish_reason"]}]});
    let usage = json!({"id":value["id"],"object":"chat.completion.chunk","created":value["created"],
        "model":value["model"],"choices":[],"usage":value["usage"]});
    format!("data: {chunk}\n\ndata: {usage}\n\ndata: [DONE]\n\n")
}
