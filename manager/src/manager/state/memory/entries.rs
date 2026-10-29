use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum MemoryFile {
    Live {
        entry: MemoryEntry,
    },
    Tombstone {
        id: MemoryId,
        scope: MemoryScope,
        revision: u64,
    },
}

impl MemoryFile {
    pub(super) fn id(&self) -> MemoryId {
        match self {
            Self::Live { entry } => entry.id,
            Self::Tombstone { id, .. } => *id,
        }
    }
    pub(super) fn encode(&self) -> Result<Vec<u8>, OperationError> {
        let mut metadata = self.clone();
        let markdown = match &mut metadata {
            Self::Live { entry } => std::mem::take(&mut entry.markdown),
            Self::Tombstone { .. } => String::new(),
        };
        Ok(format!(
            "<!-- botstral-memory-entry {} -->\n{markdown}",
            json(&metadata)?
        )
        .into_bytes())
    }
    pub(super) fn decode(bytes: &[u8]) -> Result<Self, OperationError> {
        if bytes.len() > 16384 {
            return Err(OperationError::InvalidInput);
        }
        let text = std::str::from_utf8(bytes).map_err(|_| OperationError::InvalidInput)?;
        let (header, markdown) = text.split_once('\n').ok_or(OperationError::InvalidInput)?;
        let header = header
            .strip_prefix("<!-- botstral-memory-entry ")
            .and_then(|h| h.strip_suffix(" -->"))
            .ok_or(OperationError::InvalidInput)?;
        let mut file: Self = parse(header.into())?;
        match &mut file {
            Self::Live { entry } => {
                if !entry.markdown.is_empty() {
                    return Err(OperationError::InvalidInput);
                }
                entry.markdown = markdown.into();
                validate_entry(entry)?;
            }
            Self::Tombstone { revision, .. } => {
                if !markdown.is_empty() || !(1..=MAX_COUNTER).contains(revision) {
                    return Err(OperationError::InvalidInput);
                }
            }
        }
        Ok(file)
    }
}

pub(super) fn entry_filename(id: MemoryId) -> String {
    format!("entry-{id}.md")
}
pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn legacy_id(bot: BotId) -> MemoryId {
    let hash = Sha256::digest(format!("botstral-legacy-memory-v1:{bot}"));
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&hash[..16]);
    MemoryId(Uuid::from_bytes(bytes))
}

fn validate_entry(entry: &MemoryEntry) -> Result<(), OperationError> {
    if !(1..=MAX_COUNTER).contains(&entry.revision)
        || entry.markdown.trim().is_empty()
        || entry.markdown.len() > MAX_MEMORY_ENTRY_BYTES
    {
        return Err(OperationError::InvalidInput);
    }
    match &entry.source {
        MemorySource::Run { bot, .. } => {
            if matches!(entry.scope, MemoryScope::Individual { bot: owner } if owner != *bot) {
                return Err(OperationError::InvalidInput);
            }
        }
        MemorySource::Imported { sources } => {
            let MemoryScope::Individual { bot } = entry.scope else {
                return Err(OperationError::InvalidInput);
            };
            validate_document_shape(&Document {
                bot,
                kind: DocumentKind::Memory,
                revision: entry.revision,
                markdown: entry.markdown.clone(),
                sources: sources.clone(),
            })?;
        }
    }
    Ok(())
}

pub(in crate::manager::state) fn validate_snapshot(
    entries: &[MemoryEntry],
    bot: BotId,
) -> Result<(), OperationError> {
    let mut ids = std::collections::HashSet::new();
    for entry in entries {
        validate_entry(entry)?;
        if !ids.insert(entry.id)
            || matches!(entry.scope, MemoryScope::Individual { bot: owner } if owner != bot)
        {
            return Err(OperationError::InvalidInput);
        }
    }
    for scope in [MemoryScope::Shared, MemoryScope::Individual { bot }] {
        let selected: Vec<_> = entries.iter().filter(|e| e.scope == scope).collect();
        if selected.len() > MAX_MEMORY_ENTRIES_PER_SCOPE
            || json(&selected)?.len() > MAX_MEMORY_SCOPE_BYTES
        {
            return Err(OperationError::Busy);
        }
    }
    if json(&entries)?.len() > MAX_MEMORY_CONTEXT_BYTES {
        return Err(OperationError::Busy);
    }
    Ok(())
}

impl State {
    pub(in crate::manager::state) fn reject_known_secrets(
        &self,
        value: &impl Serialize,
    ) -> Result<(), OperationError> {
        let value = serde_json::to_value(value).map_err(|_| OperationError::Internal)?;
        let mut secrets = self.known_connector_secrets()?;
        match self.provider_secret() {
            Ok(secret) => secrets.push(secret),
            Err(OperationError::NotFound) => {}
            Err(error) => return Err(error),
        }
        if secrets
            .iter()
            .any(|secret| crate::manager::provider::reflects_secret(&value, secret.expose_secret()))
        {
            return Err(OperationError::InvalidInput);
        }
        Ok(())
    }

    pub(super) fn memory_file_bytes(
        &self,
        id: MemoryId,
    ) -> Result<Option<Vec<u8>>, OperationError> {
        let path = self.directory.join(entry_filename(id));
        match fs::symlink_metadata(&path) {
            Ok(_) => botstral_link::tls::private_bytes(&path)
                .map(|bytes| Some(bytes.to_vec()))
                .map_err(|_| OperationError::Internal),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(OperationError::Internal),
        }
    }

    fn live_memory_entries(&self) -> Result<Vec<MemoryEntry>, OperationError> {
        let mut entries = Vec::new();
        let mut files = 0;
        for item in fs::read_dir(&self.directory).map_err(|_| OperationError::Internal)? {
            let path = item.map_err(|_| OperationError::Internal)?.path();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or(OperationError::InvalidInput)?;
            if !name.starts_with("entry-") || !name.ends_with(".md") {
                continue;
            }
            files += 1;
            if files > MAX_RECORDS {
                return Err(OperationError::Busy);
            }
            let bytes =
                botstral_link::tls::private_bytes(&path).map_err(|_| OperationError::Internal)?;
            let file = MemoryFile::decode(&bytes)?;
            if name != entry_filename(file.id()) {
                return Err(OperationError::InvalidInput);
            }
            if let MemoryFile::Live { entry } = file {
                entries.push(entry);
            }
        }
        entries.sort_by_key(|entry| entry.id.to_string());
        Ok(entries)
    }

    pub(in crate::manager::state) fn memory_context(
        &mut self,
        bot: BotId,
    ) -> Result<Vec<MemoryEntry>, OperationError> {
        self.recover_memory_publications()?;
        self.import_legacy_memory()?;
        let entries: Vec<_> = self
            .live_memory_entries()?
            .into_iter()
            .filter(|e| {
                e.scope == MemoryScope::Shared || e.scope == MemoryScope::Individual { bot }
            })
            .collect();
        validate_snapshot(&entries, bot)?;
        self.reject_known_secrets(&entries)?;
        Ok(entries)
    }

    pub(in crate::manager::state) fn prepare_memory_change(
        &mut self,
        run: RunId,
        bot: BotId,
        turn: RunTurnId,
        proposal: &MemoryProposal,
    ) -> Result<(MemoryOutcome, Option<PreparedMemoryPublication>), OperationError> {
        self.reject_known_secrets(proposal)?;
        let mut entries = self.memory_context(bot)?;
        let scope = proposal.scope().resolve(bot);
        let source = MemorySource::Run { bot, run, turn };
        let (file, expected, outcome) = match proposal {
            MemoryProposal::List { .. } => {
                return Ok((
                    MemoryOutcome::Listed {
                        scope,
                        entries: entries.into_iter().filter(|e| e.scope == scope).collect(),
                    },
                    None,
                ));
            }
            MemoryProposal::Create { markdown, .. } => {
                self.require_memory_file_capacity()?;
                let entry = MemoryEntry {
                    id: MemoryId::new(),
                    scope,
                    revision: 1,
                    markdown: markdown.clone(),
                    source,
                };
                if self.memory_file_bytes(entry.id)?.is_some() {
                    return Err(OperationError::Conflict);
                }
                (
                    MemoryFile::Live {
                        entry: entry.clone(),
                    },
                    None,
                    MemoryOutcome::Created { entry },
                )
            }
            MemoryProposal::Update {
                id,
                expected_revision,
                ..
            }
            | MemoryProposal::Delete {
                id,
                expected_revision,
                ..
            } => {
                let prior = entries
                    .iter()
                    .find(|e| e.id == *id && e.scope == scope)
                    .ok_or(OperationError::NotFound)?;
                if prior.revision != *expected_revision || prior.revision == MAX_COUNTER {
                    return Err(OperationError::Conflict);
                }
                let revision = prior.revision + 1;
                let prior_bytes = self
                    .memory_file_bytes(*id)?
                    .ok_or(OperationError::Conflict)?;
                if !matches!(MemoryFile::decode(&prior_bytes)?, MemoryFile::Live { entry } if &entry == prior)
                {
                    return Err(OperationError::Conflict);
                }
                let expected = Some(digest(&prior_bytes));
                match proposal {
                    MemoryProposal::Update { markdown, .. } => {
                        let entry = MemoryEntry {
                            id: *id,
                            scope,
                            revision,
                            markdown: markdown.clone(),
                            source,
                        };
                        (
                            MemoryFile::Live {
                                entry: entry.clone(),
                            },
                            expected,
                            MemoryOutcome::Updated { entry },
                        )
                    }
                    _ => (
                        MemoryFile::Tombstone {
                            id: *id,
                            scope,
                            revision,
                        },
                        expected,
                        MemoryOutcome::Deleted {
                            id: *id,
                            scope,
                            revision,
                        },
                    ),
                }
            }
        };
        entries.retain(|e| e.id != file.id());
        if let MemoryFile::Live { entry } = &file {
            entries.push(entry.clone());
        }
        validate_snapshot(&entries, bot)?;
        let bytes = file.encode()?;
        self.reject_known_secrets(&file)?;
        Ok((
            outcome,
            Some(PreparedMemoryPublication {
                entry: file.id(),
                expected_digest: expected,
                intended_bytes: bytes,
            }),
        ))
    }

    fn import_legacy_memory(&mut self) -> Result<(), OperationError> {
        if !matches!(
            self.installation,
            Installation::Configured {
                vault: VaultState::Unlocked { .. }
            }
        ) {
            return Err(OperationError::Locked);
        }
        let pending = self.connection.prepare("SELECT bot,expected_digest,intended_bytes FROM memory_imports WHERE state='pending'")?
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<Vec<u8>>>(2)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        for (bot, expected, bytes) in pending {
            self.finish_memory_import(
                bot.parse().map_err(|_| OperationError::Internal)?,
                &expected,
                bytes.as_deref(),
            )?;
        }
        let mut documents = Vec::new();
        for item in fs::read_dir(&self.directory).map_err(|_| OperationError::Internal)? {
            let path = item.map_err(|_| OperationError::Internal)?.path();
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|name| name.starts_with("memory-") && name.ends_with(".md"))
            {
                if documents.len() >= MAX_RECORDS {
                    return Err(OperationError::Busy);
                }
                documents.push(self.read_document(&path)?);
            }
        }
        for document in documents {
            self.reject_known_secrets(&document)?;
            let exists: bool = self.connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM memory_imports WHERE bot=?1)",
                [document.bot.to_string()],
                |r| r.get(0),
            )?;
            if exists {
                return Err(OperationError::Conflict);
            }
            let bytes = if document.markdown.trim().is_empty() {
                None
            } else {
                Some(
                    MemoryFile::Live {
                        entry: MemoryEntry {
                            id: legacy_id(document.bot),
                            scope: MemoryScope::Individual { bot: document.bot },
                            revision: document.revision,
                            markdown: document.markdown.clone(),
                            source: MemorySource::Imported {
                                sources: document.sources.clone(),
                            },
                        },
                    }
                    .encode()?,
                )
            };
            let expected = digest(&encode(&document)?);
            if let Some(bytes) = &bytes {
                self.require_memory_file_capacity()?;
                let MemoryFile::Live { entry } = MemoryFile::decode(bytes)? else {
                    return Err(OperationError::Internal);
                };
                let mut context: Vec<_> = self
                    .live_memory_entries()?
                    .into_iter()
                    .filter(|e| e.scope == MemoryScope::Shared || e.scope == entry.scope)
                    .collect();
                context.push(entry);
                validate_snapshot(&context, document.bot)?;
            }
            self.connection.execute(
                "INSERT INTO memory_imports VALUES(?1,?2,?3,'pending')",
                params![document.bot.to_string(), expected, bytes],
            )?;
            self.fault("memory_import_committed");
            self.finish_memory_import(document.bot, &expected, bytes.as_deref())?;
        }
        Ok(())
    }

    fn require_memory_file_capacity(&self) -> Result<(), OperationError> {
        let mut count = 0;
        for item in fs::read_dir(&self.directory).map_err(|_| OperationError::Internal)? {
            let item = item.map_err(|_| OperationError::Internal)?;
            if item
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("entry-") && name.ends_with(".md"))
            {
                count += 1;
            }
        }
        if count >= MAX_RECORDS {
            Err(OperationError::Busy)
        } else {
            Ok(())
        }
    }

    fn finish_memory_import(
        &mut self,
        bot: BotId,
        expected: &str,
        bytes: Option<&[u8]>,
    ) -> Result<(), OperationError> {
        let old = self.directory.join(filename(bot, &DocumentKind::Memory));
        let archive = self.directory.join(format!("legacy-memory-{bot}.md"));
        let archived = archive.try_exists().map_err(|_| OperationError::Internal)?;
        let path = if archived { &archive } else { &old };
        let original =
            botstral_link::tls::private_bytes(path).map_err(|_| OperationError::Internal)?;
        if digest(&original) != expected
            || (archived && old.try_exists().map_err(|_| OperationError::Internal)?)
        {
            return Err(OperationError::Conflict);
        }
        let document = decode_document(&original)?;
        if document.bot != bot || document.kind != DocumentKind::Memory {
            return Err(OperationError::InvalidInput);
        }
        self.reject_known_secrets(&document)?;
        if let Some(bytes) = bytes {
            self.reject_known_secrets(&MemoryFile::decode(bytes)?)?;
            self.publish_memory_bytes(legacy_id(bot), None, bytes)?;
        }
        self.fault("memory_import_published");
        if !archived {
            fs::rename(old, archive).map_err(|_| OperationError::Internal)?;
            self.sync_memory_directory()?;
        }
        self.fault("memory_import_archived");
        self.connection.execute(
            "UPDATE memory_imports SET state='applied' WHERE bot=?1",
            [bot.to_string()],
        )?;
        Ok(())
    }
}
