use crate::*;

pub const MAX_MEMORY_ENTRY_BYTES: usize = 4096;
pub const MAX_MEMORY_ENTRIES_PER_SCOPE: usize = 16;
pub const MAX_MEMORY_SCOPE_BYTES: usize = 8 * 1024;
pub const MAX_MEMORY_CONTEXT_BYTES: usize = 2 * MAX_MEMORY_SCOPE_BYTES;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryScopeSelection {
    Individual,
    Shared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum MemoryScope {
    Individual { bot: BotId },
    Shared,
}

impl MemoryScopeSelection {
    pub fn resolve(self, bot: BotId) -> MemoryScope {
        match self {
            Self::Individual => MemoryScope::Individual { bot },
            Self::Shared => MemoryScope::Shared,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum MemorySource {
    Run {
        bot: BotId,
        run: RunId,
        turn: RunTurnId,
    },
    Imported {
        sources: Vec<SourceReference>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryEntry {
    pub id: MemoryId,
    pub scope: MemoryScope,
    pub revision: u64,
    pub markdown: String,
    pub source: MemorySource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum MemoryProposal {
    List {
        scope: MemoryScopeSelection,
    },
    Create {
        scope: MemoryScopeSelection,
        markdown: String,
    },
    Update {
        scope: MemoryScopeSelection,
        id: MemoryId,
        expected_revision: u64,
        markdown: String,
    },
    Delete {
        scope: MemoryScopeSelection,
        id: MemoryId,
        expected_revision: u64,
    },
}

impl MemoryProposal {
    pub fn scope(&self) -> MemoryScopeSelection {
        match self {
            Self::List { scope }
            | Self::Create { scope, .. }
            | Self::Update { scope, .. }
            | Self::Delete { scope, .. } => *scope,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryRefusal {
    InvalidInput,
    NotFound,
    RevisionConflict,
    Capacity,
    ProtectedContent,
    NeedsAttention,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum MemoryOutcome {
    Listed {
        scope: MemoryScope,
        entries: Vec<MemoryEntry>,
    },
    Created {
        entry: MemoryEntry,
    },
    Updated {
        entry: MemoryEntry,
    },
    Deleted {
        id: MemoryId,
        scope: MemoryScope,
        revision: u64,
    },
    Refused {
        reason: MemoryRefusal,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VaultPurpose {
    ProviderKey,
    ReusableLogin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestVaultEntry {
    pub purpose: VaultPurpose,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum VaultOutcome {
    ProviderKeySetup,
    ReusableLoginUnsupported,
    LoginEntry { bot: BotId },
}

impl VaultOutcome {
    pub fn notice(&self) -> &'static str {
        match self {
            Self::ProviderKeySetup => {
                "Set the Mistral provider key through protected local administration with the Vault unlocked. Do not send the key in chat. No secret was saved by this handoff."
            }
            Self::LoginEntry { .. } => {
                "Open protected native login entry on your paired desktop. Do not send login secrets in chat. No secret was saved by this handoff."
            }
            Self::ReusableLoginUnsupported => {
                "Reusable login-item storage is not supported yet. Do not send login secrets in chat. No secret was saved."
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ManagerOutcome {
    Memory { outcome: MemoryOutcome },
    Vault { outcome: VaultOutcome },
}
