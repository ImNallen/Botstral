use super::*;

impl State {
    pub(super) fn prepare_occurrence(
        &mut self,
        routine: &RoutineView,
        key: String,
        at: i64,
        skipped: u32,
        prompt: Option<&str>,
        pending: Option<OccurrenceId>,
    ) -> Result<(RoutineOccurrence, Option<super::runs::PreparedRun>), OperationError> {
        let mut occurrence = RoutineOccurrence {
            id: pending.unwrap_or_default(),
            routine: routine.id,
            revision: routine.revision,
            trigger_key: key,
            scheduled_at: at,
            run: None,
            status: OccurrenceStatus::Running,
            skipped_count: skipped,
            updated_at: self.now(),
        };
        let result = (|| {
            if routine.state != RoutineState::Enabled {
                return Err(OperationError::Conflict);
            }
            self.check_binding(routine)?;
            let overlap:bool=self.connection.query_row("SELECT EXISTS(SELECT 1 FROM routine_occurrences o JOIN runs r ON r.id=o.run WHERE o.routine=?1 AND o.status IN ('running','decision') AND json_extract(r.summary,'$.phase.type') IN ('ready','calling','waiting_for_tool','waiting_for_memory','cancelling','awaiting_attention'))",[routine.id.to_string()],|r|r.get(0))?;
            if overlap {
                occurrence.status = OccurrenceStatus::Skipped {
                    reason: "Previous work is unresolved; overlap policy is skip.".into(),
                };
                return Ok(None);
            }
            self.budget_available(&routine.definition.template, pending)?;
            self.validate_run_admission(materialize(
                &routine.definition.template,
                routine.definition.bot,
                self.now(),
                prompt,
            ))
            .map(Some)
        })();
        let run = match result {
            Ok(run) => run,
            Err(OperationError::Internal) => return Err(OperationError::Internal),
            Err(e) => {
                occurrence.status = OccurrenceStatus::Blocked {
                    reason: format!("{e:?}"),
                };
                None
            }
        };
        occurrence.run = run.as_ref().map(|r| r.view.summary.id);
        Ok((occurrence, run))
    }
    pub(super) fn commit_occurrence(
        db: &Connection,
        owner: OwnerId,
        routine: &RoutineView,
        occurrence: &RoutineOccurrence,
        run: Option<&super::runs::PreparedRun>,
    ) -> Result<(), OperationError> {
        if let Some(run) = run {
            super::runs::insert_run(db, owner, run)?;
            let run = &run.view;
            db.execute(
                "INSERT INTO routine_reservations VALUES(?1,?2,?3,?4)",
                params![
                    run.summary.id.to_string(),
                    routine.id.to_string(),
                    run.request.limits.max_total_tokens as i64,
                    run.request.limits.max_cost_microusd as i64
                ],
            )?;
            db.execute(
                "UPDATE routine_budget SET admitted_runs=admitted_runs+1,tokens=tokens+?1,cost=cost+?2 WHERE id=1",
                params![run.request.limits.max_total_tokens as i64,run.request.limits.max_cost_microusd as i64],
            )?;
        }
        save_occurrence(db, occurrence)?;
        if run.is_none() {
            notify(db, routine, occurrence)?;
        }
        Ok(())
    }
    pub(super) fn fence_deleted_bots(&mut self) -> Result<(), OperationError> {
        for summary in self.run_summaries()? {
            if !resolved(&summary.phase) && self.require_live_bot(summary.bot).is_err() {
                let mut run = self.load_run(summary.id)?;
                run.summary
                    .cancellation_requested_at
                    .get_or_insert(self.now());
                self.stop_run(&mut run, None)?;
            }
        }
        Ok(())
    }
    pub(in crate::manager) fn reconcile_routines(&mut self) -> Result<(), OperationError> {
        self.fence_deleted_bots()?;
        self.reconcile_occurrence_results()?;
        self.compact_run_transcripts()?;
        let due=self.connection.prepare("SELECT view FROM routines WHERE state='enabled' AND next_at<=?1 ORDER BY next_at LIMIT 16")?.query_map([self.now()],|r|r.get::<_,String>(0))?.map(|r|parse(r?)).collect::<Result<Vec<RoutineView>,_>>()?;
        for mut r in due {
            let (at, skipped) = calendar::latest(
                &r.definition.trigger,
                r.next_at.ok_or(OperationError::Internal)?,
                self.now(),
            )?;
            let key = format!("schedule:{at}");
            let recorded:bool=self.connection.query_row("SELECT EXISTS(SELECT 1 FROM routine_occurrences WHERE routine=?1 AND trigger_key=?2)",params![r.id.to_string(),key],|row|row.get(0))?;
            if recorded {
                r.next_at = calendar::next(&r.definition.trigger, self.now())?;
                let tx = self.connection.transaction()?;
                save_routine(&tx, &r)?;
                tx.commit()?;
                continue;
            }
            let (mut o, mut run) = self.prepare_occurrence(&r, key, at, skipped, None, None)?;
            if at < self.now() - i64::from(r.definition.catchup_seconds) {
                o.status = OccurrenceStatus::Skipped {
                    reason: "Missed work expired outside the catchup window.".into(),
                };
                o.run = None;
                run = None;
            }
            r.next_at = calendar::next(&r.definition.trigger, self.now())?;
            let owner = self.principal()?;
            let tx = self.connection.transaction()?;
            Self::commit_occurrence(&tx, owner, &r, &o, run.as_ref())?;
            save_routine(&tx, &r)?;
            tx.commit()?;
            self.fault("routine_admitted");
        }
        let pending=self.connection.prepare("SELECT view,prompt FROM routine_occurrences WHERE status='pending' AND scheduled_at<=?1 ORDER BY scheduled_at LIMIT 16")?.query_map([self.now()],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?)))?.collect::<Result<Vec<_>,_>>()?;
        for (body, prompt) in pending {
            let old: RoutineOccurrence = parse(body)?;
            let r = self.load_routine(old.routine)?;
            let (o, run) = self.prepare_occurrence(
                &r,
                old.trigger_key,
                old.scheduled_at,
                0,
                prompt.as_deref(),
                Some(old.id),
            )?;
            let owner = self.principal()?;
            let tx = self.connection.transaction()?;
            Self::commit_occurrence(&tx, owner, &r, &o, run.as_ref())?;
            tx.commit()?;
        }
        Ok(())
    }
    fn compact_run_transcripts(&mut self) -> Result<(), OperationError> {
        let rows=self.connection.prepare("SELECT run,view,provider_request FROM run_turns WHERE run IN (SELECT id FROM runs WHERE json_extract(summary,'$.phase.type') IN ('completed','cancelled','stopped') AND id NOT IN (SELECT id FROM runs WHERE json_extract(summary,'$.phase.type') IN ('completed','cancelled','stopped') ORDER BY rowid DESC LIMIT 16) AND id IN (SELECT run FROM run_turns) AND id NOT IN (SELECT run FROM run_children) AND id NOT IN (SELECT parent_run FROM routine_wakeups) ORDER BY rowid LIMIT 16) AND run NOT IN (SELECT run FROM routine_occurrences WHERE status IN ('running','decision')) ORDER BY run,ordinal")?.query_map([],|row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?)))?.collect::<Result<Vec<_>,_>>()?;
        if rows.is_empty() {
            return Ok(());
        }
        let tx = self.connection.transaction()?;
        for (run, body, request) in rows {
            let turn: RunTurn = parse(body)?;
            let receipt = ArchivedRunTurn {
                manager_outcome: turn.manager_result.as_ref().and_then(|r| r.outcome.clone()),
                id: turn.id,
                ordinal: turn.ordinal,
                attempt: turn.attempt,
                messages_sha256: format!("{:x}", Sha256::digest(json(&turn.messages)?.as_bytes())),
                request_sha256: format!("{:x}", Sha256::digest(request.as_bytes())),
                response_sha256: format!("{:x}", Sha256::digest(json(&turn.response)?.as_bytes())),
                tool_call_id: turn
                    .intent
                    .as_ref()
                    .map(|i| i.call.id.clone())
                    .or_else(|| turn.manager_result.as_ref().map(|r| r.call.id.clone())),
                text: match turn.response {
                    Some(RunMessage::Assistant { content, .. }) => Some(content),
                    _ => None,
                },
                accounting: turn.accounting,
                reservation: turn.reservation,
                usage: turn.usage,
                finish_reason: turn.finish_reason,
                provider_request_id: turn.provider_request_id,
                phase: turn.phase,
            };
            tx.execute(
                "INSERT INTO run_turn_receipts VALUES(?1,?2,?3,?4) ON CONFLICT(turn) DO NOTHING",
                params![
                    receipt.id.to_string(),
                    run,
                    receipt.attempt.to_string(),
                    json(&receipt)?
                ],
            )?;
            tx.execute(
                "DELETE FROM run_turns WHERE id=?1",
                [receipt.id.to_string()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    fn reconcile_occurrence_results(&mut self) -> Result<(), OperationError> {
        let entries=self.connection.prepare("SELECT view FROM routine_occurrences WHERE status IN ('running','decision') ORDER BY rowid LIMIT ?1")?.query_map([BATCH as i64],|r|r.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?;
        for body in entries {
            let mut o: RoutineOccurrence = parse(body)?;
            let run = self.load_run(o.run.ok_or(OperationError::Internal)?)?;
            let status = if resolved(&run.summary.phase) {
                OccurrenceStatus::Finished {
                    phase: run.summary.phase.clone(),
                }
            } else if let Some(reason) = self.routine_decision(&run)? {
                OccurrenceStatus::Decision { reason }
            } else {
                OccurrenceStatus::Running
            };
            if o.status == status {
                continue;
            }
            o.status = status;
            o.updated_at = self.now();
            let r = self.load_routine(o.routine)?;
            let tx = self.connection.transaction()?;
            if resolved(&run.summary.phase) {
                let (old_tokens, old_cost): (i64, i64) = tx.query_row(
                    "SELECT tokens,cost FROM routine_reservations WHERE run=?1",
                    [run.summary.id.to_string()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                tx.execute(
                    "UPDATE routine_budget SET tokens=tokens-?1+?2,cost=cost-?3+?4 WHERE id=1",
                    params![
                        old_tokens,
                        (run.summary.accounting.settled_tokens + run.summary.accounting.held_tokens)
                            as i64,
                        old_cost,
                        (run.summary.accounting.settled_cost_microusd
                            + run.summary.accounting.held_cost_microusd)
                            as i64
                    ],
                )?;
                tx.execute(
                    "UPDATE routine_reservations SET tokens=?1,cost=?2 WHERE run=?3",
                    params![
                        (run.summary.accounting.settled_tokens + run.summary.accounting.held_tokens)
                            as i64,
                        (run.summary.accounting.settled_cost_microusd
                            + run.summary.accounting.held_cost_microusd)
                            as i64,
                        run.summary.id.to_string()
                    ],
                )?;
            }
            save_occurrence(&tx, &o)?;
            if !matches!(o.status, OccurrenceStatus::Running) {
                notify(&tx, &r, &o)?;
            }
            tx.commit()?;
        }
        Ok(())
    }
    fn routine_decision(&self, run: &RunView) -> Result<Option<String>, OperationError> {
        for intent in run
            .turns
            .iter()
            .rev()
            .filter_map(|turn| turn.intent.as_ref())
        {
            let task = self.load_task(intent.task)?;
            match task.state {
                TaskState::AwaitingApproval => {
                    return Ok(Some(format!(
                        "Task {} needs exact approval {}. No replacement work is admitted.",
                        task.id, task.approval.id
                    )));
                }
                TaskState::AwaitingAttention => {
                    return Ok(Some(format!(
                        "Task {} has an uncertain result. Review its durable evidence.",
                        task.id
                    )));
                }
                _ => {}
            }
        }
        Ok(match run.summary.phase {
            RunPhase::AwaitingAttention { reason } => Some(format!(
                "Run {} requires attention ({reason:?}). No replacement work is admitted.",
                run.summary.id
            )),
            _ => None,
        })
    }
}
