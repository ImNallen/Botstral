use super::*;
use futures_util::StreamExt;
use reqwest::{Client, Response};
use serde_json::{Value, json};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct Session<'a> {
    client: Client,
    endpoint: &'a str,
    bearer: Option<&'a str>,
    session: Option<String>,
    initialized: bool,
    changed: bool,
}
impl Session<'_> {
    fn post(&self, value: &Value) -> reqwest::RequestBuilder {
        let mut request = self
            .client
            .post(self.endpoint)
            .header("Accept", "application/json, text/event-stream")
            .json(value);
        if let Some(bearer) = self.bearer {
            request = request.bearer_auth(bearer);
        }
        if self.initialized {
            request = request.header("MCP-Protocol-Version", MCP_VERSION);
        }
        if let Some(id) = &self.session {
            request = request.header("Mcp-Session-Id", id);
        }
        request
    }
    async fn send(&self, value: &Value) -> Result<Response, ConnectorFailure> {
        let response = self
            .post(value)
            .send()
            .await
            .map_err(|_| ConnectorFailure::Transport)?;
        if matches!(response.status().as_u16(), 401 | 403) {
            return Err(ConnectorFailure::Authentication);
        }
        if !response.status().is_success() {
            return Err(ConnectorFailure::Transport);
        }
        Ok(response)
    }
    async fn notification(&self, value: Value) -> Result<(), ConnectorFailure> {
        let response = self.send(&value).await?;
        if response.status().as_u16() != 202 {
            return Err(ConnectorFailure::Protocol);
        }
        Ok(())
    }
    async fn message(&mut self, value: Value, id: &str) -> Result<Option<Value>, ConnectorFailure> {
        if value["jsonrpc"] != "2.0" || !value.is_object() {
            return Err(ConnectorFailure::Protocol);
        }
        if let Some(method) = value.get("method") {
            if method == "notifications/tools/list_changed" {
                self.changed = true;
            }
            if let Some(request) = value.get("id") {
                if !(request.is_string() || request.is_i64()) {
                    return Err(ConnectorFailure::Protocol);
                }
                let response = if method == "ping" {
                    json!({"jsonrpc":"2.0","id":request,"result":{}})
                } else {
                    json!({"jsonrpc":"2.0","id":request,"error":{"code":-32601,"message":"Unsupported client capability"}})
                };
                self.notification(response).await?;
            }
            return Ok(None);
        }
        if value["id"] != id || value.get("result").is_some() == value.get("error").is_some() {
            return Err(ConnectorFailure::Protocol);
        }
        Ok(Some(value))
    }
    async fn rpc(
        &mut self,
        id: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, ConnectorFailure> {
        let response = self
            .send(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await?;
        if response.status().as_u16() != 200 {
            return Err(ConnectorFailure::Protocol);
        }
        if !self.initialized {
            self.session = response
                .headers()
                .get("mcp-session-id")
                .map(|s| {
                    s.to_str()
                        .map(str::to_owned)
                        .map_err(|_| ConnectorFailure::Protocol)
                })
                .transpose()?;
            if self.session.as_ref().is_some_and(|s| {
                s.is_empty() || s.len() > 256 || !s.bytes().all(|b| b.is_ascii_graphic())
            }) {
                return Err(ConnectorFailure::Protocol);
            }
        }
        let sse = match response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.split(';').next().unwrap().trim())
        {
            Some("application/json") => false,
            Some("text/event-stream") => true,
            _ => return Err(ConnectorFailure::Protocol),
        };
        let mut stream = response.bytes_stream();
        let mut buffer = Vec::new();
        let mut total = 0;
        let mut messages = 0;
        let mut data = String::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| ConnectorFailure::Transport)?;
            total += chunk.len();
            if total > MAX_CONNECTOR_BYTES {
                return Err(ConnectorFailure::Bound);
            }
            buffer.extend_from_slice(&chunk);
            if sse {
                while let Some(end) = buffer.iter().position(|b| *b == b'\n') {
                    let line = buffer.drain(..=end).collect::<Vec<_>>();
                    let line = std::str::from_utf8(&line)
                        .map_err(|_| ConnectorFailure::Protocol)?
                        .trim_end_matches(['\r', '\n']);
                    if line.is_empty() && !data.is_empty() {
                        messages += 1;
                        if messages > 64 {
                            return Err(ConnectorFailure::Bound);
                        }
                        let value: Value =
                            serde_json::from_str(&data).map_err(|_| ConnectorFailure::Protocol)?;
                        data.clear();
                        if let Some(response) = self.message(value, id).await? {
                            return Ok(response);
                        }
                    } else if let Some(value) = line.strip_prefix("data:") {
                        if !data.is_empty() {
                            data.push('\n');
                        }
                        data.push_str(value.strip_prefix(' ').unwrap_or(value));
                    }
                }
            }
        }
        if sse {
            return Err(ConnectorFailure::Transport);
        }
        let value = serde_json::from_slice(&buffer).map_err(|_| ConnectorFailure::Protocol)?;
        self.message(value, id)
            .await?
            .ok_or(ConnectorFailure::Protocol)
    }
    async fn initialize(&mut self) -> Result<(), ConnectorFailure> {
        let response = self.rpc("initialize","initialize",json!({"protocolVersion":MCP_VERSION,"capabilities":{},"clientInfo":{"name":"botstral","version":"1"}})).await?;
        if response.get("error").is_some()
            || response["result"]["protocolVersion"] != MCP_VERSION
            || !response["result"]["capabilities"]["tools"].is_object()
        {
            return Err(ConnectorFailure::Protocol);
        }
        self.initialized = true;
        self.notification(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await
    }
    async fn tools(&mut self) -> Result<Vec<McpTool>, ConnectorFailure> {
        let mut tools = vec![];
        let mut cursor = None;
        let mut cursors = std::collections::HashSet::new();
        for page in 0..8 {
            let response = self
                .rpc(
                    &format!("list_{page}"),
                    "tools/list",
                    cursor.as_ref().map_or(json!({}), |s| json!({"cursor":s})),
                )
                .await?;
            let result = &response["result"];
            let list = result["tools"]
                .as_array()
                .ok_or(ConnectorFailure::Protocol)?;
            for tool in list {
                tools.push(McpTool {
                    name: tool["name"]
                        .as_str()
                        .ok_or(ConnectorFailure::Protocol)?
                        .into(),
                    description: tool
                        .get("description")
                        .map(|s| {
                            s.as_str()
                                .map(str::to_owned)
                                .ok_or(ConnectorFailure::Protocol)
                        })
                        .transpose()?
                        .unwrap_or_default(),
                    input_schema: tool["inputSchema"].clone(),
                });
            }
            validate_catalog(&tools).map_err(|_| ConnectorFailure::Protocol)?;
            match result.get("nextCursor") {
                None => return Ok(tools),
                Some(Value::String(s))
                    if !s.is_empty() && s.len() <= 512 && cursors.insert(s.clone()) =>
                {
                    cursor = Some(s.clone())
                }
                _ => return Err(ConnectorFailure::Protocol),
            }
        }
        Err(ConnectorFailure::Bound)
    }
}
pub(super) async fn execute(
    dispatch: &ConnectorDispatch,
    before_call: impl std::future::Future<Output = bool>,
) -> ConnectorOutcome {
    let client = Client::builder()
        .https_only(true)
        .no_proxy()
        .retry(reqwest::retry::never())
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(60))
        .build();
    let Ok(client) = client else {
        return ConnectorOutcome::NotSent {
            reason: ConnectorFailure::Transport,
        };
    };
    execute_with_client(dispatch, client, before_call).await
}
async fn execute_with_client(
    dispatch: &ConnectorDispatch,
    client: Client,
    before_call: impl std::future::Future<Output = bool>,
) -> ConnectorOutcome {
    let ConnectionTransport::RemoteHttp { endpoint } = &dispatch.connection.transport else {
        unreachable!()
    };
    let mut session = Session {
        client,
        endpoint,
        bearer: match &dispatch.secret {
            ConnectionSecretInput::Bearer { value } => Some(&value.0),
            _ => None,
        },
        session: None,
        initialized: false,
        changed: false,
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let until = Duration::from_secs(dispatch.deadline.saturating_sub(now).clamp(0, 60) as u64);
    let mut sent = false;
    let operation = async {
        session.initialize().await?;
        let tools = session.tools().await?;
        match &dispatch.operation {
            ConnectorOperation::Discover { .. } => Ok(ConnectorResult::Catalog { tools }),
            ConnectorOperation::Call { proposal } => {
                let tool = tools
                    .iter()
                    .find(|t| {
                        t.name == proposal.tool.name
                            && t.schema_digest() == proposal.tool.schema_digest
                    })
                    .ok_or(ConnectorFailure::CatalogChanged)?;
                if session.changed {
                    return Err(ConnectorFailure::CatalogChanged);
                }
                validate_arguments(&tool.input_schema, &proposal.arguments)
                    .map_err(|_| ConnectorFailure::Protocol)?;
                if !before_call.await {
                    return Err(ConnectorFailure::Authority);
                }
                sent = true;
                let response = session
                    .rpc(
                        &dispatch.id.to_string(),
                        "tools/call",
                        json!({"name":proposal.tool.name,"arguments":proposal.arguments}),
                    )
                    .await?;
                let result = if response.get("error").is_some() {
                    json!({"isError":true,"content":[{"type":"text","text":"Connector returned a protocol error."}]})
                } else {
                    response["result"].clone()
                };
                if !bounded_connector_json(&result, dispatch.max_result_bytes as usize)
                    || !result["content"].is_array()
                {
                    return Err(ConnectorFailure::Bound);
                }
                Ok(ConnectorResult::Tool { value: result })
            }
        }
    };
    let result = tokio::time::timeout(until, operation)
        .await
        .unwrap_or(Err(ConnectorFailure::Deadline));
    match result {
        Ok(result) => ConnectorOutcome::Finished { result },
        Err(reason) if sent => ConnectorOutcome::Unknown { reason },
        Err(reason) => ConnectorOutcome::NotSent { reason },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        extract::State as WebState,
        http::{HeaderMap, StatusCode},
        response::{IntoResponse, Response as WebResponse},
        routing::post,
    };
    use std::sync::{Arc, Mutex};
    #[derive(Clone)]
    struct Fixture {
        mode: &'static str,
        calls: Arc<Mutex<Vec<Value>>>,
    }
    async fn serve(
        WebState(f): WebState<Fixture>,
        headers: HeaderMap,
        Json(value): Json<Value>,
    ) -> WebResponse {
        assert_eq!(headers["authorization"], "Bearer synthetic-private-token");
        assert!(
            headers["accept"]
                .to_str()
                .unwrap()
                .contains("text/event-stream")
        );
        f.calls.lock().unwrap().push(value.clone());
        let method = value["method"].as_str().unwrap_or("");
        if method != "initialize" {
            assert_eq!(headers["mcp-protocol-version"], MCP_VERSION);
            assert_eq!(headers["mcp-session-id"], "fixture-session");
        }
        let result = match method {
            "initialize" => {
                json!({"protocolVersion":if f.mode=="wrong_version" {"2024-11-05"} else {MCP_VERSION},"capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}})
            }
            "notifications/initialized" | "" => return StatusCode::ACCEPTED.into_response(),
            "tools/list" => {
                json!({"tools":[{"name":"echo","description":"Synthetic echo","inputSchema":if f.mode=="changed" {json!({"type":"object","properties":{"new":{"type":"number"}}})} else {schema()}}]})
            }
            "tools/call" if f.mode == "lost" => {
                return (
                    StatusCode::OK,
                    [("content-type", "application/json")],
                    "truncated",
                )
                    .into_response();
            }
            "tools/call" if f.mode == "auth" => return StatusCode::UNAUTHORIZED.into_response(),
            "tools/call" => {
                json!({"content":[{"type":"text","text":"synthetic-private-token echo"}]})
            }
            _ => panic!("unexpected method"),
        };
        let response = json!({"jsonrpc":"2.0","id":value["id"],"result":result});
        if f.mode == "sse" {
            let callback = if method == "tools/call" {
                "data: {\"jsonrpc\":\"2.0\",\"id\":\"server-id\",\"method\":\"sampling/createMessage\",\"params\":{}}\r\n\r\n"
            } else {
                ""
            };
            (
                StatusCode::OK,
                [
                    ("content-type", "text/event-stream"),
                    ("mcp-session-id", "fixture-session"),
                ],
                format!(": bounded comment\r\n\r\n{callback}data: {response}\r\n\r\n"),
            )
                .into_response()
        } else {
            ([("mcp-session-id", "fixture-session")], Json(response)).into_response()
        }
    }
    fn schema() -> Value {
        json!({"type":"object","properties":{"value":{"type":"string"}},"required":["value"]})
    }
    #[tokio::test]
    async fn json_and_sse_negotiate_correlate_and_refuse_server_sampling() {
        for mode in [
            "json",
            "sse",
            "wrong_version",
            "changed",
            "lost",
            "auth",
            "revoked",
        ] {
            let fixture = Fixture {
                mode,
                calls: Arc::new(Mutex::new(vec![])),
            };
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}/mcp", listener.local_addr().unwrap());
            let app = Router::new()
                .route("/mcp", post(serve))
                .with_state(fixture.clone());
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let version = ConnectionVersion {
                connection: ConnectionId::new(),
                revision: 2,
                credential_epoch: 1,
            };
            let tool = McpTool {
                name: "echo".into(),
                description: "Synthetic echo".into(),
                input_schema: schema(),
            };
            let dispatch = ConnectorDispatch {
                id: ConnectorCallId::new(),
                connection: ConnectionView {
                    id: version.connection,
                    label: "Fixture".into(),
                    version: version.clone(),
                    transport: ConnectionTransport::RemoteHttp { endpoint },
                    availability: ConnectionAvailability::Ready,
                    catalog_revision: 1,
                    tools: vec![tool.clone()],
                },
                operation: ConnectorOperation::Call {
                    proposal: ConnectorProposal {
                        tool: ConnectorToolBinding {
                            connection: version,
                            catalog_revision: 1,
                            grant_revision: 1,
                            name: tool.name.clone(),
                            schema_digest: tool.schema_digest(),
                        },
                        arguments: json!({"value":"hello"}),
                    },
                },
                secret: ConnectionSecretInput::Bearer {
                    value: ConnectorSecretEntry("synthetic-private-token".into()),
                },
                deadline: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64
                    + 10,
                max_result_bytes: 8192,
            };
            let result = execute_with_client(
                &dispatch,
                Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .unwrap(),
                async { mode != "revoked" },
            )
            .await;
            match mode {
                "json" | "sse" => assert!(
                    matches!(result, ConnectorOutcome::Finished { .. }),
                    "{mode}: {result:?}"
                ),
                "wrong_version" => assert!(matches!(
                    result,
                    ConnectorOutcome::NotSent {
                        reason: ConnectorFailure::Protocol
                    }
                )),
                "revoked" => assert!(matches!(
                    result,
                    ConnectorOutcome::NotSent {
                        reason: ConnectorFailure::Authority
                    }
                )),
                "changed" => assert!(matches!(
                    result,
                    ConnectorOutcome::NotSent {
                        reason: ConnectorFailure::CatalogChanged
                    }
                )),
                "lost" | "auth" => assert!(matches!(result, ConnectorOutcome::Unknown { .. })),
                _ => unreachable!(),
            }
            let calls = fixture.calls.lock().unwrap();
            assert_eq!(
                calls.iter().filter(|v| v["method"] == "tools/call").count(),
                usize::from(!matches!(mode, "wrong_version" | "changed" | "revoked"))
            );
            assert_eq!(calls[0]["params"]["protocolVersion"], MCP_VERSION);
            if mode == "sse" {
                assert!(
                    calls
                        .iter()
                        .any(|v| v["id"] == "server-id" && v["error"]["code"] == -32601)
                );
            }
            server.abort();
        }
    }
}
