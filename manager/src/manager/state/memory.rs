use super::tasks::{event, json, parse};
use super::*;
use botstral_link::*;
use std::{io::Write, os::unix::fs::OpenOptionsExt};

fn filename(bot: BotId, kind: &DocumentKind) -> String {
    format!(
        "{}-{bot}.md",
        match kind {
            DocumentKind::Instructions => "instructions",
            DocumentKind::Memory => "memory",
        }
    )
}
fn encode(document: &Document) -> Result<Vec<u8>, OperationError> {
    let mut metadata = document.clone();
    metadata.markdown.clear();
    Ok(format!(
        "<!-- botstral-document {} -->\n{}",
        json(&metadata)?,
        document.markdown
    )
    .into_bytes())
}
impl State {
    fn read_document(&self, path: &std::path::Path) -> Result<Document, OperationError> {
        let bytes =
            botstral_link::tls::private_bytes(path).map_err(|_| OperationError::Internal)?;
        if bytes.len() > 16384 {
            return Err(OperationError::InvalidInput);
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| OperationError::InvalidInput)?;
        let (metadata, markdown) = text.split_once('\n').ok_or(OperationError::InvalidInput)?;
        let header = metadata
            .strip_prefix("<!-- botstral-document ")
            .and_then(|s| s.strip_suffix(" -->"))
            .ok_or(OperationError::InvalidInput)?;
        let mut document: Document = parse(header.into())?;
        if !document.markdown.is_empty()
            || document.revision == 0
            || document.revision > MAX_COUNTER
            || path.file_name().and_then(|n| n.to_str())
                != Some(filename(document.bot, &document.kind).as_str())
        {
            return Err(OperationError::InvalidInput);
        }
        document.markdown = markdown.into();
        self.validate_document(&document)?;
        Ok(document)
    }
    fn validate_document(&self, document: &Document) -> Result<(), OperationError> {
        self.find_bot(document.bot)?
            .ok_or(OperationError::NotFound)?;
        if document.markdown.len() > 4096
            || document.sources.is_empty()
            || document.sources.len() > 8
        {
            return Err(OperationError::InvalidInput);
        }
        for source in &document.sources {
            let valid = match source {
                SourceReference::Task { id } => self.load_task(*id)?.command.bot == document.bot,
                SourceReference::Message { id } => {
                    let bot: Option<String> = self.connection.query_row("SELECT conversations.bot FROM messages JOIN conversations ON conversations.id=messages.conversation WHERE messages.id=?1", [id.to_string()], |r| r.get(0)).optional()?;
                    bot == Some(document.bot.to_string())
                }
                SourceReference::External { reference } => {
                    reference.starts_with("https://")
                        && reference.len() <= 512
                        && !reference
                            .chars()
                            .any(|c| c.is_whitespace() || c.is_control())
                        && reference.len() > 8
                }
            };
            if !valid {
                return Err(OperationError::InvalidInput);
            }
        }
        Ok(())
    }
    pub(in crate::manager) fn reconcile_documents(
        &mut self,
    ) -> Result<Vec<Document>, OperationError> {
        let mut documents = Vec::new();
        for entry in fs::read_dir(&self.directory).map_err(|_| OperationError::Internal)? {
            let path = entry.map_err(|_| OperationError::Internal)?.path();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or(OperationError::InvalidInput)?;
            if (name.starts_with("instructions-") || name.starts_with("memory-"))
                && name.ends_with(".md")
            {
                documents.push(self.read_document(&path)?);
            }
        }
        if documents.len() > MAX_RECORDS {
            return Err(OperationError::Busy);
        }
        documents.sort_by_key(|d| filename(d.bot, &d.kind));
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for document in &documents {
            let name = filename(document.bot, &document.kind);
            let digest = format!(
                "{:x}",
                <sha2::Sha256 as sha2::Digest>::digest(encode(document)?)
            );
            let prior: Option<(u64, String)> = tx
                .query_row(
                    "SELECT revision,digest FROM document_publications WHERE name=?1",
                    [&name],
                    |r| Ok((r.get::<_, i64>(0)? as u64, r.get(1)?)),
                )
                .optional()?;
            if let Some((revision, old_digest)) = &prior
                && (*revision > document.revision
                    || (*revision == document.revision && *old_digest != digest))
            {
                return Err(OperationError::Conflict);
            }
            if prior != Some((document.revision, digest.clone())) {
                tx.execute("INSERT INTO document_publications VALUES(?1,?2,?3) ON CONFLICT(name) DO UPDATE SET revision=excluded.revision,digest=excluded.digest", params![name,document.revision as i64,digest])?;
                event(&tx, "document_published", name)?;
            }
        }
        let published = tx
            .prepare("SELECT name FROM document_publications")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        for name in published {
            if !documents.iter().any(|d| filename(d.bot, &d.kind) == name) {
                tx.execute("DELETE FROM document_publications WHERE name=?1", [&name])?;
                event(&tx, "document_removed", &name)?;
            }
        }
        tx.commit()?;
        let path = self.directory.join("memory-index.sqlite3");
        if path.exists() {
            botstral_link::tls::check_private_file(&path).map_err(|_| OperationError::Internal)?;
        }
        let mut index = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        // The index has no authority. Replacing a damaged cache never changes source files.
        if index.execute_batch("CREATE TABLE IF NOT EXISTS documents(name TEXT PRIMARY KEY, body TEXT NOT NULL) STRICT;").is_err() {
            drop(index);
            fs::remove_file(&path).map_err(|_| OperationError::Internal)?;
            index = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_NOFOLLOW)?;
            index.execute_batch("CREATE TABLE documents(name TEXT PRIMARY KEY, body TEXT NOT NULL) STRICT;")?;
        }
        let tx = index.transaction()?;
        tx.execute("DELETE FROM documents", [])?;
        for document in &documents {
            tx.execute(
                "INSERT INTO documents VALUES(?1,?2)",
                params![filename(document.bot, &document.kind), json(document)?],
            )?;
        }
        tx.commit()?;
        Ok(documents)
    }
    pub(in crate::manager) fn write_document(
        &mut self,
        token: SessionToken,
        input: WriteDocument,
    ) -> Result<Document, OperationError> {
        self.authorize(&token)?;
        let documents = self.reconcile_documents()?;
        let previous = documents
            .iter()
            .find(|d| d.bot == input.bot && d.kind == input.kind);
        let revision = previous.map_or(0, |d| d.revision);
        if let Some(previous) = previous
            && revision == input.expected_revision.saturating_add(1)
            && previous.markdown == input.markdown
            && previous.sources == input.sources
        {
            return Ok(previous.clone());
        }
        if revision != input.expected_revision
            || revision >= MAX_COUNTER
            || (previous.is_none() && documents.len() >= MAX_RECORDS)
        {
            return Err(OperationError::Conflict);
        }
        let document = Document {
            bot: input.bot,
            kind: input.kind,
            revision: revision + 1,
            sources: input.sources,
            markdown: input.markdown,
        };
        self.validate_document(&document)?;
        let name = filename(document.bot, &document.kind);
        let temporary = self.directory.join(format!(".{name}.tmp"));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temporary)
            .map_err(|_| OperationError::Internal)?;
        botstral_link::tls::private_bytes(&temporary).map_err(|_| OperationError::Internal)?;
        file.set_len(0).map_err(|_| OperationError::Internal)?;
        file.write_all(&encode(&document)?)
            .map_err(|_| OperationError::Internal)?;
        file.sync_all().map_err(|_| OperationError::Internal)?;
        self.fault("document_before_rename");
        fs::rename(temporary, self.directory.join(name)).map_err(|_| OperationError::Internal)?;
        fs::File::open(&self.directory)
            .and_then(|d| d.sync_all())
            .map_err(|_| OperationError::Internal)?;
        self.fault("document_after_rename");
        self.reconcile_documents()?;
        self.fault("document_after_index");
        Ok(document)
    }
    pub(in crate::manager) fn search_documents(
        &mut self,
        token: SessionToken,
        bot: BotId,
        query: String,
    ) -> Result<Vec<Document>, OperationError> {
        self.authorize(&token)?;
        if query.is_empty() || query.len() > 256 {
            return Err(OperationError::InvalidInput);
        }
        self.reconcile_documents()?;
        let index = Connection::open_with_flags(
            self.directory.join("memory-index.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        let mut result = Vec::new();
        for row in index
            .prepare("SELECT body FROM documents ORDER BY name")?
            .query_map([], |r| r.get::<_, String>(0))?
        {
            let document: Document = parse(row?)?;
            if document.bot == bot && document.markdown.contains(&query) {
                result.push(document);
            }
        }
        Ok(result)
    }
    pub(in crate::manager) fn rebuild_documents(
        &mut self,
        token: SessionToken,
    ) -> Result<Vec<Document>, OperationError> {
        self.authorize(&token)?;
        self.reconcile_documents()
    }
    pub(in crate::manager) fn fault(&self, point: &str) {
        #[cfg(all(feature = "nonproduction-test-worker", debug_assertions))]
        if std::env::var("BOTSTRAL_FAULT").as_deref() == Ok(point) {
            let path = self.directory.join(format!("fault-{point}.reached"));
            let mut file = fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .mode(0o600)
                .open(path)
                .expect("fault marker");
            file.write_all(point.as_bytes()).expect("fault marker");
            file.sync_all().expect("fault marker sync");
            std::process::abort();
        }
        let _ = point;
    }
}
