use super::*;

pub const MAX_COMPUTER_SPEC: usize = 2048;
pub const MAX_OUTPUTS: usize = 4;
pub const MAX_ARTIFACT_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_EXECUTION_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_SPOOL_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_STORE_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_ARTIFACT_RECORDS: usize = 512;
pub const ARTIFACT_CHUNK_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ComputerProfile {
    LinuxPlaywrightV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComputerRuntime {
    pub sha256: CertificateFingerprint,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Viewport {
    pub width: u16,
    pub height: u16,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OutputDeclaration {
    pub slot: String,
    pub name: String,
    pub media_type: MediaType,
    pub max_bytes: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ComputerOperation {
    ProtectedLogin {
        item: CredentialId,
        revision: CredentialRevision,
        purpose: CredentialFields,
        plan: Box<LoginSitePlan>,
    },
    Navigate {
        url: String,
    },
    Click {
        selector: String,
    },
    Fill {
        selector: String,
        text: String,
    },
    CaptureText {
        selector: String,
        output: String,
    },
    Screenshot {
        output: String,
    },
    WriteFile {
        path: String,
        text: String,
    },
    ReadFile {
        path: String,
        output: String,
    },
    ExportFile {
        path: String,
        output: String,
    },
}
fn bounded(value: &str, max: usize) -> bool {
    value.len() <= max && !value.contains('\0')
}
pub fn valid_slot(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 24
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComputerSpec {
    pub profile: ComputerProfile,
    pub account: ProcessAccount,
    pub runtime: ComputerRuntime,
    pub deadline: i64,
    pub limits: ProcessLimits,
    pub viewport: Viewport,
    pub operations: Vec<ComputerOperation>,
    pub outputs: Vec<OutputDeclaration>,
}
impl ComputerSpec {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        let l = &self.limits;
        if self.account.uid == 0
            || self.deadline <= 0
            || !(1..=300).contains(&l.runtime_seconds)
            || !(1..=1024).contains(&l.output_bytes)
            || !(32..=128).contains(&l.processes)
            || !(256 * 1024 * 1024..=1536 * 1024 * 1024).contains(&l.memory_bytes)
            || !(320..=1280).contains(&self.viewport.width)
            || !(240..=960).contains(&self.viewport.height)
            || self.operations.is_empty()
            || self.operations.len() > 12
            || self.outputs.len() > MAX_OUTPUTS
            || serde_json::to_vec(self)
                .map_err(|_| ErrorCode::InvalidMessage)?
                .len()
                > MAX_COMPUTER_SPEC
        {
            return Err(ErrorCode::InvalidMessage);
        }
        let mut slots = std::collections::HashSet::new();
        let mut total = 0;
        for o in &self.outputs {
            if !valid_slot(&o.slot)
                || !slots.insert(&o.slot)
                || o.name.is_empty()
                || !bounded(&o.name, 80)
                || o.name
                    .chars()
                    .any(|c| !c.is_ascii() || c.is_control() || matches!(c, '/' | '\\' | '"'))
                || !(1..=MAX_ARTIFACT_BYTES).contains(&o.max_bytes)
            {
                return Err(ErrorCode::InvalidMessage);
            }
            total += o.max_bytes;
        }
        if total > MAX_EXECUTION_BYTES {
            return Err(ErrorCode::InvalidMessage);
        }
        let mut written = std::collections::HashSet::new();
        for op in &self.operations {
            let (selector, path, output) = match op {
                ComputerOperation::ProtectedLogin { plan, .. } => {
                    plan.validate()?;
                    if self.operations.len() != 1 || !self.outputs.is_empty() {
                        return Err(ErrorCode::InvalidMessage);
                    }
                    (None, None, None)
                }
                ComputerOperation::Navigate { url } => {
                    if !bounded(url, 512)
                        || !(url.starts_with("https://") || url.starts_with("http://"))
                        || url.chars().any(char::is_control)
                    {
                        return Err(ErrorCode::InvalidMessage);
                    }
                    (None, None, None)
                }
                ComputerOperation::Click { selector } => (Some(selector), None, None),
                ComputerOperation::Fill { selector, text } => {
                    if !bounded(text, 512) {
                        return Err(ErrorCode::InvalidMessage);
                    }
                    (Some(selector), None, None)
                }
                ComputerOperation::CaptureText { selector, output } => {
                    (Some(selector), None, Some(output))
                }
                ComputerOperation::Screenshot { output } => (None, None, Some(output)),
                ComputerOperation::WriteFile { path, text } => {
                    if !bounded(text, 512) {
                        return Err(ErrorCode::InvalidMessage);
                    }
                    (None, Some(path), None)
                }
                ComputerOperation::ReadFile { path, output }
                | ComputerOperation::ExportFile { path, output } => {
                    (None, Some(path), Some(output))
                }
            };
            if selector.is_some_and(|s| s.is_empty() || !bounded(s, 256))
                || path.is_some_and(|p| !p.starts_with('/') || !bounded(p, 512))
                || output.is_some_and(|o| !slots.contains(o) || !written.insert(o))
            {
                return Err(ErrorCode::InvalidMessage);
            }
        }
        Ok(())
    }
}
impl WorkSpec {
    pub fn supervised(&self) -> bool {
        matches!(
            self,
            Self::Process { .. }
                | Self::Computer { .. }
                | Self::Vibe { .. }
                | Self::Connector { .. }
        )
    }
    pub fn outputs(&self) -> &[OutputDeclaration] {
        match self {
            Self::Computer { spec } => &spec.outputs,
            Self::Vibe { spec } => &spec.policy.outputs,
            Self::Connector { spec } => std::slice::from_ref(&spec.result),
            _ => &[],
        }
    }
}
