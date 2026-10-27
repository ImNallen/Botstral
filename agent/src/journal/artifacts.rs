use super::*;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    time::{Duration, Instant},
};

#[derive(Clone, Serialize, Deserialize)]
struct Output {
    task: TaskId,
    execution: ExecutionId,
    declaration: OutputDeclaration,
    offer: Option<ArtifactOffer>,
    id: Option<ArtifactId>,
    offset: u64,
    received: bool,
}
pub(super) struct Flight {
    message: Envelope<WorkerMessage>,
    sent: Instant,
}
fn blob(root: &Path, execution: ExecutionId, slot: &str) -> PathBuf {
    root.join("spool").join(format!("{execution}-{slot}.blob"))
}
fn save_output(db: &Connection, o: &Output) -> io::Result<()> {
    db.execute(
        "UPDATE artifacts SET record=?1,reserved=?2 WHERE execution=?3 AND slot=?4",
        params![
            encode(o)?,
            if o.received {
                0
            } else {
                match &o.offer {
                    Some(ArtifactOffer {
                        output: SealedOutput::Content { content },
                        ..
                    }) => content.length,
                    Some(_) => 0,
                    None => o.declaration.max_bytes,
                }
            } as i64,
            o.execution.to_string(),
            o.declaration.slot
        ],
    )
    .map_err(err)?;
    Ok(())
}
fn snapshot(source: &Path, destination: &Path, max: u64) -> io::Result<ArtifactContent> {
    if !destination.try_exists()? {
        let mut input = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(source)?;
        let meta = input.metadata()?;
        if !meta.is_file() || meta.len() > max {
            return Err(err("output is not a bounded regular file"));
        }
        let temp = destination.with_extension("tmp");
        if temp.try_exists()? {
            fs::remove_file(&temp)?;
        }
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temp)?;
        let mut total = 0;
        let mut buffer = [0; 16384];
        loop {
            let n = input.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if total > max {
                return Err(err("output grew beyond declaration"));
            }
            output.write_all(&buffer[..n])?;
        }
        output.sync_all()?;
        fs::rename(temp, destination)?;
    }
    fs::File::open(destination.parent().unwrap())?.sync_all()?;
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(destination)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > max {
        return Err(err("sealed output invalid"));
    }
    let mut h = Sha256::new();
    let mut total = 0;
    let mut buffer = [0; 16384];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > max {
            return Err(err("sealed output bound"));
        }
        h.update(&buffer[..n]);
    }
    Ok(ArtifactContent {
        sha256: format!("{:x}", h.finalize()).try_into().map_err(err)?,
        length: total,
    })
}
impl Journal {
    pub(super) fn initialize_artifacts(&self) -> io::Result<()> {
        self.db.execute_batch("CREATE TABLE IF NOT EXISTS artifacts(execution TEXT NOT NULL,slot TEXT NOT NULL,reserved INTEGER NOT NULL CHECK(reserved>=0),record TEXT NOT NULL,PRIMARY KEY(execution,slot)) STRICT;").map_err(err)?;
        Ok(())
    }
    fn outputs(&self) -> io::Result<Vec<Output>> {
        self.db
            .prepare("SELECT record FROM artifacts ORDER BY execution,slot")
            .map_err(err)?
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(err)?
            .map(|r| decode(r.map_err(err)?))
            .collect()
    }
    pub(super) fn reserve_outputs(&mut self, e: &Execution) -> io::Result<bool> {
        if e.body.work.outputs().is_empty() {
            return Ok(true);
        }
        let outputs = self.outputs()?;
        let (used, records): (i64, i64) = self
            .db
            .query_row(
                "SELECT coalesce(sum(reserved),0),count(*) FROM artifacts",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(err)?;
        let additional: Vec<_> = e
            .body
            .work
            .outputs()
            .iter()
            .filter(|d| {
                !outputs
                    .iter()
                    .any(|o| o.execution == e.body.execution && o.declaration.slot == d.slot)
            })
            .collect();
        if used as u64 + additional.iter().map(|o| o.max_bytes).sum::<u64>() > MAX_SPOOL_BYTES
            || records as usize + additional.len() > MAX_ARTIFACT_RECORDS
        {
            return Ok(false);
        }
        let spool = self.directory.join("spool");
        if !spool.try_exists()? {
            fs::DirBuilder::new().mode(0o700).create(&spool)?;
            fs::File::open(&self.directory)?.sync_all()?;
        }
        let tx = self.db.transaction().map_err(err)?;
        for declaration in additional {
            let o = Output {
                task: e.body.task,
                execution: e.body.execution,
                declaration: declaration.clone(),
                offer: None,
                id: None,
                offset: 0,
                received: false,
            };
            tx.execute(
                "INSERT INTO artifacts VALUES(?1,?2,?3,?4)",
                params![
                    e.body.execution.to_string(),
                    declaration.slot,
                    declaration.max_bytes as i64,
                    encode(&o)?
                ],
            )
            .map_err(err)?;
        }
        tx.commit().map_err(err)?;
        Ok(true)
    }
    pub(super) fn release_output_reservations(&self, execution: ExecutionId) -> io::Result<()> {
        self.db.execute("DELETE FROM artifacts WHERE execution=?1 AND json_extract(record,'$.offer') IS NULL",[execution.to_string()]).map_err(err)?;
        Ok(())
    }
    pub(super) fn tick_artifacts(&mut self) -> io::Result<Vec<Envelope<WorkerMessage>>> {
        for mut o in self.outputs()? {
            if o.received {
                let path = blob(&self.directory, o.execution, &o.declaration.slot);
                if path.try_exists()? {
                    fs::remove_file(path)?;
                }
                continue;
            }
            if o.offer.is_some() {
                continue;
            }
            let e = self
                .executions()?
                .into_iter()
                .find(|e| e.body.execution == o.execution)
                .ok_or_else(|| err("output execution missing"))?;
            if !super::process::stopped(&e) {
                continue;
            }
            let output = if matches!(
                e.outcome,
                Some(
                    ExecutionEvent::ProcessFinished { .. }
                        | ExecutionEvent::Stopped {
                            evidence: StopEvidence::SystemdScopeEmpty { .. },
                            ..
                        }
                )
            ) {
                let source = crate::process::directory(&self.directory, o.execution)
                    .join("outputs")
                    .join(format!("{}.out", o.declaration.slot));
                match snapshot(
                    &source,
                    &blob(&self.directory, o.execution, &o.declaration.slot),
                    o.declaration.max_bytes,
                ) {
                    Ok(content) => SealedOutput::Content { content },
                    Err(error) => SealedOutput::Unavailable {
                        reason: if error.kind() == io::ErrorKind::NotFound {
                            ArtifactUnavailable::Missing
                        } else {
                            ArtifactUnavailable::Rejected
                        },
                    },
                }
            } else {
                SealedOutput::Unavailable {
                    reason: ArtifactUnavailable::ExecutionUncertain,
                }
            };
            o.offer = Some(ArtifactOffer {
                journal: self.incarnation,
                task: o.task,
                execution: o.execution,
                slot: o.declaration.slot.clone(),
                output,
            });
            save_output(&self.db, &o)?;
            let temporary =
                blob(&self.directory, o.execution, &o.declaration.slot).with_extension("tmp");
            if temporary.try_exists()? {
                fs::remove_file(temporary)?;
            }
        }
        for e in self.executions()? {
            if super::process::stopped(&e)
                && self
                    .outputs()?
                    .iter()
                    .filter(|o| o.execution == e.body.execution)
                    .all(|o| o.offer.is_some())
            {
                let produced =
                    crate::process::directory(&self.directory, e.body.execution).join("outputs");
                if produced.try_exists()? {
                    fs::remove_dir_all(produced)?;
                }
            }
        }
        if !self.ready {
            return Ok(vec![]);
        }
        if let Some(flight) = &mut self.artifact_flight {
            if flight.sent.elapsed() < Duration::from_secs(1) {
                return Ok(vec![]);
            }
            flight.sent = Instant::now();
            return Ok(vec![flight.message.clone()]);
        }
        let Some(o) = self
            .outputs()?
            .into_iter()
            .find(|o| !o.received && o.offer.is_some())
        else {
            return Ok(vec![]);
        };
        let offer = o.offer.unwrap();
        let confirmed = self
            .artifact_confirmed
            .as_ref()
            .is_some_and(|(execution, slot)| {
                *execution == o.execution && *slot == o.declaration.slot
            });
        let message = if let (true, Some(id), SealedOutput::Content { content }) =
            (confirmed, o.id, &offer.output)
        {
            if o.offset > content.length {
                return Err(err("artifact receipt offset"));
            }
            let mut bytes =
                vec![0; (content.length - o.offset).min(ARTIFACT_CHUNK_BYTES as u64) as usize];
            let reading = (|| -> io::Result<()> {
                let mut f = fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                    .open(blob(&self.directory, o.execution, &o.declaration.slot))?;
                if !f.metadata()?.is_file() {
                    return Err(err("snapshot regular file required"));
                }
                f.seek(SeekFrom::Start(o.offset))?;
                f.read_exact(&mut bytes)
            })();
            if reading.is_err() {
                let message = Envelope::new(WorkerMessage::ArtifactFailed { offer });
                self.artifact_flight = Some(Flight {
                    message: message.clone(),
                    sent: Instant::now(),
                });
                return Ok(vec![message]);
            }
            use std::fmt::Write;
            let mut hex = String::with_capacity(bytes.len() * 2);
            for b in bytes {
                write!(&mut hex, "{b:02x}").unwrap();
            }
            WorkerMessage::ArtifactChunk {
                chunk: ArtifactChunk {
                    journal: self.incarnation,
                    execution: o.execution,
                    slot: o.declaration.slot,
                    id,
                    offset: o.offset,
                    hex,
                },
            }
        } else {
            WorkerMessage::ArtifactOffer { offer }
        };
        let message = Envelope::new(message);
        self.artifact_flight = Some(Flight {
            message: message.clone(),
            sent: Instant::now(),
        });
        Ok(vec![message])
    }
    pub(super) fn artifact_progress(
        &mut self,
        execution: ExecutionId,
        slot: &str,
        id: ArtifactId,
        offset: u64,
        state: ArtifactTransferState,
    ) -> io::Result<()> {
        let Some(mut o) = self
            .outputs()?
            .into_iter()
            .find(|o| o.execution == execution && o.declaration.slot == slot)
        else {
            return Err(err("unknown artifact receipt"));
        };
        let Some(offer) = &o.offer else {
            return Err(err("artifact not sealed"));
        };
        let length = match &offer.output {
            SealedOutput::Content { content } => content.length,
            SealedOutput::Unavailable { .. } => 0,
        };
        if o.id.is_some_and(|v| v != id)
            || offset > length
            || (state == ArtifactTransferState::Stored && offset != length)
        {
            return Err(err("invalid artifact receipt"));
        }
        o.id = Some(id);
        o.offset = offset;
        o.received = state != ArtifactTransferState::Receiving;
        save_output(&self.db, &o)?;
        self.artifact_confirmed = Some((execution, slot.to_owned()));
        self.artifact_flight = None;
        if o.received {
            let path = blob(&self.directory, execution, slot);
            if path.try_exists()? {
                fs::remove_file(path)?;
                fs::File::open(self.directory.join("spool"))?.sync_all()?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spool_capacity_is_reserved_before_authority_and_released_only_when_not_admitted() {
        let root = tempfile::tempdir().unwrap();
        let mut journal = Journal::open(root.path(), false).unwrap();
        let mut executions = Vec::new();
        for index in 0..5 {
            let spec = ComputerSpec {
                profile: ComputerProfile::LinuxPlaywrightV1,
                account: ProcessAccount {
                    uid: 1000,
                    gid: 1000,
                },
                runtime: ComputerRuntime {
                    sha256: CertificateFingerprint::from_der(b"runtime"),
                },
                deadline: 4000000000,
                limits: ProcessLimits {
                    runtime_seconds: 60,
                    output_bytes: 1024,
                    processes: 128,
                    memory_bytes: 1073741824,
                },
                viewport: Viewport {
                    width: 800,
                    height: 600,
                },
                operations: vec![ComputerOperation::WriteFile {
                    path: "/approved/guest".into(),
                    text: "fixture".into(),
                }],
                outputs: (0..4)
                    .map(|n| OutputDeclaration {
                        slot: format!("slot{n}"),
                        name: "file.txt".into(),
                        media_type: MediaType::TextPlain,
                        max_bytes: 4 * 1024 * 1024,
                    })
                    .collect(),
            };
            spec.validate().unwrap();
            let e = Execution {
                body: CommandBody {
                    id: CommandId::new(),
                    owner: OwnerId::new(),
                    task: TaskId::new(),
                    bot: BotId::new(),
                    worker: WorkerId::new(),
                    session: ComputerSessionId::new(),
                    operation: OperationId::new(),
                    execution: ExecutionId::new(),
                    work: WorkSpec::Computer {
                        spec: Box::new(spec),
                    },
                },
                phase: Phase::Prepared,
                applied: 0,
                outcome: None,
                process: None,
            };
            assert_eq!(journal.reserve_outputs(&e).unwrap(), index < 4);
            executions.push(e);
        }
        let used: i64 = journal
            .db
            .query_row("SELECT sum(reserved) FROM artifacts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(used as u64, MAX_SPOOL_BYTES);
        assert!(
            journal.reserve_outputs(&executions[0]).unwrap(),
            "identical reservations do not allocate twice"
        );
        journal
            .release_output_reservations(executions[0].body.execution)
            .unwrap();
        assert!(journal.reserve_outputs(&executions[4]).unwrap());
    }
}
