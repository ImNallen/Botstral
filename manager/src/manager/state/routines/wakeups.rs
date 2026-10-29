use super::*;
pub(in crate::manager::state) struct WakeupPlan {
    pub(in crate::manager::state) occurrence: RoutineOccurrence,
    parent: RunId,
    turn: RunTurnId,
    call: String,
    prompt: String,
}
impl State {
    pub(in crate::manager::state) fn run_wakeup_enabled(
        &self,
        run: RunId,
    ) -> Result<bool, OperationError> {
        let id: Option<String> = self
            .connection
            .query_row(
                "SELECT routine FROM routine_occurrences WHERE run=?1",
                [run.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        let Some(id) = id else { return Ok(false) };
        let r = self.load_routine(id.parse().map_err(|_| OperationError::Internal)?)?;
        Ok(r.state == RoutineState::Enabled && r.definition.wakeups.is_some())
    }
    pub(in crate::manager::state) fn prepare_wakeup(
        &mut self,
        parent: RunId,
        turn: RunTurnId,
        call: &str,
        request: &RequestWakeup,
    ) -> Result<WakeupPlan, OperationError> {
        let body: String = self
            .connection
            .query_row(
                "SELECT view FROM routine_occurrences WHERE run=?1",
                [parent.to_string()],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(OperationError::Unauthorized)?;
        let parent_occurrence: RoutineOccurrence = parse(body)?;
        let r = self.load_routine(parent_occurrence.routine)?;
        let grant = r
            .definition
            .wakeups
            .as_ref()
            .ok_or(OperationError::Unauthorized)?;
        if r.state != RoutineState::Enabled || r.revision != parent_occurrence.revision {
            return Err(OperationError::Unauthorized);
        }
        self.check_binding(&r)?;
        let now = self.now();
        if request.prompt.is_empty()
            || request.prompt.len() > MAX_INFERENCE_PROMPT_BYTES
            || request.at < now + i64::from(grant.minimum_spacing_seconds)
            || request.at > now + i64::from(grant.horizon_seconds)
        {
            return Err(OperationError::InvalidInput);
        }
        let (count,pending,last):(u32,u32,Option<i64>)=self.connection.query_row("SELECT count(*),coalesce(sum(CASE WHEN o.status IN ('pending','running','decision') THEN 1 ELSE 0 END),0),max(o.scheduled_at) FROM routine_wakeups w JOIN routine_occurrences o ON o.id=w.occurrence WHERE o.routine=?1",[r.id.to_string()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?)))?;
        if count >= grant.max_total
            || pending >= u32::from(grant.max_pending)
            || last.is_some_and(|last| request.at < last + i64::from(grant.minimum_spacing_seconds))
        {
            return Err(OperationError::InferenceBudget);
        }
        self.budget_available(&r.definition.template, None)?;
        let mut template = r.definition.template.clone();
        template.prompt = request.prompt.clone();
        self.validate_run_admission(materialize(&template, r.definition.bot, now, None))?;
        Ok(WakeupPlan {
            occurrence: RoutineOccurrence {
                id: OccurrenceId::new(),
                routine: r.id,
                revision: r.revision,
                trigger_key: format!("wakeup:{parent}:{turn}:{call}"),
                scheduled_at: request.at,
                run: None,
                status: OccurrenceStatus::Pending,
                skipped_count: 0,
                updated_at: now,
            },
            parent,
            turn,
            call: call.into(),
            prompt: request.prompt.clone(),
        })
    }
    pub(in crate::manager::state) fn commit_wakeup(
        db: &Connection,
        plan: WakeupPlan,
    ) -> Result<(), OperationError> {
        save_occurrence(db, &plan.occurrence)?;
        db.execute(
            "UPDATE routine_occurrences SET prompt=?1 WHERE id=?2",
            params![plan.prompt, plan.occurrence.id.to_string()],
        )?;
        db.execute(
            "INSERT INTO routine_wakeups VALUES(?1,?2,?3,?4)",
            params![
                plan.parent.to_string(),
                plan.turn.to_string(),
                plan.call,
                plan.occurrence.id.to_string()
            ],
        )?;
        Ok(())
    }
}
