use super::*;
use crate::process::{self, Backend, Configuration, EmptyScopeProof, Manifest, Ready};
use std::{
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
};
use tokio::task::JoinHandle;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct ProcessExecution {
    manifest: Manifest,
    fence: InputFence,
    lifecycle: Lifecycle,
    evidence: ProcessEvidence,
    runtime_deadline: Option<i64>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Lifecycle {
    LaunchIntent,
    Identified {
        ready: Ready,
    },
    ReleasePossible {
        ready: Ready,
    },
    StopIntent {
        ready: Option<Ready>,
        reason: StopReason,
    },
    Stopped {
        scope: ProcessScope,
    },
    Unproved,
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum StopReason {
    Completed,
    Cancelled,
    Disconnected,
    Shutdown,
    Control,
    Deadline,
    Recovery,
}
pub(super) enum ProcessOperation {
    Start(JoinHandle<io::Result<Ready>>),
    Stop(JoinHandle<io::Result<EmptyScopeProof>>),
}
impl ProcessOperation {
    fn finished(&self) -> bool {
        match self {
            Self::Start(h) => h.is_finished(),
            Self::Stop(h) => h.is_finished(),
        }
    }
}
impl Journal {
    pub async fn configure_processes(&mut self) -> io::Result<()> {
        let config = Configuration::read(&self.directory)?;
        if let Some(configuration) = config.clone() {
            match Backend::probe(&self.directory, self.incarnation, configuration).await {
                Ok(backend) => self.backend = Some(backend),
                Err(_) => eprintln!("process capability unavailable"),
            }
        }
        for mut e in self.executions()? {
            if let Some(process) = e.process.clone()
                && !matches!(process.lifecycle, Lifecycle::Stopped { .. })
            {
                if self.process_slot.is_some() {
                    return Err(err("multiple admitted process scopes"));
                }
                self.process_slot = Some(e.body.execution);
                self.request_stop(&mut e, StopReason::Recovery)?;
                if self.backend.is_none() {
                    let configuration = Configuration {
                        profile: process::Configuration::read(&self.directory)?
                            .map_or(ProcessProfile::LinuxSystemdV1, |v| v.profile),
                        account: process.manifest.spec.account,
                        ceilings: process.manifest.spec.limits.clone(),
                    };
                    self.backend = Backend::recovery(configuration).ok();
                }
            }
        }
        Ok(())
    }
    pub fn process_capable(&self) -> bool {
        self.backend.as_ref().is_some_and(|backend| {
            backend.probed
                && Configuration::read(&self.directory).ok().flatten().as_ref()
                    == Some(&backend.configuration)
        })
    }
    pub(super) fn reserve_process(
        &mut self,
        e: &Execution,
        spec: &ProcessSpec,
    ) -> io::Result<bool> {
        if self.process_slot.is_some()
            || !self.process_capable()
            || spec.validate().is_err()
            || !self
                .backend
                .as_ref()
                .is_some_and(|b| b.configuration.permits(spec))
            || spec.deadline <= now()
        {
            return Ok(false);
        }
        let head = self.high_water()?;
        let reserved: i64 = self
            .db
            .query_row(
                "SELECT coalesce(sum(remaining),0) FROM reservations",
                [],
                |r| r.get(0),
            )
            .map_err(err)?;
        let own: i64 = self
            .db
            .query_row(
                "SELECT coalesce((SELECT remaining FROM reservations WHERE execution=?1),0)",
                [e.body.execution.to_string()],
                |r| r.get(0),
            )
            .map_err(err)?;
        if head + (reserved - own) as u64 + 8 > 16384 {
            return Ok(false);
        }
        let directory = process::directory(&self.directory, e.body.execution);
        if directory.try_exists()? {
            for entry in std::fs::read_dir(&directory)? {
                if entry?.file_name() != "reserved" {
                    return Err(err("process directory cannot be reused"));
                }
            }
        } else {
            std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
        }
        let reserved = directory.join("reserved");
        if reserved.try_exists()? {
            tls::check_private_file(&reserved)?;
        }
        let mut storage = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&reserved)?;
        storage.write_all(&[0; 32768])?;
        storage.sync_all()?;
        self.db
            .execute(
                "INSERT OR IGNORE INTO reservations VALUES(?1,8)",
                [e.body.execution.to_string()],
            )
            .map_err(err)?;
        self.process_slot = Some(e.body.execution);
        Ok(true)
    }
    pub(super) fn admit_process(
        &mut self,
        e: &mut Execution,
        binding: &ExecutionBinding,
        fence: InputFence,
    ) -> io::Result<()> {
        let WorkSpec::Process { spec } = &e.body.work else {
            unreachable!()
        };
        if self.process_slot != Some(e.body.execution) || !self.process_capable() {
            return self.reconcile_execution(e);
        }
        let backend = self.backend.as_ref().unwrap().clone();
        let manifest = Manifest {
            journal: self.incarnation,
            execution: e.body.execution,
            host: backend.host.clone(),
            boot: backend.boot.clone(),
            spec: *spec.clone(),
            expires_at: binding.expires_at,
        };
        let directory = process::directory(&self.directory, e.body.execution);
        std::fs::remove_file(directory.join("reserved"))?;
        process::write(&directory.join("manifest.json"), &manifest)?;
        e.process = Some(ProcessExecution {
            manifest: manifest.clone(),
            fence,
            lifecycle: Lifecycle::LaunchIntent,
            runtime_deadline: None,
            evidence: ProcessEvidence {
                effect: ProcessEffect::NotReleased,
                result: None,
            },
        });
        save(&self.db, e)?;
        self.fault("process_launch_intent");
        self.process_operation = Some(ProcessOperation::Start(tokio::spawn(async move {
            backend.start(&directory, &manifest).await
        })));
        Ok(())
    }
    pub(super) fn cancel_process(&mut self, e: &mut Execution) -> io::Result<()> {
        if e.process.is_some() {
            return self.request_stop(e, StopReason::Cancelled);
        }
        if e.phase == Phase::Stopped {
            return Ok(());
        }
        self.claims.remove(&e.body.execution);
        if self.process_slot == Some(e.body.execution) {
            self.process_slot = None;
        }
        e.phase = Phase::Stopped;
        e.outcome = Some(ExecutionEvent::Stopped {
            applied: 0,
            evidence: StopEvidence::NotLaunched,
        });
        let tx = self.db.transaction().map_err(err)?;
        save(&tx, e)?;
        report(&tx, e, e.outcome.clone().unwrap())?;
        tx.execute(
            "DELETE FROM reservations WHERE execution=?1",
            [e.body.execution.to_string()],
        )
        .map_err(err)?;
        tx.commit().map_err(err)
    }
    fn request_stop(&mut self, e: &mut Execution, reason: StopReason) -> io::Result<()> {
        let Some(process) = &mut e.process else {
            return Ok(());
        };
        let ready = match &process.lifecycle {
            Lifecycle::Stopped { .. } => {
                if reason == StopReason::Cancelled && e.phase != Phase::Stopped {
                    let Lifecycle::Stopped { scope } = &process.lifecycle else {
                        unreachable!()
                    };
                    e.phase = Phase::Stopped;
                    e.outcome = Some(ExecutionEvent::Stopped {
                        applied: 0,
                        evidence: StopEvidence::SystemdScopeEmpty {
                            scope: scope.clone(),
                        },
                    });
                    let tx = self.db.transaction().map_err(err)?;
                    save(&tx, e)?;
                    report(&tx, e, e.outcome.clone().unwrap())?;
                    tx.execute(
                        "DELETE FROM reservations WHERE execution=?1",
                        [e.body.execution.to_string()],
                    )
                    .map_err(err)?;
                    tx.commit().map_err(err)?;
                }
                return Ok(());
            }
            Lifecycle::Unproved => return Ok(()),
            Lifecycle::StopIntent { ready, .. } => {
                if reason == StopReason::Cancelled {
                    process.lifecycle = Lifecycle::StopIntent {
                        ready: ready.clone(),
                        reason,
                    };
                    save(&self.db, e)?;
                }
                return Ok(());
            }
            Lifecycle::LaunchIntent => None,
            Lifecycle::Identified { ready } | Lifecycle::ReleasePossible { ready } => {
                Some(ready.clone())
            }
        };
        process.lifecycle = Lifecycle::StopIntent { ready, reason };
        save(&self.db, e)
    }
    pub fn stop_processes(&mut self, shutdown: bool) -> io::Result<()> {
        self.ready = false;
        for mut e in self.executions()? {
            self.request_stop(
                &mut e,
                if shutdown {
                    StopReason::Shutdown
                } else {
                    StopReason::Disconnected
                },
            )?;
        }
        Ok(())
    }
    pub fn can_confirm_session(&self, session: ComputerSessionId) -> io::Result<bool> {
        Ok(self.ready
            && self
                .executions()?
                .iter()
                .filter(|e| e.body.session == session)
                .all(|e| {
                    e.process
                        .as_ref()
                        .is_none_or(|p| matches!(p.lifecycle, Lifecycle::Stopped { .. }))
                }))
    }
    pub fn processes_stopped(&self) -> io::Result<bool> {
        Ok(self.executions()?.iter().all(|e| {
            e.process
                .as_ref()
                .is_none_or(|p| matches!(p.lifecycle, Lifecycle::Stopped { .. }))
        }))
    }
    fn unproved(&mut self, e: &mut Execution, reason: &str) -> io::Result<()> {
        if matches!(e.process.as_ref().unwrap().lifecycle, Lifecycle::Unproved) {
            return Ok(());
        }
        e.process.as_mut().unwrap().lifecycle = Lifecycle::Unproved;
        e.phase = Phase::Uncertain;
        e.outcome = Some(ExecutionEvent::Uncertain {
            reason: reason.into(),
        });
        let tx = self.db.transaction().map_err(err)?;
        save(&tx, e)?;
        report(&tx, e, e.outcome.clone().unwrap())?;
        tx.commit().map_err(err)
    }
    pub async fn tick_processes(&mut self, sessions: Option<&Sessions>) -> io::Result<()> {
        let Some(id) = self.process_slot else {
            return Ok(());
        };
        let mut e = self
            .executions()?
            .into_iter()
            .find(|e| e.body.execution == id)
            .ok_or_else(|| err("active process missing"))?;
        if e.process.is_none() {
            if !matches!(e.phase, Phase::ClaimPending | Phase::Admitted) {
                self.process_slot = None;
            }
            return Ok(());
        }
        if let Some(op) = &self.process_operation {
            if !op.finished() {
                return Ok(());
            }
            match self.process_operation.take().unwrap() {
                ProcessOperation::Start(handle) => match handle.await.map_err(err)? {
                    Ok(ready) => {
                        self.fault("process_start_observed");
                        let p = e.process.as_mut().unwrap();
                        p.lifecycle = match &p.lifecycle {
                            Lifecycle::StopIntent { reason, .. } => Lifecycle::StopIntent {
                                ready: Some(ready),
                                reason: *reason,
                            },
                            _ => Lifecycle::Identified { ready },
                        };
                        save(&self.db, &e)?;
                    }
                    Err(_) => {
                        self.request_stop(&mut e, StopReason::Recovery)?;
                    }
                },
                ProcessOperation::Stop(handle) => match handle.await.map_err(err)? {
                    Ok(proof) => return self.finish_process(&mut e, proof),
                    Err(_) => {
                        if let Lifecycle::StopIntent {
                            ready: Some(ready), ..
                        } = e.process.as_ref().unwrap().lifecycle.clone()
                        {
                            self.import_result(&mut e, &self.backend.clone().unwrap(), &ready)?;
                        }
                        return self.unproved(&mut e, "scope_stop_unproved");
                    }
                },
            }
        }
        let Some(backend) = self.backend.clone() else {
            return self.unproved(&mut e, "process_backend_unavailable");
        };
        let directory = process::directory(&self.directory, id);
        let p = e.process.as_ref().unwrap().clone();
        match p.lifecycle {
            Lifecycle::Identified { ready } => {
                let authorized = self.ready
                    && sessions.is_some_and(|s| s.check(p.fence).is_ok())
                    && p.fence.owner == InputOwner::Bot
                    && p.manifest.expires_at > now()
                    && p.manifest.spec.deadline > now()
                    && self.process_capable()
                    && backend.configuration.permits(&p.manifest.spec);
                if !authorized {
                    self.request_stop(&mut e, StopReason::Control)?;
                    return Ok(());
                }
                let process = e.process.as_mut().unwrap();
                process.lifecycle = Lifecycle::ReleasePossible {
                    ready: ready.clone(),
                };
                process.runtime_deadline =
                    Some(now() + i64::from(process.manifest.spec.limits.runtime_seconds));
                process.evidence.effect = ProcessEffect::Possible;
                let event = ExecutionEvent::ProcessResult {
                    evidence: process.evidence.clone(),
                };
                let tx = self.db.transaction().map_err(err)?;
                save(&tx, &e)?;
                report(&tx, &e, event)?;
                tx.commit().map_err(err)?;
                if backend.release(&directory, &ready).is_err() {
                    self.request_stop(&mut e, StopReason::Recovery)?;
                }
            }
            Lifecycle::ReleasePossible { ready } => {
                if directory.join("result.json").try_exists()? {
                    self.import_result(&mut e, &backend, &ready)?;
                    self.request_stop(&mut e, StopReason::Completed)?;
                } else if p.manifest.spec.deadline <= now()
                    || p.runtime_deadline.is_some_and(|until| now() >= until)
                {
                    self.request_stop(&mut e, StopReason::Deadline)?;
                } else if !self.ready {
                    self.request_stop(&mut e, StopReason::Disconnected)?;
                } else if sessions.is_none_or(|s| s.check(p.fence).is_err()) {
                    self.request_stop(&mut e, StopReason::Control)?;
                }
            }
            Lifecycle::StopIntent { ready, reason } => {
                let ready = match ready {
                    Some(r) => r,
                    None => match backend.observe(&directory, &p.manifest).await {
                        Ok(r) => r,
                        Err(_) => return self.unproved(&mut e, "launch_or_scope_unknown"),
                    },
                };
                e.process.as_mut().unwrap().lifecycle = Lifecycle::StopIntent {
                    ready: Some(ready.clone()),
                    reason,
                };
                save(&self.db, &e)?;
                self.process_operation = Some(ProcessOperation::Stop(tokio::spawn(async move {
                    backend.stop(&ready.scope).await
                })));
            }
            Lifecycle::LaunchIntent => {
                self.request_stop(&mut e, StopReason::Recovery)?;
            }
            Lifecycle::Stopped { .. } | Lifecycle::Unproved => {}
        }
        Ok(())
    }
    fn import_result(
        &mut self,
        e: &mut Execution,
        backend: &Backend,
        ready: &Ready,
    ) -> io::Result<()> {
        let p = e.process.as_mut().unwrap();
        if p.evidence.result.is_some() {
            return Ok(());
        }
        let directory = process::directory(&self.directory, e.body.execution);
        let observed = match backend.observation(&directory, ready, true)? {
            Some(value) => Some(value),
            None => backend.observation(&directory, ready, false)?,
        };
        if let Some(mut evidence) = observed {
            self.fault("process_result_observed");
            if p.evidence.effect == ProcessEffect::Possible
                && evidence.effect == ProcessEffect::NotReleased
            {
                evidence.effect = ProcessEffect::Possible;
            }
            p.evidence = evidence.clone();
            let tx = self.db.transaction().map_err(err)?;
            save(&tx, e)?;
            report(&tx, e, ExecutionEvent::ProcessResult { evidence })?;
            tx.commit().map_err(err)?;
        }
        Ok(())
    }
    fn finish_process(&mut self, e: &mut Execution, proof: EmptyScopeProof) -> io::Result<()> {
        let p = e.process.as_ref().unwrap();
        let Lifecycle::StopIntent {
            ready: Some(ready),
            reason,
        } = &p.lifecycle
        else {
            return Err(err("stop proof without intent"));
        };
        let reason = *reason;
        let ready = ready.clone();
        self.import_result(e, &self.backend.clone().unwrap(), &ready)?;
        let p = e.process.as_mut().unwrap();
        p.lifecycle = Lifecycle::Stopped {
            scope: proof.scope().clone(),
        };
        let observed = p
            .evidence
            .result
            .as_ref()
            .is_some_and(|r| !matches!(r.exit, ProcessExit::Unobserved));
        let event = if reason == StopReason::Completed && observed {
            e.phase = Phase::Resolved;
            ExecutionEvent::ProcessFinished {
                reason: ProcessCompletionReason::Result,
                success: p
                    .evidence
                    .result
                    .as_ref()
                    .is_some_and(|r| r.exit == ProcessExit::Exited { code: 0 }),
                scope: proof.scope().clone(),
            }
        } else if reason == StopReason::Deadline {
            e.phase = Phase::Resolved;
            ExecutionEvent::ProcessFinished {
                reason: ProcessCompletionReason::Limit,
                success: false,
                scope: proof.scope().clone(),
            }
        } else if reason == StopReason::Cancelled {
            e.phase = Phase::Stopped;
            ExecutionEvent::Stopped {
                applied: 0,
                evidence: StopEvidence::SystemdScopeEmpty {
                    scope: proof.scope().clone(),
                },
            }
        } else {
            e.phase = Phase::Uncertain;
            ExecutionEvent::Uncertain {
                reason: if reason == StopReason::Deadline {
                    "process_deadline"
                } else {
                    "process_interrupted"
                }
                .into(),
            }
        };
        e.outcome = Some(event.clone());
        let tx = self.db.transaction().map_err(err)?;
        save(&tx, e)?;
        report(&tx, e, event)?;
        if e.phase == Phase::Stopped {
            tx.execute(
                "DELETE FROM reservations WHERE execution=?1",
                [e.body.execution.to_string()],
            )
            .map_err(err)?;
        } else {
            tx.execute(
                "UPDATE reservations SET remaining=min(remaining,1) WHERE execution=?1",
                [e.body.execution.to_string()],
            )
            .map_err(err)?;
        }
        tx.commit().map_err(err)?;
        self.process_slot = None;
        Ok(())
    }
}
