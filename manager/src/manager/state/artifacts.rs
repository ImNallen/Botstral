use super::tasks::{json, parse};
use super::*;
use botstral_link::*;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::Path,
};

fn storage_error(_: std::io::Error) -> OperationError {
    OperationError::Internal
}
fn directory(root: &Path) -> PathBuf {
    root.join("artifacts")
}
fn path(root: &Path, id: ArtifactId, ready: bool) -> PathBuf {
    directory(root).join(format!("{id}.{}", if ready { "blob" } else { "part" }))
}
fn open(path: &Path, write: bool) -> Result<fs::File, OperationError> {
    let f = fs::OpenOptions::new()
        .read(true)
        .write(write)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(storage_error)?;
    if !f.metadata().map_err(storage_error)?.is_file() {
        return Err(OperationError::Internal);
    }
    Ok(f)
}
fn digest(file: &mut fs::File, content: &ArtifactContent) -> Result<(), OperationError> {
    if file.metadata().map_err(storage_error)?.len() != content.length {
        return Err(OperationError::Conflict);
    }
    file.seek(SeekFrom::Start(0)).map_err(storage_error)?;
    let mut h = Sha256::new();
    let mut buffer = [0; 16384];
    let mut total = 0;
    loop {
        let n = file.read(&mut buffer).map_err(storage_error)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > content.length {
            return Err(OperationError::Conflict);
        }
        h.update(&buffer[..n]);
    }
    if total != content.length || format!("{:x}", h.finalize()) != content.sha256.as_str() {
        return Err(OperationError::Conflict);
    }
    Ok(())
}
impl State {
    pub(super) fn reconcile_artifacts(&mut self) -> Result<(), OperationError> {
        let root = directory(&self.directory);
        let count: i64 = self
            .connection
            .query_row("SELECT count(*) FROM artifacts", [], |r| r.get(0))?;
        if count == 0 {
            return Ok(());
        }
        if !root.try_exists().map_err(storage_error)? {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(&root)
                .map_err(storage_error)?;
        }
        let rows=self.connection.prepare("SELECT id,offer,offset,state FROM artifacts WHERE offer IS NOT NULL AND state!='unavailable'")?.query_map([], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)? as u64,r.get::<_,String>(3)?)))?.collect::<Result<Vec<_>,_>>()?;
        for (id, offer, offset, state) in rows {
            let id: ArtifactId = id.parse().map_err(|_| OperationError::Internal)?;
            let offer: ArtifactOffer = parse(offer)?;
            let SealedOutput::Content { content } = &offer.output else {
                continue;
            };
            if path(&self.directory, id, true)
                .try_exists()
                .map_err(storage_error)?
            {
                let verified = open(&path(&self.directory, id, true), false)
                    .and_then(|mut f| digest(&mut f, content));
                if verified.is_ok() {
                    self.promote(id, &offer, content)?;
                } else {
                    self.corrupt(id, &offer)?;
                }
            } else if state == "available" {
                self.corrupt(id, &offer)?;
            } else {
                fs::File::open(&self.directory)
                    .map_err(storage_error)?
                    .sync_all()
                    .map_err(storage_error)?;
                let file = path(&self.directory, id, false);
                match open(&file, true) {
                    Ok(mut f) if f.metadata().map_err(storage_error)?.len() >= offset => {
                        f.set_len(offset).map_err(storage_error)?;
                        f.sync_all().map_err(storage_error)?;
                        if offset == content.length {
                            if digest(&mut f, content).is_ok() {
                                fs::rename(&file, path(&self.directory, id, true))
                                    .map_err(storage_error)?;
                                self.promote(id, &offer, content)?;
                            } else {
                                self.corrupt(id, &offer)?;
                            }
                        }
                    }
                    _ if offset == 0 => {
                        self.create_partial(id)?;
                    }
                    _ => {
                        self.corrupt(id, &offer)?;
                    }
                }
            }
        }
        Ok(())
    }
    fn create_partial(&self, id: ArtifactId) -> Result<(), OperationError> {
        let root = directory(&self.directory);
        if !root.try_exists().map_err(storage_error)? {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(&root)
                .map_err(storage_error)?;
        }
        fs::File::open(&self.directory)
            .map_err(storage_error)?
            .sync_all()
            .map_err(storage_error)?;
        let file = path(&self.directory, id, false);
        let f = if file.try_exists().map_err(storage_error)? {
            open(&file, true)?
        } else {
            fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(file)
                .map_err(storage_error)?
        };
        f.set_len(0).map_err(storage_error)?;
        f.sync_all().map_err(storage_error)?;
        fs::File::open(root)
            .map_err(storage_error)?
            .sync_all()
            .map_err(storage_error)
    }
    fn corrupt(&mut self, id: ArtifactId, offer: &ArtifactOffer) -> Result<(), OperationError> {
        let mut task = self.load_task(offer.task)?;
        let output = task
            .outputs
            .iter_mut()
            .find(|o| o.declaration.slot == offer.slot)
            .ok_or(OperationError::Internal)?;
        output.state = ArtifactState::Unavailable {
            reason: ArtifactUnavailable::Corrupt,
        };
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE artifacts SET state='unavailable' WHERE id=?1",
            [id.to_string()],
        )?;
        tasks::save(&tx, &task)?;
        super::tasks::event(&tx, "artifact_updated", task.id)?;
        tx.commit()?;
        Ok(())
    }
    fn promote(
        &mut self,
        id: ArtifactId,
        offer: &ArtifactOffer,
        content: &ArtifactContent,
    ) -> Result<(), OperationError> {
        fs::File::open(directory(&self.directory))
            .map_err(storage_error)?
            .sync_all()
            .map_err(storage_error)?;
        let mut task = self.load_task(offer.task)?;
        task.outputs
            .iter_mut()
            .find(|o| o.declaration.slot == offer.slot)
            .ok_or(OperationError::Internal)?
            .state = ArtifactState::Available {
            id,
            content: content.clone(),
        };
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE artifacts SET state='available',offset=?1 WHERE id=?2",
            params![content.length as i64, id.to_string()],
        )?;
        tasks::save(&tx, &task)?;
        super::tasks::event(&tx, "artifact_updated", task.id)?;
        tx.commit()?;
        Ok(())
    }
    fn artifact_scope(
        &self,
        lease: ConnectionLease,
        journal: JournalId,
        execution: ExecutionId,
        slot: &str,
    ) -> Result<(ArtifactId, TaskView, Option<ArtifactOffer>, u64, String), OperationError> {
        self.check_lease(lease)?;
        if !valid_slot(slot) {
            return Err(OperationError::InvalidInput);
        }
        let (id,task,offer,offset,state):(String,String,Option<String>,u64,String)=self.connection.query_row("SELECT id,task,offer,offset,state FROM artifacts WHERE worker=?1 AND journal=?2 AND execution=?3 AND slot=?4",params![lease.worker.to_string(),journal.to_string(),execution.to_string(),slot],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get::<_,i64>(3)? as u64,r.get(4)?))).optional()?.ok_or(ErrorCode::Unauthorized)?;
        let current: String = self.connection.query_row(
            "SELECT incarnation FROM worker_journals WHERE worker=?1",
            [lease.worker.to_string()],
            |r| r.get(0),
        )?;
        if current != journal.to_string() {
            return Err(ErrorCode::Unauthorized.into());
        }
        let task = self.load_task(task.parse().map_err(|_| OperationError::Internal)?)?;
        if !task.approval.consumed
            || task.command.worker != lease.worker
            || task.command.execution != execution
        {
            return Err(ErrorCode::Unauthorized.into());
        }
        Ok((
            id.parse().map_err(|_| OperationError::Internal)?,
            task,
            offer.map(parse).transpose()?,
            offset,
            state,
        ))
    }
    pub(super) fn artifact_failed(
        &mut self,
        lease: ConnectionLease,
        offer: ArtifactOffer,
    ) -> Result<(), OperationError> {
        let (id, _, previous, offset, state) =
            self.artifact_scope(lease, offer.journal, offer.execution, &offer.slot)?;
        if previous.as_ref() != Some(&offer) {
            return Err(OperationError::Conflict);
        }
        let receipt = if state == "available" {
            ArtifactTransferState::Stored
        } else {
            self.corrupt(id, &offer)?;
            ArtifactTransferState::Unavailable
        };
        self.deliver(
            lease.worker,
            Envelope::new(ManagerMessage::ArtifactProgress {
                execution: offer.execution,
                slot: offer.slot,
                id,
                offset,
                state: receipt,
            }),
        )
    }
    pub(super) fn artifact_offer(
        &mut self,
        lease: ConnectionLease,
        offer: ArtifactOffer,
    ) -> Result<(), OperationError> {
        let (id, mut task, previous, offset, state) =
            self.artifact_scope(lease, offer.journal, offer.execution, &offer.slot)?;
        if task.id != offer.task {
            return Err(ErrorCode::Unauthorized.into());
        }
        if let Some(previous) = previous {
            if previous != offer {
                return Err(OperationError::Conflict);
            }
            return self.deliver(
                lease.worker,
                Envelope::new(ManagerMessage::ArtifactProgress {
                    execution: offer.execution,
                    slot: offer.slot,
                    id,
                    offset,
                    state: match state.as_str() {
                        "available" => ArtifactTransferState::Stored,
                        "unavailable" => ArtifactTransferState::Unavailable,
                        _ => ArtifactTransferState::Receiving,
                    },
                }),
            );
        }
        let stopped = matches!(
            task.outcome,
            Some(
                ExecutionEvent::ProcessFinished { .. }
                    | ExecutionEvent::Stopped {
                        evidence: StopEvidence::SystemdScopeEmpty { .. },
                        ..
                    }
            )
        );
        let uncertain = matches!(
            (&offer.output, &task.outcome),
            (
                SealedOutput::Unavailable {
                    reason: ArtifactUnavailable::ExecutionUncertain
                },
                Some(ExecutionEvent::Uncertain { .. })
            )
        );
        if !stopped && !uncertain {
            return Err(OperationError::Conflict);
        }
        let output = task
            .outputs
            .iter_mut()
            .find(|o| o.declaration.slot == offer.slot)
            .ok_or(ErrorCode::Unauthorized)?;
        let (length, state, view) = match &offer.output {
            SealedOutput::Unavailable { reason } => (
                0,
                "unavailable",
                ArtifactState::Unavailable { reason: *reason },
            ),
            SealedOutput::Content { content } => {
                if content.length > output.declaration.max_bytes
                    || !matches!(
                        task.outcome,
                        Some(
                            ExecutionEvent::ProcessFinished { .. }
                                | ExecutionEvent::Stopped {
                                    evidence: StopEvidence::SystemdScopeEmpty { .. },
                                    ..
                                }
                        )
                    )
                {
                    return Err(OperationError::Conflict);
                }
                self.create_partial(id)?;
                (
                    content.length,
                    "transferring",
                    ArtifactState::Transferring {
                        id,
                        content: content.clone(),
                    },
                )
            }
        };
        output.state = view;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE artifacts SET offer=?1,reserved=?2,state=?3 WHERE id=?4",
            params![json(&offer)?, length as i64, state, id.to_string()],
        )?;
        tasks::save(&tx, &task)?;
        super::tasks::event(&tx, "artifact_updated", task.id)?;
        tx.commit()?;
        self.deliver(
            lease.worker,
            Envelope::new(ManagerMessage::ArtifactProgress {
                execution: offer.execution,
                slot: offer.slot,
                id,
                offset: 0,
                state: if state == "unavailable" {
                    ArtifactTransferState::Unavailable
                } else {
                    ArtifactTransferState::Receiving
                },
            }),
        )
    }
    pub(super) fn artifact_chunk(
        &mut self,
        lease: ConnectionLease,
        chunk: ArtifactChunk,
    ) -> Result<(), OperationError> {
        let ArtifactChunk {
            journal,
            execution,
            slot,
            id,
            offset,
            hex,
        } = chunk;
        let (expected, _, offer, through, state) =
            self.artifact_scope(lease, journal, execution, &slot)?;
        if id != expected
            || hex.len() > ARTIFACT_CHUNK_BYTES * 2
            || !hex.len().is_multiple_of(2)
            || !hex.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(OperationError::InvalidInput);
        }
        let offer = offer.ok_or(OperationError::Conflict)?;
        let SealedOutput::Content { content } = &offer.output else {
            return Err(OperationError::Conflict);
        };
        if state == "unavailable" {
            return self.deliver(
                lease.worker,
                Envelope::new(ManagerMessage::ArtifactProgress {
                    execution,
                    slot,
                    id,
                    offset: through,
                    state: ArtifactTransferState::Unavailable,
                }),
            );
        }
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        let end = offset
            .checked_add(bytes.len() as u64)
            .ok_or(OperationError::InvalidInput)?;
        if end > content.length || offset > through || (offset < through && end > through) {
            return Err(OperationError::Conflict);
        }
        let file = open(
            &path(&self.directory, id, state == "available"),
            state != "available",
        );
        let checked = file.and_then(|mut file| {
            if state == "available" {
                digest(&mut file, content)?;
            }
            Ok(file)
        });
        let mut file = match checked {
            Ok(file) => file,
            Err(_) => {
                self.corrupt(id, &offer)?;
                return self.deliver(
                    lease.worker,
                    Envelope::new(ManagerMessage::ArtifactProgress {
                        execution,
                        slot,
                        id,
                        offset: through,
                        state: ArtifactTransferState::Unavailable,
                    }),
                );
            }
        };
        if offset < through || state == "available" {
            file.seek(SeekFrom::Start(offset)).map_err(storage_error)?;
            let mut prior = vec![0; bytes.len()];
            file.read_exact(&mut prior).map_err(storage_error)?;
            if prior != bytes {
                return Err(OperationError::Conflict);
            }
        } else {
            file.set_len(through).map_err(storage_error)?;
            file.seek(SeekFrom::Start(through)).map_err(storage_error)?;
            file.write_all(&bytes).map_err(storage_error)?;
            file.sync_all().map_err(storage_error)?;
            self.connection.execute(
                "UPDATE artifacts SET offset=?1 WHERE id=?2",
                params![end as i64, id.to_string()],
            )?;
        }
        let through = through.max(end);
        let mut receipt = if through == content.length {
            ArtifactTransferState::Stored
        } else {
            ArtifactTransferState::Receiving
        };
        if through == content.length && state != "available" {
            self.fault("artifact_offset_committed");
            if digest(&mut file, content).is_err() {
                self.corrupt(id, &offer)?;
                receipt = ArtifactTransferState::Unavailable;
            } else {
                fs::rename(
                    path(&self.directory, id, false),
                    path(&self.directory, id, true),
                )
                .map_err(storage_error)?;
                self.promote(id, &offer, content)?;
            }
        }
        self.deliver(
            lease.worker,
            Envelope::new(ManagerMessage::ArtifactProgress {
                execution,
                slot,
                id,
                offset: through,
                state: receipt,
            }),
        )
    }
    pub(in crate::manager) fn download_artifact(
        &mut self,
        token: impl Into<OwnerAuthority>,
        task: TaskId,
        id: ArtifactId,
    ) -> Result<(fs::File, OutputDeclaration, u64), OperationError> {
        self.authorize_owner(&token.into())?;
        let task = self.load_task(task)?;
        if task.command.owner != self.principal()? {
            return Err(OperationError::Unauthorized);
        }
        let output = task
            .outputs
            .iter()
            .find(|o| matches!(&o.state,ArtifactState::Available{id:other,..} if *other==id))
            .ok_or(OperationError::NotFound)?;
        let ArtifactState::Available { content, .. } = &output.state else {
            unreachable!()
        };
        let checked = open(&path(&self.directory, id, true), false).and_then(|mut file| {
            digest(&mut file, content)?;
            Ok(file)
        });
        let mut file = match checked {
            Ok(file) => file,
            Err(error) => {
                let encoded: String = self.connection.query_row(
                    "SELECT offer FROM artifacts WHERE id=?1",
                    [id.to_string()],
                    |r| r.get(0),
                )?;
                self.corrupt(id, &parse(encoded)?)?;
                return Err(error);
            }
        };
        file.seek(SeekFrom::Start(0)).map_err(storage_error)?;
        Ok((file, output.declaration.clone(), content.length))
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogCursor {
    bot: Option<BotId>,
    name: String,
    filename: String,
    id: ArtifactId,
}
impl State {
    pub(in crate::manager) fn artifact_page(
        &self,
        token: impl Into<OwnerAuthority>,
        query: ArtifactQuery,
    ) -> Result<ArtifactPage, OperationError> {
        self.authorize_owner(&token.into())?;
        if query.limit as usize != ARTIFACT_PAGE_SIZE
            || query.name.len() > MAX_ARTIFACT_NAME_QUERY
            || !query.name.bytes().all(|b| (b' '..=b'~').contains(&b))
            || query
                .after
                .as_ref()
                .is_some_and(|s| s.len() > MAX_ARTIFACT_CURSOR_BYTES)
        {
            return Err(OperationError::InvalidInput);
        }
        let name = query.name.to_ascii_lowercase();
        let after = query
            .after
            .map(|s| {
                serde_json::from_str::<CatalogCursor>(&s).map_err(|_| OperationError::InvalidInput)
            })
            .transpose()?;
        if after.as_ref().is_some_and(|c| {
            c.bot != query.bot
                || c.name != name
                || c.filename.len() > 80
                || c.filename.is_empty()
                || c.filename
                    .bytes()
                    .any(|b| !(b' '..=b'~').contains(&b) || matches!(b, b'/' | b'\\' | b'"'))
                || c.filename != c.filename.to_ascii_lowercase()
        }) {
            return Err(OperationError::InvalidInput);
        }
        let principal = self.principal()?;
        let mut statement = self.connection.prepare(
            "SELECT id,task,slot,offer FROM artifacts WHERE offer IS NOT NULL ORDER BY id LIMIT ?1",
        )?;
        let rows = statement.query_map([MAX_ARTIFACT_RECORDS as i64], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        let mut entries = Vec::new();
        for row in rows {
            let (id, task, slot, offer) = row?;
            let offer: ArtifactOffer = parse(offer)?;
            let id: ArtifactId = id.parse().map_err(|_| OperationError::Internal)?;
            let task: TaskView =
                self.load_task(task.parse().map_err(|_| OperationError::Internal)?)?;
            if offer.task != task.id || offer.slot != slot {
                continue;
            }
            if task.command.owner != principal
                || query.bot.is_some_and(|bot| bot != task.command.bot)
            {
                continue;
            }
            let Some(output) = task.outputs.iter().find(|o| o.declaration.slot == slot) else {
                continue;
            };
            match &output.state {
                ArtifactState::Available { id: other, .. }
                | ArtifactState::Transferring { id: other, .. }
                    if *other == id => {}
                ArtifactState::Unavailable { .. } => {}
                _ => continue,
            }
            let filename = output.declaration.name.to_ascii_lowercase();
            if !filename.contains(&name)
                || after
                    .as_ref()
                    .is_some_and(|c| (&filename, id.0) <= (&c.filename, c.id.0))
            {
                continue;
            }
            entries.push(SavedArtifact {
                id,
                task: task.id,
                bot: task.command.bot,
                output: output.clone(),
            });
        }
        entries.sort_by(|a, b| {
            (a.output.declaration.name.to_ascii_lowercase(), a.id.0)
                .cmp(&(b.output.declaration.name.to_ascii_lowercase(), b.id.0))
        });
        let has_more = entries.len() > ARTIFACT_PAGE_SIZE;
        entries.truncate(ARTIFACT_PAGE_SIZE);
        let next = if has_more {
            let last = entries.last().ok_or(OperationError::Internal)?;
            Some(json(&CatalogCursor {
                bot: query.bot,
                name,
                filename: last.output.declaration.name.to_ascii_lowercase(),
                id: last.id,
            })?)
        } else {
            None
        };
        Ok(ArtifactPage { entries, next })
    }
}
