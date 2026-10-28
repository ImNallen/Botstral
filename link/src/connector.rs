use crate::*;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

pub const MCP_VERSION: &str = "2025-06-18";
pub const MAX_CONNECTIONS: usize = 32;
pub const MAX_CONNECTOR_TOOLS: usize = 32;
pub const MAX_CONNECTOR_BYTES: usize = 128 * 1024;
pub const MAX_CONNECTOR_RESULT: u32 = 8192;
pub const MAX_CONNECTOR_CALLS: u32 = 16;

identifiers!(ConnectionId, ConnectorCallId);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConnectionVersion {
    pub connection: ConnectionId,
    pub revision: u64,
    pub credential_epoch: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConnectorToolBinding {
    pub connection: ConnectionVersion,
    pub catalog_revision: u64,
    pub grant_revision: u64,
    pub name: String,
    pub schema_digest: CertificateFingerprint,
}
impl ConnectorToolBinding {
    pub fn public_name(&self) -> String {
        let digest = connector_digest(&serde_json::to_value(self).expect("binding serializes"));
        format!("connector_{}", &digest.as_str()[..40])
    }
    pub fn vibe_name(&self) -> String {
        format!("manager_{}", self.public_name())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConnectionTransport {
    RemoteHttp {
        endpoint: String,
    },
    WorkerStdio {
        worker: WorkerId,
        account: ProcessAccount,
        executable: String,
        arguments: Vec<String>,
        cwd: String,
        environment: BTreeMap<String, String>,
    },
}
impl ConnectionTransport {
    pub fn class(&self) -> &'static str {
        match self {
            Self::RemoteHttp { .. } => "remote_http",
            Self::WorkerStdio { .. } => "worker_stdio",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionAvailability {
    NeedsDiscovery,
    Ready,
    AuthenticationRequired,
    Unavailable,
    Revoked,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}
impl McpTool {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        if self.name.is_empty()
            || self.name.len() > 128
            || !self
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
            || self.description.len() > 2048
            || self.input_schema["type"] != "object"
            || !bounded_connector_json(&self.input_schema, 8192)
        {
            return Err(ErrorCode::InvalidMessage);
        }
        Ok(())
    }
    pub fn schema_digest(&self) -> CertificateFingerprint {
        connector_digest(&self.input_schema)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConnectorToolDefinition {
    pub binding: ConnectorToolBinding,
    pub description: String,
    pub input_schema: Value,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConnectionView {
    pub id: ConnectionId,
    pub label: String,
    pub version: ConnectionVersion,
    pub transport: ConnectionTransport,
    pub availability: ConnectionAvailability,
    pub catalog_revision: u64,
    pub tools: Vec<McpTool>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InstallConnection {
    pub id: ConnectionId,
    pub label: String,
    pub transport: ConnectionTransport,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BotConnectorGrants {
    pub bot: BotId,
    pub connection: ConnectionId,
    pub revision: u64,
    pub tools: Vec<ConnectorToolBinding>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetConnectorGrants {
    pub connection: ConnectionVersion,
    pub catalog_revision: u64,
    pub expected_revision: u64,
    pub tools: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConnectorRunner {
    pub runtime: CertificateFingerprint,
    pub account: ProcessAccount,
    pub limits: ProcessLimits,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConnectorToolPolicy {
    pub tools: Vec<ConnectorToolBinding>,
    pub max_calls: u32,
    pub max_result_bytes: u32,
    pub runner: ConnectorRunner,
}
impl ConnectorToolPolicy {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        self.runner.limits.validate()?;
        let names: HashSet<_> = self
            .tools
            .iter()
            .map(ConnectorToolBinding::public_name)
            .collect();
        if self.runner.account.uid == 0
            || self.tools.len() > MAX_CONNECTOR_TOOLS
            || names.len() != self.tools.len()
            || !(1..=MAX_CONNECTOR_CALLS).contains(&self.max_calls)
            || !(1..=MAX_CONNECTOR_RESULT).contains(&self.max_result_bytes)
            || self
                .tools
                .iter()
                .any(|t| t.grant_revision == 0 || t.catalog_revision == 0)
        {
            return Err(ErrorCode::InvalidMessage);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConnectorProposal {
    pub tool: ConnectorToolBinding,
    pub arguments: Value,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConnectorOperation {
    Discover { connection: ConnectionVersion },
    Call { proposal: ConnectorProposal },
}
impl ConnectorOperation {
    pub fn connection(&self) -> &ConnectionVersion {
        match self {
            Self::Discover { connection } => connection,
            Self::Call { proposal } => &proposal.tool.connection,
        }
    }
    pub fn digest(&self) -> CertificateFingerprint {
        connector_digest(&serde_json::to_value(self).expect("operation serializes"))
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConnectorSpec {
    pub operation: ConnectorOperation,
    pub runner: ConnectorRunner,
    pub deadline: i64,
    pub result: OutputDeclaration,
}
impl ConnectorSpec {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        self.runner.limits.validate()?;
        if self.runner.account.uid == 0
            || self.deadline <= 0
            || self.result.slot != "connector"
            || self.result.name != "connector.json"
            || self.result.media_type != MediaType::TextPlain
            || !(1..=u64::from(MAX_CONNECTOR_RESULT)).contains(&self.result.max_bytes)
            || serde_json::to_vec(self)
                .map_err(|_| ErrorCode::InvalidMessage)?
                .len()
                > MAX_COMPUTER_SPEC
        {
            return Err(ErrorCode::InvalidMessage);
        }
        if let ConnectorOperation::Call { proposal } = &self.operation
            && (!proposal.arguments.is_object()
                || !bounded_connector_json(&proposal.arguments, 8192))
        {
            return Err(ErrorCode::InvalidMessage);
        }
        Ok(())
    }
}
pub fn connector_output(max_bytes: u32) -> OutputDeclaration {
    OutputDeclaration {
        slot: "connector".into(),
        name: "connector.json".into(),
        media_type: MediaType::TextPlain,
        max_bytes: max_bytes.into(),
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiscoverConnection {
    pub task: TaskId,
    pub session: ComputerSessionId,
    pub expected: ConnectionVersion,
    pub runner: ConnectorRunner,
    pub deadline: i64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorFailure {
    Authority,
    Authentication,
    CatalogChanged,
    Protocol,
    Bound,
    Transport,
    Deadline,
    Restart,
    Interrupted,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConnectorResult {
    Catalog { tools: Vec<McpTool> },
    Tool { value: Value },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConnectorOutcome {
    NotSent { reason: ConnectorFailure },
    Finished { result: ConnectorResult },
    Unknown { reason: ConnectorFailure },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConnectorCallState {
    Reserved,
    DispatchPossible,
    Resolved { outcome: ConnectorOutcome },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConnectorCallView {
    pub id: ConnectorCallId,
    pub task: TaskId,
    pub execution: ExecutionId,
    pub connection: ConnectionVersion,
    pub tool: Option<ConnectorToolBinding>,
    pub argument_digest: CertificateFingerprint,
    pub state: ConnectorCallState,
    pub created_at: i64,
    pub revoked_during_call_at: Option<i64>,
    pub late_result: Option<ConnectorResult>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConnectorGatewayRequest {
    Catalog,
    Execute {
        id: ConnectorCallId,
        operation: ConnectorOperation,
    },
    Status {
        id: ConnectorCallId,
    },
    Settle {
        id: ConnectorCallId,
        outcome: ConnectorOutcome,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConnectorAudit {
    pub sequence: u64,
    pub connection: ConnectionId,
    pub call: Option<ConnectorCallId>,
    pub code: String,
    pub digest: Option<CertificateFingerprint>,
    pub at: i64,
}
pub fn bounded_connector_json(value: &Value, max: usize) -> bool {
    fn depth(v: &Value, left: usize) -> bool {
        left != 0
            && match v {
                Value::Array(a) => a.len() <= 256 && a.iter().all(|v| depth(v, left - 1)),
                Value::Object(o) => o.len() <= 256 && o.values().all(|v| depth(v, left - 1)),
                _ => true,
            }
    }
    depth(value, 24) && serde_json::to_vec(value).is_ok_and(|v| v.len() <= max)
}

#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct ConnectorSecretEntry(pub String);
impl Drop for ConnectorSecretEntry {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.0.zeroize();
    }
}
#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConnectionSecretInput {
    None,
    Bearer {
        value: ConnectorSecretEntry,
    },
    LocalEnvironment {
        values: BTreeMap<String, ConnectorSecretEntry>,
    },
}
#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplaceConnectionSecret {
    pub expected_revision: u64,
    pub secret: ConnectionSecretInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConnectionSummary {
    pub id: ConnectionId,
    pub label: String,
    pub version: ConnectionVersion,
    pub availability: ConnectionAvailability,
    pub catalog_revision: u64,
    pub tool_count: u32,
}
impl From<ConnectionView> for ConnectionSummary {
    fn from(view: ConnectionView) -> Self {
        Self {
            id: view.id,
            label: view.label,
            version: view.version,
            availability: view.availability,
            catalog_revision: view.catalog_revision,
            tool_count: view.tools.len() as u32,
        }
    }
}

/// Hash JSON values with explicit types, lengths, sorted keys and IEEE-754 floats.
pub fn connector_digest(value: &Value) -> CertificateFingerprint {
    fn length(hash: &mut Sha256, n: usize) {
        hash.update((n as u64).to_be_bytes());
    }
    fn add(hash: &mut Sha256, value: &Value) {
        match value {
            Value::Null => hash.update(b"n"),
            Value::Bool(v) => hash.update(if *v { b"t" } else { b"f" }),
            Value::String(v) => {
                hash.update(b"s");
                length(hash, v.len());
                hash.update(v.as_bytes());
            }
            Value::Number(n) if n.is_i64() || n.is_u64() => {
                let v = n.to_string();
                hash.update(b"i");
                length(hash, v.len());
                hash.update(v.as_bytes());
            }
            Value::Number(n) => {
                hash.update(b"d");
                hash.update(n.as_f64().expect("JSON number").to_be_bytes());
            }
            Value::Array(values) => {
                hash.update(b"a");
                length(hash, values.len());
                for v in values {
                    add(hash, v);
                }
            }
            Value::Object(values) => {
                hash.update(b"o");
                length(hash, values.len());
                for (k, v) in values.iter().collect::<BTreeMap<_, _>>() {
                    add(hash, &Value::String(k.clone()));
                    add(hash, v);
                }
            }
        }
    }
    let mut hash = Sha256::new();
    add(&mut hash, value);
    format!("{:x}", hash.finalize())
        .try_into()
        .expect("SHA-256 digest")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worker_and_manager_share_canonical_digest_vectors() {
        let vectors: Value =
            serde_json::from_str(include_str!("../schema/connector-digests.json")).unwrap();
        for vector in vectors.as_array().unwrap() {
            assert_eq!(
                connector_digest(&vector["value"]).as_str(),
                vector["digest"].as_str().unwrap()
            );
        }
    }
}
