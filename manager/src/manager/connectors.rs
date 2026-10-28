use super::*;
use botstral_link::*;
use serde::Serialize;
use zeroize::Zeroizing;

mod http;

pub(super) struct ConnectorDispatch {
    pub id: ConnectorCallId,
    pub connection: ConnectionView,
    pub operation: ConnectorOperation,
    pub secret: ConnectionSecretInput,
    pub deadline: i64,
    pub max_result_bytes: u32,
}
pub(super) enum ConnectorAdmission {
    Existing(Box<ConnectorCallView>),
    Reserved(ConnectorCallId),
}
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ConnectorGatewayReply {
    Catalog {
        tools: Vec<ConnectorToolDefinition>,
    },
    Status {
        call: ConnectorCallView,
    },
    Local {
        id: ConnectorCallId,
        transport: ConnectionTransport,
        operation: ConnectorOperation,
        secret: ConnectionSecretInput,
        deadline: i64,
        max_result_bytes: u32,
    },
}

impl Manager {
    pub async fn install_connection(
        &self,
        owner: SessionToken,
        input: InstallConnection,
    ) -> Result<ConnectionView, OperationError> {
        self.queue
            .call(false, move |s| s.install_connection(owner, input))
            .await
    }
    pub async fn connections(
        &self,
        owner: SessionToken,
    ) -> Result<Vec<ConnectionView>, OperationError> {
        self.queue.call(false, move |s| s.connections(owner)).await
    }
    pub async fn connection(
        &self,
        owner: SessionToken,
        id: ConnectionId,
    ) -> Result<ConnectionView, OperationError> {
        self.queue
            .call(false, move |s| s.connection_view(owner, id))
            .await
    }
    pub async fn replace_connection_secret(
        &self,
        owner: SessionToken,
        id: ConnectionId,
        input: ReplaceConnectionSecret,
    ) -> Result<ConnectionView, OperationError> {
        self.queue
            .call(false, move |s| {
                s.replace_connection_secret(owner, id, input)
            })
            .await
    }
    pub async fn reconnect_connection(
        &self,
        owner: SessionToken,
        id: ConnectionId,
        expected: u64,
    ) -> Result<ConnectionView, OperationError> {
        self.queue
            .call(false, move |s| {
                s.change_connection(owner, id, expected, false)
            })
            .await
    }
    pub async fn revoke_connection(
        &self,
        owner: SessionToken,
        id: ConnectionId,
        expected: u64,
    ) -> Result<ConnectionView, OperationError> {
        self.queue
            .call(false, move |s| {
                s.change_connection(owner, id, expected, true)
            })
            .await
    }
    pub async fn discover_connection(
        &self,
        owner: SessionToken,
        input: DiscoverConnection,
    ) -> Result<TaskView, OperationError> {
        self.queue
            .call(false, move |s| s.discover_connection(owner, input))
            .await
    }
    pub async fn connector_grants(
        &self,
        owner: SessionToken,
        bot: BotId,
        connection: ConnectionId,
    ) -> Result<BotConnectorGrants, OperationError> {
        self.queue
            .call(false, move |s| s.connector_grants(owner, bot, connection))
            .await
    }
    pub async fn set_connector_grants(
        &self,
        owner: SessionToken,
        bot: BotId,
        input: SetConnectorGrants,
    ) -> Result<BotConnectorGrants, OperationError> {
        self.queue
            .call(false, move |s| s.set_connector_grants(owner, bot, input))
            .await
    }
    pub async fn connector_tools(
        &self,
        owner: SessionToken,
        bot: BotId,
    ) -> Result<Vec<ConnectorToolDefinition>, OperationError> {
        self.queue
            .call(false, move |s| s.connector_tools(owner, bot))
            .await
    }
    pub async fn connector_calls(
        &self,
        owner: SessionToken,
        task: TaskId,
    ) -> Result<Vec<ConnectorCallView>, OperationError> {
        self.queue
            .call(false, move |s| s.connector_calls(owner, task))
            .await
    }
    pub async fn connector_audit(
        &self,
        owner: SessionToken,
        id: ConnectionId,
        after: u64,
    ) -> Result<Vec<ConnectorAudit>, OperationError> {
        self.queue
            .call(false, move |s| s.connector_audit(owner, id, after))
            .await
    }
    pub(crate) async fn connector_request(
        &self,
        peer: crate::worker_http::VerifiedIdentity,
        task: TaskId,
        execution: ExecutionId,
        request: ConnectorGatewayRequest,
    ) -> Result<ConnectorGatewayReply, OperationError> {
        let capacity = self
            .connector_capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| OperationError::Busy)?;
        let mut registered = self.inference.coding_calls.lock().await;
        if self
            .inference
            .stopping
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(OperationError::Busy);
        }
        while let Some(result) = registered.try_join_next() {
            result.map_err(|_| OperationError::Internal)?;
        }
        let queue = self.queue.clone();
        let mut stopped = self.inference.stop.subscribe();
        let (reply, receive) = oneshot::channel();
        registered.spawn(async move {
            let _capacity = capacity;
            let result = async {
                match request {
                    ConnectorGatewayRequest::Catalog => queue.call_wait(move |s| s.connector_gateway_catalog(peer, task, execution)).await
                        .map(|tools| ConnectorGatewayReply::Catalog { tools }),
                    ConnectorGatewayRequest::Status { id } => queue.call_wait(move |s| s.connector_status(peer, task, execution, id)).await
                        .map(|call| ConnectorGatewayReply::Status { call }),
                    ConnectorGatewayRequest::Settle { id, outcome } => queue.call_wait(move |s| s.settle_local_connector(peer, task, execution, id, outcome)).await
                        .map(|call| ConnectorGatewayReply::Status { call }),
                    ConnectorGatewayRequest::Execute { id, operation } => {
                        let admission = queue.call_wait(move |s| s.admit_connector(peer, task, execution, id, operation)).await?;
                        if let ConnectorAdmission::Existing(call) = admission { return Ok(ConnectorGatewayReply::Status { call: *call }); }
                        let ConnectorAdmission::Reserved(id) = admission else { unreachable!() };
                        let dispatch = queue.call_wait(move |s| s.dispatch_connector(id)).await?;
                        let Some(dispatch) = dispatch else {
                            return queue.call_wait(move |s| s.load_connector_call(id)).await.map(|call| ConnectorGatewayReply::Status { call });
                        };
                        match &dispatch.connection.transport {
                            ConnectionTransport::WorkerStdio { .. } => Ok(ConnectorGatewayReply::Local {
                                id, transport: dispatch.connection.transport, operation: dispatch.operation,
                                secret: dispatch.secret, deadline: dispatch.deadline, max_result_bytes: dispatch.max_result_bytes,
                            }),
                            ConnectionTransport::RemoteHttp { .. } => {
                                let monitor = async {
                                    let mut tick = tokio::time::interval(std::time::Duration::from_millis(200));
                                    loop {
                                        tick.tick().await;
                                        if !queue.call_wait(move |s| s.connector_still_authorized(id)).await.unwrap_or(false) { break; }
                                    }
                                };
                                let outcome = tokio::select! {
                                    biased;
                                    _ = stopped.changed() => ConnectorOutcome::Unknown { reason: ConnectorFailure::Interrupted },
                                    _ = monitor => ConnectorOutcome::Unknown { reason: ConnectorFailure::Authority },
                                    result = http::execute(&dispatch, async { queue.call_wait(move |s| s.connector_still_authorized(id)).await.unwrap_or(false) }) => result,
                                };
                                queue.call_wait(move |s| s.finish_connector(id, outcome)).await.map(|call| ConnectorGatewayReply::Status { call })
                            }
                        }
                    }
                }
            }.await;
            let _ = reply.send(result);
        });
        drop(registered);
        receive.await.map_err(|_| OperationError::Internal)?
    }
}

fn bounded_schema(schema: &serde_json::Value) -> bool {
    fn visit(
        value: &serde_json::Value,
        root: &serde_json::Value,
        left: &mut usize,
        depth: usize,
    ) -> bool {
        if *left == 0 || depth == 0 {
            return false;
        }
        *left -= 1;
        match value {
            serde_json::Value::Object(object) => {
                if [
                    "$id",
                    "$anchor",
                    "$dynamicAnchor",
                    "$dynamicRef",
                    "$recursiveRef",
                    "$recursiveAnchor",
                ]
                .iter()
                .any(|key| object.contains_key(*key))
                {
                    return false;
                }
                if let Some(reference) = object.get("$ref") {
                    let Some(pointer) = reference.as_str().and_then(|s| s.strip_prefix('#')) else {
                        return false;
                    };
                    if !pointer.starts_with('/')
                        || !root
                            .pointer(pointer)
                            .is_some_and(|target| visit(target, root, left, depth - 1))
                    {
                        return false;
                    }
                }
                object
                    .values()
                    .all(|value| visit(value, root, left, depth - 1))
            }
            serde_json::Value::Array(array) => array
                .iter()
                .all(|value| visit(value, root, left, depth - 1)),
            _ => true,
        }
    }
    bounded_connector_json(schema, 8192) && visit(schema, schema, &mut 1024, 64)
}

pub(super) fn validate_catalog(tools: &[McpTool]) -> Result<(), OperationError> {
    let mut names = std::collections::HashSet::new();
    if tools.len() > MAX_CONNECTOR_TOOLS
        || serde_json::to_vec(tools)
            .map_err(|_| OperationError::Internal)?
            .len()
            > 32768
    {
        return Err(OperationError::InvalidInput);
    }
    for tool in tools {
        tool.validate()?;
        if !bounded_schema(&tool.input_schema) {
            return Err(OperationError::InvalidInput);
        }
        if !names.insert(&tool.name) {
            return Err(OperationError::InvalidInput);
        }
        jsonschema::options()
            .offline()
            .with_pattern_options(jsonschema::PatternOptions::fancy_regex().backtrack_limit(20000))
            .build(&tool.input_schema)
            .map_err(|_| OperationError::InvalidInput)?;
    }
    Ok(())
}
pub(super) fn validate_arguments(
    schema: &serde_json::Value,
    arguments: &serde_json::Value,
) -> Result<(), OperationError> {
    if !bounded_schema(schema)
        || !arguments.is_object()
        || !bounded_connector_json(arguments, 8192)
        || !jsonschema::options()
            .offline()
            .with_pattern_options(jsonschema::PatternOptions::fancy_regex().backtrack_limit(20000))
            .build(schema)
            .map_err(|_| OperationError::InvalidInput)?
            .is_valid(arguments)
    {
        return Err(OperationError::InvalidInput);
    }
    Ok(())
}
pub(super) fn secret_values(secret: &ConnectionSecretInput) -> Vec<&str> {
    match secret {
        ConnectionSecretInput::None => vec![],
        ConnectionSecretInput::Bearer { value } => vec![&value.0],
        ConnectionSecretInput::LocalEnvironment { values } => {
            values.values().map(|s| s.0.as_str()).collect()
        }
    }
}
pub(super) fn redact_result(result: &mut ConnectorResult, secrets: &[&str]) {
    match result {
        ConnectorResult::Catalog { tools } => {
            for tool in tools {
                redact_text(&mut tool.name, secrets);
                redact_text(&mut tool.description, secrets);
                redact(&mut tool.input_schema, secrets);
            }
        }
        ConnectorResult::Tool { value } => redact(value, secrets),
    }
}
fn redact_text(value: &mut String, secrets: &[&str]) {
    for secret in secrets {
        if !secret.is_empty() {
            *value = value.replace(secret, "[redacted]");
        }
    }
}
fn redact(value: &mut serde_json::Value, secrets: &[&str]) {
    match value {
        serde_json::Value::String(s) => redact_text(s, secrets),
        serde_json::Value::Array(a) => {
            for v in a {
                redact(v, secrets);
            }
        }
        serde_json::Value::Object(o) => {
            let old = std::mem::take(o);
            for (k, mut v) in old {
                let mut key = serde_json::Value::String(k);
                redact(&mut key, secrets);
                redact(&mut v, secrets);
                o.insert(key.as_str().unwrap().into(), v);
            }
        }
        _ => {}
    }
}
pub(super) fn secret_bytes(
    secret: &ConnectionSecretInput,
) -> Result<Zeroizing<Vec<u8>>, OperationError> {
    let bytes = Zeroizing::new(serde_json::to_vec(secret).map_err(|_| OperationError::Internal)?);
    if bytes.len() > 16384 {
        return Err(OperationError::InvalidInput);
    }
    Ok(bytes)
}

#[cfg(test)]
pub(in crate::manager) async fn execute_fixture(dispatch: &ConnectorDispatch) -> ConnectorOutcome {
    http::execute_with_client(
        dispatch,
        reqwest::Client::builder().no_proxy().build().unwrap(),
        async { true },
    )
    .await
}
