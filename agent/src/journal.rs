use crate::session::Sessions;
use botstral_link::*;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
};

use botstral_link::JournalPhase as Phase;
#[derive(Clone, Serialize, Deserialize)]
struct Execution {
    body: CommandBody,
    phase: Phase,
    applied: u32,
    outcome: Option<ExecutionEvent>,
}

pub(crate) struct Journal {
    db: Connection,
    pub incarnation: JournalId,
    directory: PathBuf,
    test_worker: bool,
    ready: bool,
    deliveries: std::collections::HashMap<ExecutionId, Delivery>,
    sent_through: u64,
    claims: std::collections::HashMap<ExecutionId, std::time::Instant>,
    #[cfg(feature = "nonproduction-test-worker")]
    provider: crate::effect_provider::Provider,
}
fn err(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
fn encode<T: Serialize>(v: &T) -> io::Result<String> {
    serde_json::to_string(v).map_err(err)
}
fn decode<T: serde::de::DeserializeOwned>(v: String) -> io::Result<T> {
    serde_json::from_str(&v).map_err(err)
}
fn save(db: &Connection, e: &Execution) -> io::Result<()> {
    db.execute(
        "UPDATE executions SET record=?1 WHERE command=?2",
        params![encode(e)?, e.body.id.to_string()],
    )
    .map_err(err)?;
    Ok(())
}
fn report(db: &Connection, e: &Execution, event: ExecutionEvent) -> io::Result<()> {
    let pending: i64 = db
        .query_row("SELECT count(*) FROM reports", [], |r| r.get(0))
        .map_err(err)?;
    let head: i64 = db
        .query_row("SELECT head FROM metadata", [], |r| r.get(0))
        .map_err(err)?;
    if pending >= 16384 || head >= 16384 {
        return Err(err("report capacity reached"));
    }
    db.execute("UPDATE metadata SET head=head+1", [])
        .map_err(err)?;
    let sequence: i64 = db
        .query_row("SELECT head FROM metadata", [], |r| r.get(0))
        .map_err(err)?;
    let terminal = matches!(
        event,
        ExecutionEvent::Completed { .. }
            | ExecutionEvent::Failed { .. }
            | ExecutionEvent::Uncertain { .. }
            | ExecutionEvent::Stopped { .. }
    );
    let report = DurableReport {
        sequence: sequence as u64,
        command: e.body.id,
        execution: e.body.execution,
        event,
    };
    db.execute(
        "INSERT INTO reports VALUES(?1,?2)",
        params![sequence, encode(&report)?],
    )
    .map_err(err)?;
    if terminal {
        db.execute("INSERT INTO outcomes VALUES(?1,?2) ON CONFLICT(command) DO UPDATE SET body=excluded.body",params![e.body.id.to_string(),encode(&report)?]).map_err(err)?;
    }
    Ok(())
}
impl Journal {
    pub fn open(directory: &Path, test_worker: bool) -> io::Result<Self> {
        let canonical = directory.canonicalize()?;
        let directory = canonical.as_path();
        for name in ["agent.sqlite3", "agent.sqlite3-wal", "agent.sqlite3-shm"] {
            let p = directory.join(name);
            if std::fs::symlink_metadata(&p).is_ok() {
                tls::check_private_file(&p)?;
            }
        }
        let path = directory.join("agent.sqlite3");
        let exists = path.exists();
        let mut db = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(err)?;
        db.busy_timeout(std::time::Duration::from_secs(2))
            .map_err(err)?;
        db.pragma_update(None, "journal_mode", "WAL").map_err(err)?;
        db.pragma_update(None, "synchronous", "FULL").map_err(err)?;
        if !exists {
            let tx = db.transaction().map_err(err)?;
            tx.execute_batch("CREATE TABLE metadata(incarnation TEXT NOT NULL, head INTEGER NOT NULL, acknowledged INTEGER NOT NULL) STRICT; CREATE TABLE executions(command TEXT PRIMARY KEY, execution TEXT NOT NULL UNIQUE, task TEXT NOT NULL UNIQUE, record TEXT NOT NULL) STRICT; CREATE TABLE reports(sequence INTEGER PRIMARY KEY, body TEXT NOT NULL) STRICT; CREATE TABLE outcomes(command TEXT PRIMARY KEY, body TEXT NOT NULL) STRICT; PRAGMA user_version=1;").map_err(err)?;
            tx.execute(
                "INSERT INTO metadata VALUES(?1,0,0)",
                [JournalId::new().to_string()],
            )
            .map_err(err)?;
            tx.commit().map_err(err)?;
        }
        let version: i64 = db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(err)?;
        let integrity: String = db
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .map_err(err)?;
        if version != 1 || integrity != "ok" {
            return Err(err("invalid worker journal"));
        }
        let incarnation: String = db
            .query_row("SELECT incarnation FROM metadata", [], |r| r.get(0))
            .map_err(err)?;
        let mut result = Self {
            db,
            incarnation: incarnation.parse().map_err(err)?,
            directory: directory.into(),
            test_worker,
            ready: false,
            deliveries: Default::default(),
            sent_through: 0,
            claims: Default::default(),
            #[cfg(feature = "nonproduction-test-worker")]
            provider: crate::effect_provider::Provider::open(directory)?,
        };
        result.reconcile()?;
        Ok(result)
    }
    pub fn high_water(&self) -> io::Result<u64> {
        self.db
            .query_row("SELECT head FROM metadata", [], |r| r.get::<_, i64>(0))
            .map(|v| v as u64)
            .map_err(err)
    }
    pub fn attach(&mut self) {
        self.ready = false;
        self.deliveries.clear();
        self.sent_through = 0;
        self.claims.clear();
    }
    fn executions(&self) -> io::Result<Vec<Execution>> {
        self.db
            .prepare("SELECT record FROM executions ORDER BY command")
            .map_err(err)?
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(err)?
            .map(|r| decode(r.map_err(err)?))
            .collect()
    }
    fn reconcile(&mut self) -> io::Result<()> {
        for mut e in self.executions()? {
            if matches!(e.phase, Phase::ClaimPending | Phase::Admitted) {
                #[cfg(feature = "nonproduction-test-worker")]
                let observed = if self.test_worker {
                    self.provider.observe(&e.body)?
                } else {
                    None
                };
                #[cfg(not(feature = "nonproduction-test-worker"))]
                let observed: Option<u32> = None;
                let outcome = match observed {
                    Some(applied) => {
                        e.applied = applied;
                        if matches!(e.body.work,WorkSpec::TestInput { steps } if steps>applied) {
                            e.phase = Phase::Uncertain;
                            ExecutionEvent::Uncertain {
                                reason: "interrupted_input_requires_fresh_operation".into(),
                            }
                        } else {
                            e.phase = Phase::Resolved;
                            ExecutionEvent::Completed { applied }
                        }
                    }
                    None => {
                        e.phase = Phase::Uncertain;
                        ExecutionEvent::Uncertain {
                            reason: "effect_or_permit_unknown".into(),
                        }
                    }
                };
                e.outcome = Some(outcome.clone());
                let tx = self.db.transaction().map_err(err)?;
                save(&tx, &e)?;
                report(&tx, &e, outcome)?;
                tx.commit().map_err(err)?;
            }
        }
        Ok(())
    }
    pub fn disconnected(&mut self) -> io::Result<()> {
        self.attach();
        self.reconcile()
    }
    fn page(
        &self,
        sessions: &Sessions,
        after: u64,
        high_water: u64,
    ) -> io::Result<Vec<Envelope<WorkerMessage>>> {
        if after > high_water || high_water > self.high_water()? {
            return Err(err("invalid recovery cursor"));
        }
        let mut reports = Vec::new();
        for row in self.db.prepare("SELECT body FROM reports WHERE sequence>?1 AND sequence<=?2 ORDER BY sequence LIMIT ?3").map_err(err)?.query_map(params![after as i64,high_water as i64,REPORT_PAGE as i64],|r|r.get::<_,String>(0)).map_err(err)? {
            let entry:DurableReport=decode(row.map_err(err)?)?;
            reports.push(entry);
            if encode(&reports)?.len()>PAGE_BYTES {reports.pop();break;}
        }
        if reports.is_empty() {
            return Ok(vec![]);
        }
        Ok(vec![Envelope::new(WorkerMessage::Reports {
            generation: sessions.generation,
            incarnation: self.incarnation,
            high_water,
            reports,
        })])
    }
    pub fn handle(
        &mut self,
        sessions: &Sessions,
        message: &ManagerMessage,
    ) -> io::Result<Option<Vec<Envelope<WorkerMessage>>>> {
        let out = match message {
            ManagerMessage::InventoryRequest { offset } => {
                let inventory = self.executions()?;
                if *offset as usize > inventory.len() {
                    return Err(err("invalid inventory offset"));
                }
                let entries: Vec<_> = inventory
                    .iter()
                    .skip(*offset as usize)
                    .take(REPORT_PAGE)
                    .map(|e| JournalEntry {
                        body: e.body.clone(),
                        phase: e.phase.clone(),
                    })
                    .collect();
                if encode(&entries)?.len() > PAGE_BYTES {
                    return Err(err("inventory page size"));
                }
                vec![Envelope::new(WorkerMessage::Inventory {
                    generation: sessions.generation,
                    incarnation: self.incarnation,
                    offset: *offset,
                    total: inventory.len() as u16,
                    entries,
                })]
            }

            ManagerMessage::Recover { after, high_water } => {
                self.page(sessions, *after, *high_water)?
            }
            ManagerMessage::ReportAck { through } => {
                if *through > self.high_water()? {
                    return Err(err("future report receipt"));
                }
                let tx = self.db.transaction().map_err(err)?;
                tx.execute("DELETE FROM reports WHERE sequence<=?1", [*through as i64])
                    .map_err(err)?;
                tx.execute(
                    "UPDATE metadata SET acknowledged=max(acknowledged,?1)",
                    [*through as i64],
                )
                .map_err(err)?;
                tx.commit().map_err(err)?;
                self.sent_through = 0;
                vec![]
            }
            ManagerMessage::RecoveryReady { generation } if *generation == sessions.generation => {
                self.ready = true;
                vec![]
            }
            ManagerMessage::Delivery(delivery) => {
                sessions.check(delivery.fence).map_err(err)?;
                if !self.ready || !sessions.matches(&delivery.body) {
                    return Err(err("delivery before recovery or wrong task scope"));
                }
                let existing: Option<String> = self
                    .db
                    .query_row(
                        "SELECT record FROM executions WHERE command=?1",
                        [delivery.body.id.to_string()],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(err)?;
                let mut e = if let Some(body) = existing {
                    let e: Execution = decode(body)?;
                    if e.body != delivery.body {
                        return Err(err("immutable command conflict"));
                    }
                    e
                } else {
                    let count: i64 = self
                        .db
                        .query_row("SELECT count(*) FROM executions", [], |r| r.get(0))
                        .map_err(err)?;
                    if count >= MAX_TASKS as i64 {
                        return Err(err("journal admission capacity"));
                    }
                    let e = Execution {
                        body: delivery.body.clone(),
                        phase: Phase::Prepared,
                        applied: 0,
                        outcome: None,
                    };
                    let tx = self.db.transaction().map_err(err)?;
                    tx.execute(
                        "INSERT INTO executions VALUES(?1,?2,?3,?4)",
                        params![
                            e.body.id.to_string(),
                            e.body.execution.to_string(),
                            e.body.task.to_string(),
                            encode(&e)?
                        ],
                    )
                    .map_err(err)?;
                    report(&tx, &e, ExecutionEvent::Accepted)?;
                    if !matches!(e.body.work, WorkSpec::Unavailable) {
                        report(&tx, &e, ExecutionEvent::ApprovalRequired)?;
                    }
                    tx.commit().map_err(err)?;
                    self.fault("acceptance_committed");
                    e
                };
                if delivery.cancel {
                    if e.phase != Phase::Stopped {
                        e.phase = Phase::Stopped;
                        let outcome = ExecutionEvent::Stopped {
                            applied: e.applied,
                            evidence: StopEvidence::NoProcessGroup,
                        };
                        e.outcome = Some(outcome.clone());
                        let tx = self.db.transaction().map_err(err)?;
                        save(&tx, &e)?;
                        report(&tx, &e, outcome)?;
                        tx.commit().map_err(err)?;
                    }
                    self.deliveries.remove(&e.body.execution);
                } else if e.phase == Phase::Prepared {
                    self.deliveries.insert(e.body.execution, delivery.clone());
                } else if let Some(body) = self
                    .db
                    .query_row(
                        "SELECT body FROM outcomes WHERE command=?1",
                        [e.body.id.to_string()],
                        |r| r.get::<_, String>(0),
                    )
                    .optional()
                    .map_err(err)?
                {
                    let outcome: DurableReport = decode(body)?;
                    let acknowledged: i64 = self
                        .db
                        .query_row("SELECT acknowledged FROM metadata", [], |r| r.get(0))
                        .map_err(err)?;
                    if outcome.sequence > acknowledged as u64 {
                        return Ok(Some(self.page(
                            sessions,
                            acknowledged as u64,
                            self.high_water()?,
                        )?));
                    }
                    return Ok(Some(vec![Envelope::new(WorkerMessage::Reports {
                        generation: sessions.generation,
                        incarnation: self.incarnation,
                        high_water: self.high_water()?,
                        reports: vec![outcome],
                    })]));
                }
                vec![]
            }
            ManagerMessage::ClaimDenied { execution } => {
                self.claims.remove(execution);
                if let Some(delivery) = self.deliveries.remove(execution) {
                    let body: String = self
                        .db
                        .query_row(
                            "SELECT record FROM executions WHERE command=?1",
                            [delivery.body.id.to_string()],
                            |r| r.get(0),
                        )
                        .map_err(err)?;
                    let mut e: Execution = decode(body)?;
                    if e.phase == Phase::ClaimPending {
                        e.phase = Phase::Prepared;
                        let tx = self.db.transaction().map_err(err)?;
                        save(&tx, &e)?;
                        report(&tx, &e, ExecutionEvent::ApprovalRequired)?;
                        tx.commit().map_err(err)?;
                    }
                }
                vec![]
            }
            ManagerMessage::Permit {
                incarnation,
                fence,
                binding,
            } => {
                let Some(delivery) = self.deliveries.remove(&binding.execution) else {
                    return Ok(Some(vec![]));
                };
                let body: String = self
                    .db
                    .query_row(
                        "SELECT record FROM executions WHERE command=?1",
                        [delivery.body.id.to_string()],
                        |r| r.get(0),
                    )
                    .map_err(err)?;
                let mut e: Execution = decode(body)?;
                if e.phase != Phase::ClaimPending
                    || *incarnation != self.incarnation
                    || *fence != delivery.fence
                    || delivery.authority.as_ref() != Some(binding)
                    || sessions.check(*fence).is_err()
                    || binding.expires_at <= now()
                {
                    self.reconcile()?;
                    return Ok(Some(vec![]));
                }
                self.claims.remove(&binding.execution);
                e.phase = Phase::Admitted;
                let tx = self.db.transaction().map_err(err)?;
                save(&tx, &e)?;
                report(&tx, &e, ExecutionEvent::Admitted)?;
                tx.commit().map_err(err)?;
                self.fault("permit_admitted");
                self.execute(&mut e, sessions, *fence, binding.expires_at)?;
                vec![]
            }
            _ => return Ok(None),
        };
        Ok(Some(out))
    }
    pub fn tick(&mut self, sessions: &Sessions) -> io::Result<Vec<Envelope<WorkerMessage>>> {
        if !self.ready {
            return Ok(vec![]);
        }
        let mut messages = Vec::new();
        let paused = cfg!(all(feature = "nonproduction-test-worker", debug_assertions))
            && self.directory.join("pause-before-claim").exists();
        for mut e in self.executions()? {
            if e.phase == Phase::ClaimPending
                && self
                    .claims
                    .get(&e.body.execution)
                    .is_some_and(|started| started.elapsed() >= std::time::Duration::from_secs(5))
            {
                e.phase = Phase::Uncertain;
                let outcome = ExecutionEvent::Uncertain {
                    reason: "permit_timeout".into(),
                };
                e.outcome = Some(outcome.clone());
                self.deliveries.remove(&e.body.execution);
                self.claims.remove(&e.body.execution);
                let tx = self.db.transaction().map_err(err)?;
                save(&tx, &e)?;
                report(&tx, &e, outcome)?;
                tx.commit().map_err(err)?;
            }
            if matches!(e.body.work, WorkSpec::Unavailable) && e.phase == Phase::Prepared {
                e.phase = Phase::Resolved;
                e.outcome = Some(ExecutionEvent::Failed {
                    code: ErrorCode::UnsupportedExecution,
                });
                let tx = self.db.transaction().map_err(err)?;
                save(&tx, &e)?;
                report(&tx, &e, e.outcome.clone().unwrap())?;
                tx.commit().map_err(err)?;
                continue;
            }
            if e.phase == Phase::Prepared
                && !paused
                && let Some(delivery) = self.deliveries.get(&e.body.execution)
                && let Some(binding) = &delivery.authority
                && sessions.check(delivery.fence).is_ok()
                && delivery.fence.owner == InputOwner::Bot
                && binding.expires_at > now()
            {
                e.phase = Phase::ClaimPending;
                self.claims
                    .insert(e.body.execution, std::time::Instant::now());
                save(&self.db, &e)?;
                messages.push(Envelope::new(WorkerMessage::Claim {
                    incarnation: self.incarnation,
                    fence: delivery.fence,
                    binding: binding.clone(),
                }));
            }
        }
        let head = self.high_water()?;
        let ack: i64 = self
            .db
            .query_row("SELECT acknowledged FROM metadata", [], |r| r.get(0))
            .map_err(err)?;
        if head > ack as u64 && self.sent_through == 0 {
            messages.extend(self.page(sessions, ack as u64, head)?);
            self.sent_through = head;
        }
        Ok(messages)
    }
    fn execute(
        &mut self,
        e: &mut Execution,
        sessions: &Sessions,
        fence: InputFence,
        expires_at: i64,
    ) -> io::Result<()> {
        if sessions.check(fence).is_err() || fence.owner != InputOwner::Bot || expires_at <= now() {
            return self.reconcile();
        }
        #[cfg(feature = "nonproduction-test-worker")]
        if self.test_worker {
            let applied = self.provider.apply(&e.body)?;
            self.fault("effect_committed");
            e.applied = applied;
            e.phase = Phase::Resolved;
            e.outcome = Some(ExecutionEvent::Completed { applied });
            let tx = self.db.transaction().map_err(err)?;
            save(&tx, e)?;
            report(&tx, e, ExecutionEvent::Progress { applied })?;
            report(&tx, e, e.outcome.clone().unwrap())?;
            tx.commit().map_err(err)?;
            self.fault("outcome_committed");
            return Ok(());
        }
        let _ = self.test_worker;
        e.phase = Phase::Resolved;
        e.outcome = Some(ExecutionEvent::Failed {
            code: ErrorCode::UnsupportedExecution,
        });
        let tx = self.db.transaction().map_err(err)?;
        save(&tx, e)?;
        report(&tx, e, e.outcome.clone().unwrap())?;
        tx.commit().map_err(err)?;
        Ok(())
    }
    fn fault(&self, point: &str) {
        #[cfg(all(feature = "nonproduction-test-worker", debug_assertions))]
        if std::env::var("BOTSTRAL_FAULT").as_deref() == Ok(point) {
            use std::{io::Write, os::unix::fs::OpenOptionsExt};
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(self.directory.join(format!("fault-{point}.reached")))
                .expect("fault marker");
            f.write_all(point.as_bytes()).expect("fault marker");
            f.sync_all().expect("fault sync");
            std::process::abort();
        }
        let _ = point;
    }
}
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn journal_opens_and_preserves_incarnation() {
        let root = tempfile::tempdir().unwrap();
        let journal =
            Journal::open(root.path(), cfg!(feature = "nonproduction-test-worker")).unwrap();
        let incarnation = journal.incarnation;
        drop(journal);
        use std::os::unix::fs::PermissionsExt;
        for entry in std::fs::read_dir(root.path()).unwrap() {
            std::fs::set_permissions(
                entry.unwrap().path(),
                std::fs::Permissions::from_mode(0o600),
            )
            .unwrap();
        }
        assert_eq!(
            Journal::open(root.path(), cfg!(feature = "nonproduction-test-worker"))
                .unwrap()
                .incarnation,
            incarnation
        );
    }
}

#[cfg(test)]
mod corruption_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn corrupt_or_linked_journals_never_become_empty_replacements() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("agent.sqlite3");
        let original = b"corrupt durable journal";
        std::fs::write(&path, original).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(Journal::open(root.path(), false).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::remove_file(&path).unwrap();
        let target = root.path().join("unrelated");
        std::fs::write(&target, original).unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::hard_link(&target, &path).unwrap();
        assert!(Journal::open(root.path(), false).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), original);
    }
}
