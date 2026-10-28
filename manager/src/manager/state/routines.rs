mod calendar;
#[cfg(test)]
mod tests;
mod wakeups;

use super::{
    tasks::{event, json, parse},
    *,
};
use botstral_link::*;
use sha2::{Digest, Sha256};

const BATCH: usize = 16;
fn resolved(phase: &RunPhase) -> bool {
    matches!(
        phase,
        RunPhase::Completed | RunPhase::Cancelled | RunPhase::Stopped { .. }
    )
}
fn state_name(state: RoutineState) -> &'static str {
    match state {
        RoutineState::Enabled => "enabled",
        RoutineState::Paused => "paused",
        RoutineState::Deleted => "deleted",
    }
}
fn status_name(status: &OccurrenceStatus) -> &'static str {
    match status {
        OccurrenceStatus::Pending => "pending",
        OccurrenceStatus::Running => "running",
        OccurrenceStatus::Decision { .. } => "decision",
        OccurrenceStatus::Finished { .. } => "finished",
        OccurrenceStatus::Blocked { .. } => "blocked",
        OccurrenceStatus::Skipped { .. } => "skipped",
    }
}
fn save_routine(db: &Connection, view: &RoutineView) -> Result<(), OperationError> {
    db.execute("INSERT INTO routines VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,state=excluded.state,next_at=excluded.next_at,view=excluded.view",params![view.id.to_string(),view.definition.bot.to_string(),view.revision as i64,state_name(view.state),view.next_at,json(view)?])?;
    event(db, "routine_changed", view.id)
}
fn save_occurrence(db: &Connection, view: &RoutineOccurrence) -> Result<(), OperationError> {
    db.execute("INSERT INTO routine_occurrences(id,routine,revision,trigger_key,scheduled_at,run,status,view,binding) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,(SELECT json_extract(view,'$.definition.template') FROM routines WHERE id=?2)) ON CONFLICT(id) DO UPDATE SET run=excluded.run,status=excluded.status,view=excluded.view",params![view.id.to_string(),view.routine.to_string(),view.revision as i64,view.trigger_key,view.scheduled_at,view.run.map(|r|r.to_string()),status_name(&view.status),json(view)?])?;
    event(db, "routine_occurrence", view.id)
}
fn notify(
    db: &Connection,
    routine: &RoutineView,
    occurrence: &RoutineOccurrence,
) -> Result<(), OperationError> {
    let message = if let Some(run) = occurrence.run {
        db.query_row("SELECT json_extract(view,'$.response.content') FROM run_turns WHERE run=?1 AND json_extract(view,'$.response.content') IS NOT NULL ORDER BY ordinal DESC LIMIT 1",[run.to_string()],|r|r.get::<_,String>(0)).optional()?.unwrap_or_default()
    } else {
        String::new()
    };
    let entry = InboxEntry {
        message: message.chars().take(MAX_RUN_TEXT_BYTES).collect(),
        id: InboxId::new(),
        occurrence: occurrence.id,
        bot: routine.definition.bot,
        routine: routine.id,
        run: occurrence.run,
        status: occurrence.status.clone(),
        created_at: occurrence.updated_at,
        acknowledged: false,
    };
    let inserted = db.execute(
        "INSERT OR IGNORE INTO routine_inbox(id,occurrence,transition,view) VALUES(?1,?2,?3,?4)",
        params![
            entry.id.to_string(),
            entry.occurrence.to_string(),
            json(&entry.status)?,
            json(&entry)?
        ],
    )?;
    if inserted > 0 {
        event(db, "routine_inbox", entry.id)?;
    }
    Ok(())
}
fn materialize(template: &RunTemplate, bot: BotId, now: i64, prompt: Option<&str>) -> SubmitRun {
    let mut policy = template.execution_policy.clone();
    if let Some(vibe) = policy.vibe.as_mut() {
        vibe.limits.deadline += now;
    }
    SubmitRun {
        images: vec![],
        id: RunId::new(),
        bot,
        session: template.session,
        configuration_revision: template.configuration_revision,
        prompt: prompt.unwrap_or(&template.prompt).into(),
        execution_policy: policy,
        limits: RunLimits {
            deadline: now + i64::from(template.duration_seconds),
            max_turns: template.max_turns,
            max_total_tokens: template.max_total_tokens,
            max_cost_microusd: template.max_cost_microusd,
            max_output_tokens: template.max_output_tokens,
            max_operations: template.max_operations,
            max_concurrent_operations: 1,
        },
    }
}
impl State {
    pub(super) fn require_live_bot(&self, bot: BotId) -> Result<(), OperationError> {
        let live:bool=self.connection.query_row("SELECT EXISTS(SELECT 1 FROM bots WHERE id=?1 AND NOT EXISTS(SELECT 1 FROM bot_tombstones WHERE bot=?1))",[bot.to_string()],|r|r.get(0))?;
        if live {
            Ok(())
        } else {
            Err(OperationError::NotFound)
        }
    }
    fn load_routine(&self, id: RoutineId) -> Result<RoutineView, OperationError> {
        let body: Option<String> = self
            .connection
            .query_row(
                "SELECT view FROM routines WHERE id=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        parse(body.ok_or(OperationError::NotFound)?)
    }
    pub(super) fn routine_views(&self) -> Result<Vec<RoutineView>, OperationError> {
        self.connection
            .prepare("SELECT view FROM routines WHERE state!='deleted' ORDER BY rowid LIMIT 128")?
            .query_map([], |r| r.get::<_, String>(0))?
            .map(|r| parse(r?))
            .collect()
    }
    pub(super) fn publisher_views(&self) -> Result<Vec<PublisherView>, OperationError> {
        self.connection
            .prepare("SELECT view FROM routine_publishers ORDER BY rowid LIMIT 32")?
            .query_map([], |r| r.get::<_, String>(0))?
            .map(|r| parse(r?))
            .collect()
    }
    pub(super) fn routine_budget(&self) -> Result<Option<RoutineBudget>, OperationError> {
        self.connection
            .query_row("SELECT view FROM routine_budget", [], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
            .map(parse)
            .transpose()
    }
    pub(super) fn routine_recent(&self) -> Result<Vec<RoutineOccurrence>, OperationError> {
        self.connection
            .prepare("SELECT view FROM routine_occurrences ORDER BY rowid DESC LIMIT 32")?
            .query_map([], |r| r.get::<_, String>(0))?
            .map(|r| parse(r?))
            .collect()
    }
    pub(in crate::manager) fn inbox(&self) -> Result<Vec<InboxEntry>, OperationError> {
        self.connection.prepare("SELECT view,acknowledged FROM routine_inbox WHERE acknowledged=0 ORDER BY sequence LIMIT 64")?.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,bool>(1)?)))?.map(|r| {let (v,a)=r?;let mut v:InboxEntry=parse(v)?;v.acknowledged=a;Ok(v)}).collect()
    }
    pub(in crate::manager) fn routine_history(
        &self,
        token: impl Into<OwnerAuthority>,
        id: RoutineId,
        before: Option<OccurrenceId>,
        limit: u16,
    ) -> Result<Vec<RoutineOccurrence>, OperationError> {
        self.authorize_owner(&token.into())?;
        self.load_routine(id)?;
        if !(1..=MAX_ROUTINE_PAGE as u16).contains(&limit) {
            return Err(OperationError::InvalidInput);
        }
        self.connection.prepare("SELECT view FROM routine_occurrences WHERE routine=?1 AND rowid < coalesce((SELECT rowid FROM routine_occurrences WHERE id=?2),9223372036854775807) ORDER BY rowid DESC LIMIT ?3")?.query_map(params![id.to_string(),before.map(|id|id.to_string()),limit],|r|r.get::<_,String>(0))?.map(|r|parse(r?)).collect()
    }
    pub(in crate::manager) fn routine_preview(
        &self,
        token: impl Into<OwnerAuthority>,
        trigger: RoutineTrigger,
        after: i64,
        limit: u16,
    ) -> Result<Vec<i64>, OperationError> {
        self.authorize_owner(&token.into())?;
        if !(1..=16).contains(&limit) {
            return Err(OperationError::InvalidInput);
        }
        let mut times = Vec::new();
        let mut after = after;
        for _ in 0..limit {
            let Some(at) = calendar::next(&trigger, after)? else {
                break;
            };
            times.push(at);
            after = at;
        }
        Ok(times)
    }
    fn validate_definition(&self, definition: &RoutineDefinition) -> Result<(), OperationError> {
        self.require_live_bot(definition.bot)?;
        calendar::validate(&definition.trigger)?;
        let t = &definition.template;
        if definition.name.trim().is_empty()
            || definition.name.len() > 100
            || json(definition)?.len() > MAX_ROUTINE_BYTES
            || definition.catchup_seconds > 86400
            || !(1..=MAX_RUN_SECONDS as u32).contains(&t.duration_seconds)
            || t.prompt.trim().is_empty()
            || t.prompt.len() > MAX_INFERENCE_PROMPT_BYTES
            || t.configuration_revision == 0
            || t.permission_revision > MAX_COUNTER
            || !(1..=MAX_RUN_TURNS).contains(&t.max_turns)
            || !(1..=MAX_INFERENCE_OUTPUT_TOKENS).contains(&t.max_output_tokens)
            || t.max_total_tokens == 0
            || t.max_total_tokens > MAX_COUNTER
            || t.max_cost_microusd == 0
            || t.max_cost_microusd > MAX_COUNTER
            || !(1..=96).contains(&t.max_operations)
        {
            return Err(OperationError::InvalidInput);
        }
        if t.execution_policy.vibe.as_ref().is_some_and(|v| {
            v.limits.deadline <= 0 || v.limits.deadline > i64::from(t.duration_seconds)
        }) {
            return Err(OperationError::InvalidInput);
        }
        let request = materialize(t, definition.bot, self.now(), None);
        crate::manager::runs::tools::validate_policy(
            &request.execution_policy,
            request.limits.deadline,
        )?;
        if let Some(grant) = &definition.wakeups
            && (grant.minimum_spacing_seconds < 60
                || grant.horizon_seconds < grant.minimum_spacing_seconds
                || grant.horizon_seconds > 604800
                || !(1..=16).contains(&grant.max_pending)
                || !(1..=1000).contains(&grant.max_total))
        {
            return Err(OperationError::InvalidInput);
        }
        if let RoutineTrigger::Event {
            publisher, topic, ..
        } = &definition.trigger
        {
            let source = self
                .publisher_views()?
                .into_iter()
                .find(|p| p.id == *publisher)
                .ok_or(OperationError::NotFound)?;
            if source.revoked || source.bot != definition.bot || !source.topics.contains(topic) {
                return Err(OperationError::Unauthorized);
            }
        }
        Ok(())
    }
    fn check_binding(&self, routine: &RoutineView) -> Result<(), OperationError> {
        self.require_live_bot(routine.definition.bot)?;
        let t = &routine.definition.template;
        let session = self.find_computer_session(t.session)?;
        let permission = self.permission(routine.definition.bot)?;
        let revoked: bool = self.connection.query_row(
            "SELECT revoked FROM workers WHERE id=?1",
            [session.worker_id.to_string()],
            |r| r.get(0),
        )?;
        if session.bot_id != routine.definition.bot
            || session.revision != t.session_revision
            || session.owner != InputOwner::Bot
            || revoked
            || !permission.allow
            || permission.revision != t.permission_revision
        {
            return Err(OperationError::Unauthorized);
        }
        if let RoutineTrigger::Event { publisher, .. } = routine.definition.trigger {
            let source = self
                .publisher_views()?
                .into_iter()
                .find(|p| p.id == publisher)
                .ok_or(OperationError::NotFound)?;
            if source.revoked || source.bot != routine.definition.bot {
                return Err(OperationError::Unauthorized);
            }
        }
        Ok(())
    }
    pub(super) fn routine_run_authorized(&self, id: RunId) -> Result<bool, OperationError> {
        let stored: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT routine,binding FROM routine_occurrences WHERE run=?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((id, binding)) = stored else {
            return Ok(true);
        };
        let mut routine = self.load_routine(id.parse().map_err(|_| OperationError::Internal)?)?;
        routine.definition.template = parse(binding)?;
        Ok(routine.state != RoutineState::Deleted && self.check_binding(&routine).is_ok())
    }
    fn budget_available(
        &self,
        t: &RunTemplate,
        exclude: Option<OccurrenceId>,
    ) -> Result<(), OperationError> {
        let budget = self
            .routine_budget()?
            .ok_or(OperationError::InferenceBudget)?;
        let (runs,tokens,cost):(u64,u64,u64)=self.connection.query_row("SELECT count(*),coalesce(sum(tokens),0),coalesce(sum(cost),0) FROM routine_reservations",[],|r|Ok((r.get::<_,i64>(0)? as u64,r.get::<_,i64>(1)? as u64,r.get::<_,i64>(2)? as u64)))?;
        let mut pending_runs = 0u64;
        let mut pending_tokens = 0u64;
        let mut pending_cost = 0u64;
        for row in self.connection.prepare("SELECT o.id,r.view FROM routine_occurrences o JOIN routines r ON r.id=o.routine WHERE o.status='pending'")?.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))? {
            let (id,body)=row?;
            if exclude.is_some_and(|e|e.to_string()==id) {continue;}
            let r:RoutineView=parse(body)?;
            pending_runs+=1;pending_tokens+=r.definition.template.max_total_tokens;pending_cost+=r.definition.template.max_cost_microusd;
        }
        if runs + pending_runs + 1 > u64::from(budget.max_runs)
            || tokens + pending_tokens + t.max_total_tokens > budget.max_tokens
            || cost + pending_cost + t.max_cost_microusd > budget.max_cost_microusd
        {
            Err(OperationError::InferenceBudget)
        } else {
            Ok(())
        }
    }
    fn prepare_occurrence(
        &self,
        routine: &RoutineView,
        key: String,
        at: i64,
        skipped: u32,
        prompt: Option<&str>,
        pending: Option<OccurrenceId>,
    ) -> Result<(RoutineOccurrence, Option<RunView>), OperationError> {
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
            let overlap:bool=self.connection.query_row("SELECT EXISTS(SELECT 1 FROM routine_occurrences o JOIN runs r ON r.id=o.run WHERE o.routine=?1 AND json_extract(r.summary,'$.phase.type') NOT IN ('completed','cancelled','stopped'))",[routine.id.to_string()],|r|r.get(0))?;
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
        occurrence.run = run.as_ref().map(|r| r.summary.id);
        Ok((occurrence, run))
    }
    fn commit_occurrence(
        db: &Connection,
        owner: OwnerId,
        routine: &RoutineView,
        occurrence: &RoutineOccurrence,
        run: Option<&RunView>,
    ) -> Result<(), OperationError> {
        if let Some(run) = run {
            super::runs::insert_run(db, owner, run)?;
            db.execute(
                "INSERT INTO routine_reservations VALUES(?1,?2,?3,?4)",
                params![
                    run.summary.id.to_string(),
                    routine.id.to_string(),
                    run.request.limits.max_total_tokens as i64,
                    run.request.limits.max_cost_microusd as i64
                ],
            )?;
        }
        save_occurrence(db, occurrence)?;
        if run.is_none() {
            notify(db, routine, occurrence)?;
        }
        Ok(())
    }
    pub(in crate::manager) fn routine_command(
        &mut self,
        token: impl Into<OwnerAuthority>,
        input: RoutineCommand,
    ) -> Result<RoutineControlResult, OperationError> {
        self.authorize_owner(&token.into())?;
        if json(&input)?.len() > MAX_ROUTINE_BYTES + 2048 {
            return Err(OperationError::InvalidInput);
        }
        let previous: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT request,result FROM routine_commands WHERE id=?1",
                [input.id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((body, result)) = previous {
            return if body == json(&input)? {
                parse(result)
            } else {
                Err(OperationError::Conflict)
            };
        }
        let now = self.now();
        let owner = self.principal()?;
        let tx = self.connection.unchecked_transaction()?;
        let message = match &input.action {
            RoutineAction::Create { id, definition } => {
                self.validate_definition(definition)?;
                tasks::count_bound(&tx, "routines", MAX_ROUTINES)?;
                if self.load_routine(*id).is_ok() {
                    return Err(OperationError::Conflict);
                }
                let next_at = match definition.trigger {
                    RoutineTrigger::Once { at } => Some(at),
                    _ => calendar::next(&definition.trigger, now - 1)?,
                };
                save_routine(
                    &tx,
                    &RoutineView {
                        id: *id,
                        revision: 1,
                        state: RoutineState::Enabled,
                        definition: *definition.clone(),
                        next_at,
                    },
                )?;
                "Routine created."
            }
            RoutineAction::Replace {
                id,
                expected_revision,
                definition,
            } => {
                self.validate_definition(definition)?;
                let mut r = self.load_routine(*id)?;
                if r.revision != *expected_revision
                    || r.state == RoutineState::Deleted
                    || r.definition.bot != definition.bot
                {
                    return Err(OperationError::Conflict);
                }
                self.cancel_pending(&tx, *id, "Definition replaced.")?;
                r.revision += 1;
                r.definition = *definition.clone();
                r.next_at = calendar::next(&r.definition.trigger, now)?;
                save_routine(&tx, &r)?;
                "Routine updated."
            }
            RoutineAction::SetPaused {
                id,
                expected_revision,
                paused,
            } => {
                let mut r = self.load_routine(*id)?;
                if r.revision != *expected_revision || r.state == RoutineState::Deleted {
                    return Err(OperationError::Conflict);
                }
                self.cancel_pending(&tx, *id, "Routine paused or resumed.")?;
                r.revision += 1;
                r.state = if *paused {
                    RoutineState::Paused
                } else {
                    RoutineState::Enabled
                };
                r.next_at = if *paused {
                    None
                } else {
                    calendar::next(&r.definition.trigger, now)?
                };
                save_routine(&tx, &r)?;
                if *paused {
                    "Routine paused."
                } else {
                    "Routine resumed without backfill."
                }
            }
            RoutineAction::Delete {
                id,
                expected_revision,
            } => {
                let mut r = self.load_routine(*id)?;
                if r.revision != *expected_revision {
                    return Err(OperationError::Conflict);
                }
                self.cancel_pending(&tx, *id, "Routine deleted.")?;
                r.revision += 1;
                r.state = RoutineState::Deleted;
                r.next_at = None;
                save_routine(&tx, &r)?;
                "Routine deleted."
            }
            RoutineAction::Test {
                id,
                expected_revision,
            } => {
                let r = self.load_routine(*id)?;
                if r.revision != *expected_revision {
                    return Err(OperationError::Conflict);
                }
                let (o, run) =
                    self.prepare_occurrence(&r, format!("test:{}", input.id), now, 0, None, None)?;
                Self::commit_occurrence(&tx, owner, &r, &o, run.as_ref())?;
                "Live bounded test recorded."
            }
            RoutineAction::SetBudget { budget } => {
                if budget.max_runs == 0
                    || budget.max_runs > 1_000_000
                    || budget.max_tokens > MAX_COUNTER
                    || budget.max_cost_microusd > MAX_COUNTER
                {
                    return Err(OperationError::InvalidInput);
                }
                tx.execute("INSERT INTO routine_budget VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET view=excluded.view",[json(budget)?])?;
                event(&tx, "routine_budget", "owner")?;
                "Cumulative owner budget updated. Spent amounts remain counted."
            }
            RoutineAction::InstallPublisher {
                id,
                bot,
                name,
                topics,
                credential_sha256,
            } => {
                self.require_live_bot(*bot)?;
                tasks::count_bound(&tx, "routine_publishers", MAX_PUBLISHERS)?;
                if name.is_empty()
                    || name.len() > 100
                    || topics.is_empty()
                    || topics.len() > 16
                    || topics.iter().any(|t| !calendar::name(t))
                    || credential_sha256.len() != 64
                    || !credential_sha256
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                {
                    return Err(OperationError::InvalidInput);
                }
                let credential = (0..32)
                    .map(|i| {
                        u8::from_str_radix(&credential_sha256[i * 2..i * 2 + 2], 16)
                            .map_err(|_| OperationError::InvalidInput)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let p = PublisherView {
                    id: *id,
                    bot: *bot,
                    name: name.clone(),
                    topics: topics.clone(),
                    revision: 1,
                    revoked: false,
                    last_received_at: None,
                    last_error: None,
                };
                tx.execute("INSERT INTO routine_publishers(id,bot,credential_hash,view) VALUES(?1,?2,?3,?4)",params![id.to_string(),bot.to_string(),credential,json(&p)?])?;
                event(&tx, "routine_publisher", id)?;
                "Publisher installed. Use a random bearer credential with at least 32 bytes."
            }
            RoutineAction::RevokePublisher {
                id,
                expected_revision,
            } => {
                let mut p = self
                    .publisher_views()?
                    .into_iter()
                    .find(|p| p.id == *id)
                    .ok_or(OperationError::NotFound)?;
                if p.revision != *expected_revision {
                    return Err(OperationError::Conflict);
                }
                p.revision += 1;
                p.revoked = true;
                tx.execute("UPDATE routine_publishers SET view=?1,credential_hash=randomblob(32) WHERE id=?2",params![json(&p)?,id.to_string()])?;
                event(&tx, "routine_publisher", id)?;
                "Publisher revoked."
            }
            RoutineAction::AcknowledgeInbox { id } => {
                if tx.execute(
                    "UPDATE routine_inbox SET acknowledged=1 WHERE id=?1",
                    [id.to_string()],
                )? == 0
                {
                    return Err(OperationError::NotFound);
                }
                event(&tx, "routine_inbox", id)?;
                "Inbox entry acknowledged."
            }
            RoutineAction::DeleteBot { bot } => {
                self.require_live_bot(*bot)?;
                tx.execute(
                    "INSERT INTO bot_tombstones VALUES(?1,?2)",
                    params![bot.to_string(), now],
                )?;
                for mut r in self
                    .routine_views()?
                    .into_iter()
                    .filter(|r| r.definition.bot == *bot)
                {
                    self.cancel_pending(&tx, r.id, "Bot deleted.")?;
                    r.revision += 1;
                    r.state = RoutineState::Deleted;
                    r.next_at = None;
                    save_routine(&tx, &r)?;
                }
                for mut p in self
                    .publisher_views()?
                    .into_iter()
                    .filter(|p| p.bot == *bot)
                {
                    p.revoked = true;
                    p.revision += 1;
                    tx.execute("UPDATE routine_publishers SET view=?1,credential_hash=randomblob(32) WHERE id=?2",params![json(&p)?,p.id.to_string()])?;
                }
                let tasks = tx
                    .prepare("SELECT view FROM tasks")?
                    .query_map([], |r| r.get::<_, String>(0))?
                    .map(|r| parse::<TaskView>(r?))
                    .collect::<Result<Vec<_>, _>>()?;
                for mut task in tasks
                    .into_iter()
                    .filter(|t| t.command.bot == *bot && !t.state.terminal())
                {
                    task.state = TaskState::Cancelling;
                    tx.execute(
                        "INSERT OR IGNORE INTO cancellations VALUES(?1,?2)",
                        params![task.id.to_string(), CommandId::new().to_string()],
                    )?;
                    tasks::save(&tx, &task)?;
                    event(&tx, "cancellation_requested", task.id)?;
                }
                for summary in self
                    .run_summaries()?
                    .into_iter()
                    .filter(|r| r.bot == *bot && !resolved(&r.phase))
                {
                    let mut summary = summary;
                    summary.cancellation_requested_at.get_or_insert(now);
                    summary.updated_at = now;
                    super::runs::save_summary(&tx, &summary)?;
                }
                event(&tx, "bot_deleted", bot)?;
                "Bot deleted. Active work is stopping; dispatched effects still require reconciliation."
            }
            RoutineAction::DuplicateBot { bot, copy, name } => {
                self.require_live_bot(*bot)?;
                if name.trim().is_empty() || name.len() > 100 {
                    return Err(OperationError::InvalidInput);
                }
                tasks::count_bound(&tx, "bots", MAX_WORKSPACE_BOTS)?;
                tx.execute(
                    "INSERT INTO bots VALUES(?1,?2,?3)",
                    params![copy.to_string(), name, now],
                )?;
                for r in self
                    .routine_views()?
                    .into_iter()
                    .filter(|r| r.definition.bot == *bot)
                {
                    tasks::count_bound(&tx, "routines", MAX_ROUTINES)?;
                    let mut definition = r.definition;
                    definition.bot = *copy;
                    definition.template.session = ComputerSessionId::new();
                    definition.template.session_revision = Revision::INITIAL;
                    definition.template.configuration_revision = 0;
                    definition.template.permission_revision = 0;
                    definition.wakeups = None;
                    save_routine(
                        &tx,
                        &RoutineView {
                            id: RoutineId::new(),
                            revision: 1,
                            state: RoutineState::Paused,
                            definition,
                            next_at: None,
                        },
                    )?;
                }
                event(&tx, "bot_created", copy)?;
                "Bot duplicated with paused routines. Configure new bindings and grants before activation."
            }
        };
        let result = RoutineControlResult {
            request: input.id,
            message: message.into(),
        };
        tx.execute(
            "INSERT INTO routine_commands VALUES(?1,?2,?3)",
            params![input.id.to_string(), json(&input)?, json(&result)?],
        )?;
        tx.commit()?;
        self.fault("routine_command_committed");
        self.fence_deleted_bots()?;
        Ok(result)
    }
    fn cancel_pending(
        &self,
        db: &Connection,
        id: RoutineId,
        reason: &str,
    ) -> Result<(), OperationError> {
        let r = self.load_routine(id)?;
        let rows = db
            .prepare("SELECT view FROM routine_occurrences WHERE routine=?1 AND status='pending'")?
            .query_map([id.to_string()], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        for body in rows {
            let mut o: RoutineOccurrence = parse(body)?;
            o.status = OccurrenceStatus::Skipped {
                reason: reason.into(),
            };
            o.updated_at = self.now();
            save_occurrence(db, &o)?;
            notify(db, &r, &o)?;
        }
        Ok(())
    }
    fn fence_deleted_bots(&mut self) -> Result<(), OperationError> {
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
    fn compact_run_transcripts(&self) -> Result<(), OperationError> {
        self.connection.execute("DELETE FROM run_turns WHERE run IN (SELECT id FROM runs WHERE json_extract(summary,'$.phase.type') IN ('completed','cancelled','stopped') AND id NOT IN (SELECT id FROM runs WHERE json_extract(summary,'$.phase.type') IN ('completed','cancelled','stopped') ORDER BY rowid DESC LIMIT 16) AND id IN (SELECT run FROM run_turns) ORDER BY rowid LIMIT 16) AND run NOT IN (SELECT run FROM run_children) AND run NOT IN (SELECT parent_run FROM routine_wakeups) AND run NOT IN (SELECT run FROM routine_occurrences WHERE status IN ('running','decision'))",[])?;
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
            } else if matches!(run.summary.phase, RunPhase::AwaitingAttention { .. })
                || run.turns.iter().filter_map(|t| t.intent.as_ref()).any(|i| {
                    self.load_task(i.task).is_ok_and(|t| {
                        t.state == TaskState::AwaitingApproval
                            || t.state == TaskState::AwaitingAttention
                    })
                })
            {
                OccurrenceStatus::Decision {
                    reason: "Run requires an owner decision. No replacement work is admitted."
                        .into(),
                }
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
    pub(in crate::manager) fn ingest_routine_event(
        &mut self,
        publisher: PublisherId,
        credential: String,
        bytes: Vec<u8>,
    ) -> Result<EventReceipt, OperationError> {
        if bytes.len() > MAX_EVENT_BYTES || !(43..=256).contains(&credential.len()) {
            return Err(OperationError::InvalidInput);
        }
        let (hash,body,start,count):(Vec<u8>,String,i64,u32)=self.connection.query_row("SELECT credential_hash,view,window_start,received FROM routine_publishers WHERE id=?1",[publisher.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?.ok_or(OperationError::Unauthorized)?;
        let mut source: PublisherView = parse(body)?;
        let digest = Sha256::digest(credential.as_bytes());
        if !bool::from(hash.ct_eq(digest.as_slice()))
            || source.revoked
            || self.require_live_bot(source.bot).is_err()
        {
            return Err(OperationError::Unauthorized);
        }
        let result = (|| {
            self.provider_secret()?;
            let now = self.now();
            let count = if now - start >= 60 { 0 } else { count };
            if count >= 60 {
                return Err(OperationError::RateLimited);
            }
            self.connection.execute(
                "UPDATE routine_publishers SET window_start=?1,received=?2 WHERE id=?3",
                params![
                    if count == 0 { now } else { start },
                    count + 1,
                    publisher.to_string()
                ],
            )?;
            let event: PublisherEvent =
                serde_json::from_slice(&bytes).map_err(|_| OperationError::InvalidInput)?;
            if !calendar::name(&event.id)
                || event.fields.len() > 16
                || event.fields.iter().any(|(k, v)| {
                    !calendar::name(k) || json(v).is_err() || json(v).is_ok_and(|b| b.len() > 512)
                })
                || !source.topics.contains(&event.topic)
                || event.timestamp < now - EVENT_REPLAY_SECONDS
                || event.timestamp > now + 300
            {
                return Err(OperationError::InvalidInput);
            }
            let digest = Sha256::digest(&bytes).to_vec();
            let existing:Option<(Vec<u8>,String)>=self.connection.query_row("SELECT digest,receipt FROM routine_event_receipts WHERE publisher=?1 AND event_id=?2",params![publisher.to_string(),event.id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
            if let Some((old, receipt)) = existing {
                return if old == digest {
                    parse(receipt)
                } else {
                    Err(OperationError::Conflict)
                };
            }
            let owner = self.principal()?;
            let tx = self.connection.unchecked_transaction()?;
            let mut matched = 0;
            for r in self
                .routine_views()?
                .into_iter()
                .filter(|r| r.state == RoutineState::Enabled)
            {
                let RoutineTrigger::Event {
                    publisher: p,
                    topic,
                    equals,
                } = &r.definition.trigger
                else {
                    continue;
                };
                if *p != publisher
                    || topic != &event.topic
                    || !equals.iter().all(|(k, v)| event.fields.get(k) == Some(v))
                {
                    continue;
                }
                let key = format!("event:{publisher}:{}", event.id);
                let (o, run) = self.prepare_occurrence(&r, key, now, 0, None, None)?;
                Self::commit_occurrence(&tx, owner, &r, &o, run.as_ref())?;
                matched += 1;
            }
            let receipt = EventReceipt {
                publisher,
                event_id: event.id,
                matched,
            };
            tx.execute(
                "INSERT INTO routine_event_receipts VALUES(?1,?2,?3,?4,?5)",
                params![
                    publisher.to_string(),
                    receipt.event_id,
                    digest,
                    event.timestamp,
                    json(&receipt)?
                ],
            )?;
            source.last_received_at = Some(now);
            source.last_error = None;
            tx.execute(
                "UPDATE routine_publishers SET view=?1 WHERE id=?2",
                params![json(&source)?, publisher.to_string()],
            )?;
            event_fn(&tx, publisher)?;
            tx.commit()?;
            self.fault("routine_event_committed");
            Ok(receipt)
        })();
        if let Err(error) = &result {
            self.record_source_failure(&mut source, error)?;
        }
        result
    }
    fn record_source_failure(
        &self,
        source: &mut PublisherView,
        error: &OperationError,
    ) -> Result<(), OperationError> {
        source.last_error = Some(format!("{error:?}"));
        let tx = self.connection.unchecked_transaction()?;
        tx.execute(
            "UPDATE routine_publishers SET view=?1 WHERE id=?2",
            params![json(source)?, source.id.to_string()],
        )?;
        let key = format!("source-health:{}:{}", source.id, self.now() / 60);
        for r in self.routine_views()?.into_iter().filter(|r|r.state==RoutineState::Enabled && matches!(&r.definition.trigger,RoutineTrigger::Event {publisher,..} if *publisher==source.id)) {
            let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM routine_occurrences WHERE routine=?1 AND trigger_key=?2)",params![r.id.to_string(),key],|row|row.get(0))?;
            if exists {continue;}
            let o=RoutineOccurrence {id:OccurrenceId::new(),routine:r.id,revision:r.revision,trigger_key:key.clone(),scheduled_at:self.now(),run:None,status:OccurrenceStatus::Blocked {reason:format!("Publisher ingress failed: {error:?}")},skipped_count:0,updated_at:self.now()};
            save_occurrence(&tx,&o)?;notify(&tx,&r,&o)?;
        }
        event(&tx, "routine_publisher", source.id)?;
        tx.commit()?;
        Ok(())
    }
}
fn event_fn(db: &Connection, id: PublisherId) -> Result<(), OperationError> {
    event(db, "routine_event", id)
}
impl State {
    pub(in crate::manager) fn routine_status(
        &self,
        authority: impl Into<OwnerAuthority>,
        id: RequestId,
    ) -> Result<Option<RoutineControlResult>, OperationError> {
        self.authorize_owner(&authority.into())?;
        self.connection
            .query_row(
                "SELECT result FROM routine_commands WHERE id=?1",
                [id.to_string()],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .map(parse)
            .transpose()
    }
    pub(in crate::manager) fn routine_conversation(
        &mut self,
        token: impl Into<OwnerAuthority>,
        bot: BotId,
        id: RequestId,
        text: String,
    ) -> Result<RoutineControlResult, OperationError> {
        let authority = token.into();
        self.authorize_owner(&authority)?;
        self.require_live_bot(bot)?;
        if text == "/routine help" {
            return Ok(RoutineControlResult {request:id,message:"Owner commands: /routine create <RoutineDefinition JSON>; /routine edit <id> <revision> <RoutineDefinition JSON>; /routine pause|resume|test|delete <id> <revision>; /routine history <id>; /routine list. Test runs real bounded work. App Routines shows the same definitions, schedule preview, and inbox. Publisher subscriptions support authenticated named webhook topics only.".into()});
        }
        if text == "/routine list" {
            return Ok(RoutineControlResult {
                request: id,
                message: json(
                    &self
                        .routine_views()?
                        .into_iter()
                        .filter(|r| r.definition.bot == bot)
                        .collect::<Vec<_>>(),
                )?,
            });
        }
        let rest = text
            .strip_prefix("/routine ")
            .ok_or(OperationError::InvalidInput)?;
        let (verb, args) = rest.split_once(' ').ok_or(OperationError::InvalidInput)?;
        if verb == "history" {
            let routine: RoutineId = args.parse().map_err(|_| OperationError::InvalidInput)?;
            if self.load_routine(routine)?.definition.bot != bot {
                return Err(OperationError::Unauthorized);
            }
            return Ok(RoutineControlResult {
                request: id,
                message: json(&self.routine_history(authority, routine, None, 32)?)?,
            });
        }
        let action = if verb == "create" {
            let definition: RoutineDefinition =
                serde_json::from_str(args).map_err(|_| OperationError::InvalidInput)?;
            if definition.bot != bot {
                return Err(OperationError::Unauthorized);
            }
            // The request identifier also identifies a retried conversation creation.
            RoutineAction::Create {
                id: RoutineId(id.0),
                definition: Box::new(definition),
            }
        } else {
            let mut parts = args.splitn(3, ' ');
            let routine: RoutineId = parts
                .next()
                .ok_or(OperationError::InvalidInput)?
                .parse()
                .map_err(|_| OperationError::InvalidInput)?;
            let revision: u64 = parts
                .next()
                .ok_or(OperationError::InvalidInput)?
                .parse()
                .map_err(|_| OperationError::InvalidInput)?;
            if self.load_routine(routine)?.definition.bot != bot {
                return Err(OperationError::Unauthorized);
            }
            match verb {
                "pause" | "resume" => RoutineAction::SetPaused {
                    id: routine,
                    expected_revision: revision,
                    paused: verb == "pause",
                },
                "test" => RoutineAction::Test {
                    id: routine,
                    expected_revision: revision,
                },
                "delete" => RoutineAction::Delete {
                    id: routine,
                    expected_revision: revision,
                },
                "edit" => {
                    let definition: RoutineDefinition =
                        serde_json::from_str(parts.next().ok_or(OperationError::InvalidInput)?)
                            .map_err(|_| OperationError::InvalidInput)?;
                    if definition.bot != bot {
                        return Err(OperationError::Unauthorized);
                    }
                    RoutineAction::Replace {
                        id: routine,
                        expected_revision: revision,
                        definition: Box::new(definition),
                    }
                }
                _ => return Err(OperationError::InvalidInput),
            }
        };
        self.routine_command(authority, RoutineCommand { id, action })
    }
}
