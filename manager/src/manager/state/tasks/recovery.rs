use super::*;

impl State {
    pub(in crate::manager) fn pump_tasks(
        &mut self,
        worker: WorkerId,
    ) -> Result<(), OperationError> {
        let Some(connection) = self.worker_connections.get(&worker) else {
            return Ok(());
        };
        if !connection.recovered {
            return Ok(());
        }
        let generation = connection.generation;
        let test_capable = connection.capabilities.contains(&Capability::TestInput);
        let capacity = connection.outbound.capacity().min(REPORT_PAGE);
        let ids = self
            .connection
            .prepare("SELECT id FROM tasks WHERE worker = ?1 ORDER BY id")?
            .query_map([worker.to_string()], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut sent = 0;
        for id in ids {
            let task = self.load_task(id.parse().map_err(|_| OperationError::Internal)?)?;
            if task.state.terminal() || (task.state == TaskState::AwaitingAttention) {
                continue;
            }
            if sent >= capacity {
                break;
            }
            if !test_capable && !matches!(task.command.work, WorkSpec::Unavailable) {
                continue;
            }
            let session = self.find_computer_session(task.command.session)?;
            if session.status != ControlStatus::Confirmed
                || (session.owner != InputOwner::Bot && task.state != TaskState::Cancelling)
            {
                continue;
            }
            let incarnation: String = self.connection.query_row(
                "SELECT incarnation FROM worker_journals WHERE worker = ?1",
                [worker.to_string()],
                |r| r.get(0),
            )?;
            self.connection.execute(
                "UPDATE commands SET delivered = 1, incarnation = ?1 WHERE id = ?2 ",
                params![incarnation, task.command.id.to_string()],
            )?;
            let delivery = Delivery {
                body: task.command,
                fence: session.fence(generation),
                authority: if task.approval.revoked || task.approval.consumed {
                    None
                } else {
                    task.approval.binding
                },
                cancel: task.state == TaskState::Cancelling,
            };
            if self
                .worker_connections
                .get(&worker)
                .unwrap()
                .deliveries
                .get(&delivery.body.id)
                == Some(&delivery)
            {
                continue;
            }
            self.deliver(
                worker,
                Envelope::new(ManagerMessage::Delivery(delivery.clone())),
            )?;
            self.worker_connections
                .get_mut(&worker)
                .unwrap()
                .deliveries
                .insert(delivery.body.id, delivery);
            sent += 1;
        }
        Ok(())
    }
    pub(in crate::manager) fn recovery_hello(
        &mut self,
        lease: ConnectionLease,
        incarnation: JournalId,
        high_water: u64,
    ) -> Result<(), OperationError> {
        if high_water > MAX_COUNTER {
            return Err(OperationError::InvalidInput);
        }
        let old: Option<(String, u64)> = self
            .connection
            .query_row(
                "SELECT incarnation, received FROM worker_journals WHERE worker = ?1",
                [lease.worker.to_string()],
                |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64)),
            )
            .optional()?;
        let after = if let Some((old_id, received)) = old {
            if old_id == incarnation.to_string() {
                if received > high_water {
                    return Err(ErrorCode::Conflict.into());
                }
                received
            } else {
                let views = self
                    .connection
                    .prepare("SELECT view FROM tasks WHERE worker = ?1")?
                    .query_map([lease.worker.to_string()], |r| r.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                let tx = self.connection.transaction()?;
                for view in views {
                    let mut task: TaskView = parse(view)?;
                    let delivered: bool = tx.query_row(
                        "SELECT delivered FROM commands WHERE task = ?1",
                        [task.id.to_string()],
                        |r| r.get(0),
                    )?;
                    if delivered && !task.state.terminal() {
                        task.state = task.state.reduce(&ExecutionEvent::Uncertain {
                            reason: "journal_replaced".into(),
                        });
                        task.outcome = Some(ExecutionEvent::Uncertain {
                            reason: "journal_replaced".into(),
                        });
                        save(&tx, &task)?;
                        event(&tx, "journal_replaced", task.id)?;
                    }
                }
                tx.execute(
                    "DELETE FROM worker_journals WHERE worker = ?1",
                    [lease.worker.to_string()],
                )?;
                tx.commit()?;
                0
            }
        } else {
            0
        };
        self.connection.execute(
            "INSERT OR IGNORE INTO worker_journals VALUES(?1, ?2, 0)",
            params![lease.worker.to_string(), incarnation.to_string()],
        )?;
        self.worker_connections
            .get_mut(&lease.worker)
            .unwrap()
            .recovery_high = high_water;
        if after == high_water {
            self.deliver(
                lease.worker,
                Envelope::new(ManagerMessage::InventoryRequest { offset: 0 }),
            )?;
        } else {
            self.deliver(
                lease.worker,
                Envelope::new(ManagerMessage::Recover { after, high_water }),
            )?;
        }
        Ok(())
    }
    pub(in crate::manager) fn reconcile_inventory(
        &mut self,
        lease: ConnectionLease,
        incarnation: JournalId,
        offset: u16,
        total: u16,
        entries: Vec<JournalEntry>,
    ) -> Result<(), OperationError> {
        let current: String = self.connection.query_row(
            "SELECT incarnation FROM worker_journals WHERE worker=?1",
            [lease.worker.to_string()],
            |r| r.get(0),
        )?;
        let connection = self.worker_connections.get(&lease.worker).unwrap();
        if current != incarnation.to_string()
            || total as usize > MAX_TASKS
            || offset as usize != connection.inventory.len()
            || connection.inventory_total.is_some_and(|n| n != total)
            || entries.len() > REPORT_PAGE
            || json(&entries)?.len() > PAGE_BYTES
            || offset as usize + entries.len() > total as usize
            || (entries.is_empty() && offset < total)
        {
            return Err(ErrorCode::Conflict.into());
        }
        let mut uncertain = Vec::new();
        for entry in entries {
            let mut task = self.load_task(entry.body.task)?;
            if task.command != entry.body
                || entry.body.worker != lease.worker
                || self
                    .worker_connections
                    .get(&lease.worker)
                    .unwrap()
                    .inventory
                    .contains(&entry.body.id)
            {
                return Err(ErrorCode::Conflict.into());
            }
            if task.approval.consumed
                && matches!(entry.phase, JournalPhase::Prepared)
                && !task.state.terminal()
            {
                task.state = task.state.reduce(&ExecutionEvent::Uncertain {
                    reason: "consumed_permit_missing".into(),
                });
                task.outcome = Some(ExecutionEvent::Uncertain {
                    reason: "consumed_permit_missing".into(),
                });
                uncertain.push(task);
            }
            self.worker_connections
                .get_mut(&lease.worker)
                .unwrap()
                .inventory
                .push(entry.body.id);
        }
        self.worker_connections
            .get_mut(&lease.worker)
            .unwrap()
            .inventory_total = Some(total);
        let next = self
            .worker_connections
            .get(&lease.worker)
            .unwrap()
            .inventory
            .len();
        if next < total as usize {
            let tx = self.connection.transaction()?;
            for task in uncertain {
                save(&tx, &task)?;
                event(&tx, "recovery_attention", task.id)?;
            }
            tx.commit()?;
            self.deliver(
                lease.worker,
                Envelope::new(ManagerMessage::InventoryRequest {
                    offset: next as u16,
                }),
            )?;
            return Ok(());
        }
        let views = self
            .connection
            .prepare("SELECT view FROM tasks WHERE worker=?1")?
            .query_map([lease.worker.to_string()], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        for view in views {
            let mut task: TaskView = parse(view)?;
            if task.approval.consumed
                && !task.state.terminal()
                && !self
                    .worker_connections
                    .get(&lease.worker)
                    .unwrap()
                    .inventory
                    .contains(&task.command.id)
            {
                task.state = task.state.reduce(&ExecutionEvent::Uncertain {
                    reason: "execution_missing_from_journal".into(),
                });
                task.outcome = Some(ExecutionEvent::Uncertain {
                    reason: "execution_missing_from_journal".into(),
                });
                uncertain.push(task);
            }
        }
        let tx = self.connection.transaction()?;
        for task in uncertain {
            save(&tx, &task)?;
            event(&tx, "recovery_attention", task.id)?;
        }
        tx.commit()?;
        self.finish_recovery(lease)
    }
    fn finish_recovery(&mut self, lease: ConnectionLease) -> Result<(), OperationError> {
        self.worker_connections
            .get_mut(&lease.worker)
            .unwrap()
            .recovered = true;
        self.deliver(
            lease.worker,
            Envelope::new(ManagerMessage::RecoveryReady {
                generation: lease.generation,
            }),
        )?;
        self.pump_tasks(lease.worker)
    }
    pub(in crate::manager) fn durable_reports(
        &mut self,
        lease: ConnectionLease,
        incarnation: JournalId,
        high_water: u64,
        reports: Vec<DurableReport>,
    ) -> Result<(), OperationError> {
        if reports.is_empty() || reports.len() > REPORT_PAGE || json(&reports)?.len() > PAGE_BYTES {
            return Err(OperationError::InvalidInput);
        }
        let (current, mut through): (String, u64) = self.connection.query_row(
            "SELECT incarnation, received FROM worker_journals WHERE worker = ?1",
            [lease.worker.to_string()],
            |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64)),
        )?;
        if current != incarnation.to_string() || high_water > MAX_COUNTER {
            return Err(ErrorCode::Conflict.into());
        }
        let recovering = !self
            .worker_connections
            .get(&lease.worker)
            .unwrap()
            .recovered;
        if recovering
            && high_water
                != self
                    .worker_connections
                    .get(&lease.worker)
                    .unwrap()
                    .recovery_high
        {
            return Err(ErrorCode::Conflict.into());
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for report in reports {
            if report.sequence > high_water || report.sequence == 0 {
                return Err(ErrorCode::InvalidMessage.into());
            }
            let body = json(&report)?;
            if body.len() > 1024
                || matches!(&report.event,ExecutionEvent::Uncertain {reason} if reason.len()>256)
            {
                return Err(ErrorCode::InvalidMessage.into());
            }
            let prior: Option<String> = tx.query_row("SELECT body FROM report_receipts WHERE worker=?1 AND incarnation=?2 AND sequence=?3", params![lease.worker.to_string(),current, report.sequence as i64], |r| r.get(0)).optional()?;
            if let Some(prior) = prior {
                if prior != body {
                    return Err(ErrorCode::Conflict.into());
                }
                continue;
            }
            if report.sequence != through + 1 {
                return Err(ErrorCode::Conflict.into());
            }
            let view: String = tx.query_row("SELECT tasks.view FROM tasks JOIN commands ON commands.task=tasks.id WHERE commands.id=?1 AND tasks.worker=?2 AND commands.incarnation=?3", params![report.command.to_string(),lease.worker.to_string(),current], |r| r.get(0)).optional()?.ok_or(ErrorCode::UnknownTask)?;
            let mut task: TaskView = parse(view)?;
            if task.command.execution != report.execution {
                return Err(ErrorCode::Conflict.into());
            }
            let was_terminal = task.state.terminal();
            if !was_terminal
                && matches!(
                    report.event,
                    ExecutionEvent::Completed { .. } | ExecutionEvent::Uncertain { .. }
                )
                && !task.effect_evidence.contains(&report.event)
            {
                if task.effect_evidence.len() >= 4 {
                    return Err(ErrorCode::InvalidMessage.into());
                }
                task.effect_evidence.push(report.event.clone());
            }
            if !(was_terminal
                || task.approval.consumed
                    && matches!(report.event, ExecutionEvent::ApprovalRequired))
            {
                task.state = task.state.reduce(&report.event);
            }
            match &report.event {
                ExecutionEvent::Progress { applied }
                | ExecutionEvent::Completed { applied }
                | ExecutionEvent::Stopped { applied, .. } => {
                    if !was_terminal {
                        task.progress = task.progress.max(*applied);
                    }
                }
                _ => {}
            }
            if !was_terminal
                && matches!(
                    report.event,
                    ExecutionEvent::Completed { .. }
                        | ExecutionEvent::Failed { .. }
                        | ExecutionEvent::Uncertain { .. }
                        | ExecutionEvent::Stopped { .. }
                )
            {
                task.outcome = Some(report.event.clone());
            }
            save(&tx, &task)?;
            tx.execute(
                "INSERT INTO report_receipts VALUES(?1,?2,?3,?4)",
                params![
                    lease.worker.to_string(),
                    current,
                    report.sequence as i64,
                    body
                ],
            )?;
            event(&tx, "worker_report", task.id)?;
            through = report.sequence;
        }
        tx.execute(
            "UPDATE worker_journals SET received=?1 WHERE worker=?2",
            params![through as i64, lease.worker.to_string()],
        )?;
        tx.commit()?;
        self.fault("reports_committed");
        self.deliver(
            lease.worker,
            Envelope::new(ManagerMessage::ReportAck { through }),
        )?;
        if !recovering {
            self.pump_tasks(lease.worker)?;
        }
        if recovering {
            if through == high_water {
                self.deliver(
                    lease.worker,
                    Envelope::new(ManagerMessage::InventoryRequest { offset: 0 }),
                )?;
            } else {
                self.deliver(
                    lease.worker,
                    Envelope::new(ManagerMessage::Recover {
                        after: through,
                        high_water,
                    }),
                )?;
            }
        }
        Ok(())
    }
    pub(in crate::manager) fn claim(
        &mut self,
        lease: ConnectionLease,
        incarnation: JournalId,
        fence: InputFence,
        binding: ExecutionBinding,
    ) -> Result<(), OperationError> {
        let allowed = self.check_input_fence(lease.worker, fence).is_ok()
            && fence.owner == InputOwner::Bot
            && self
                .worker_connections
                .get(&lease.worker)
                .is_some_and(|c| c.recovered);
        let mut task = self.load_task(binding.task)?;
        let policy = self.permission(task.command.bot)?;
        let current: String = self.connection.query_row(
            "SELECT incarnation FROM worker_journals WHERE worker=?1",
            [lease.worker.to_string()],
            |r| r.get(0),
        )?;
        let delivered: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM commands WHERE id=?1 AND incarnation=?2 AND delivered=1)",
            params![task.command.id.to_string(), current],
            |r| r.get(0),
        )?;
        let valid = allowed
            && delivered
            && current == incarnation.to_string()
            && task.command.worker == lease.worker
            && task.command.session == fence.session_id
            && !task.state.terminal()
            && !matches!(
                task.state,
                TaskState::Cancelling | TaskState::AwaitingAttention
            )
            && !task.approval.revoked
            && !task.approval.consumed
            && task.approval.binding.as_ref() == Some(&binding)
            && policy.allow
            && policy.revision == binding.permission_revision
            && binding.expires_at > self.now()
            && task.request.settings.deadline > self.now();
        if !valid {
            return self.deliver(
                lease.worker,
                Envelope::new(ManagerMessage::ClaimDenied {
                    execution: binding.execution,
                }),
            );
        }
        task.approval.consumed = true;
        task.state = task.state.reduce(&ExecutionEvent::Admitted);
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        save(&tx, &task)?;
        event(&tx, "execution_claimed", task.id)?;
        tx.commit()?;
        self.fault("claim_committed");
        self.deliver(
            lease.worker,
            Envelope::new(ManagerMessage::Permit {
                incarnation,
                fence,
                binding,
            }),
        )
    }
}
