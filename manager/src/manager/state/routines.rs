mod calendar;
mod commands;
mod ingress;
mod reconcile;
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
#[derive(serde::Serialize, serde::Deserialize)]
struct PublisherBinding {
    id: PublisherId,
    revision: u64,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct RoutineBinding {
    bot: BotId,
    session: ComputerSessionId,
    session_revision: Revision,
    permission_revision: u64,
    publisher: Option<PublisherBinding>,
}
fn binding(
    db: &Connection,
    definition: &RoutineDefinition,
) -> Result<RoutineBinding, OperationError> {
    let publisher = if let RoutineTrigger::Event { publisher, .. } = definition.trigger {
        let body: String = db.query_row(
            "SELECT view FROM routine_publishers WHERE id=?1",
            [publisher.to_string()],
            |r| r.get(0),
        )?;
        let source: PublisherView = parse(body)?;
        Some(PublisherBinding {
            id: publisher,
            revision: source.revision,
        })
    } else {
        None
    };
    Ok(RoutineBinding {
        bot: definition.bot,
        session: definition.template.session,
        session_revision: definition.template.session_revision,
        permission_revision: definition.template.permission_revision,
        publisher,
    })
}
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
    let definition: String = db.query_row(
        "SELECT view FROM routines WHERE id=?1",
        [view.routine.to_string()],
        |r| r.get(0),
    )?;
    let definition: RoutineView = parse(definition)?;
    let binding = binding(db, &definition.definition)?;
    db.execute("INSERT INTO routine_occurrences(id,routine,revision,trigger_key,scheduled_at,run,status,view,binding) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(id) DO UPDATE SET run=excluded.run,status=excluded.status,view=excluded.view",params![view.id.to_string(),view.routine.to_string(),view.revision as i64,view.trigger_key,view.scheduled_at,view.run.map(|r|r.to_string()),status_name(&view.status),json(view)?,json(&binding)?])?;
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
        session: Some(template.session),
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
            .prepare("SELECT view FROM routine_publishers WHERE json_extract(view,'$.revoked')=0 OR id IN (SELECT id FROM routine_publishers WHERE json_extract(view,'$.revoked')=1 ORDER BY rowid DESC LIMIT 16) ORDER BY rowid")?
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
            if definition.template.prompt.len() > 2048 {
                return Err(OperationError::InvalidInput);
            }
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
        self.check_routine_binding(&binding(&self.connection, &routine.definition)?)
    }
    fn check_routine_binding(&self, binding: &RoutineBinding) -> Result<(), OperationError> {
        self.require_live_bot(binding.bot)?;
        let session = self.find_computer_session(binding.session)?;
        let permission = self.permission(binding.bot)?;
        let revoked: bool = self.connection.query_row(
            "SELECT revoked FROM workers WHERE id=?1",
            [session.worker_id.to_string()],
            |r| r.get(0),
        )?;
        if session.bot_id != binding.bot
            || session.revision != binding.session_revision
            || session.owner != InputOwner::Bot
            || revoked
            || !permission.allow
            || permission.revision != binding.permission_revision
        {
            return Err(OperationError::Unauthorized);
        }
        if let Some(publisher) = &binding.publisher {
            let body: String = self.connection.query_row(
                "SELECT view FROM routine_publishers WHERE id=?1",
                [publisher.id.to_string()],
                |row| row.get(0),
            )?;
            let source: PublisherView = parse(body)?;
            if source.revoked || source.bot != binding.bot || source.revision != publisher.revision
            {
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
        let state: String =
            self.connection
                .query_row("SELECT state FROM routines WHERE id=?1", [id], |row| {
                    row.get(0)
                })?;
        Ok(state != "deleted" && self.check_routine_binding(&parse(binding)?).is_ok())
    }
    fn budget_available(
        &self,
        t: &RunTemplate,
        exclude: Option<OccurrenceId>,
    ) -> Result<(), OperationError> {
        let budget = self
            .routine_budget()?
            .ok_or(OperationError::InferenceBudget)?;
        let (runs, tokens, cost): (u64, u64, u64) = self.connection.query_row(
            "SELECT admitted_runs,tokens,cost FROM routine_budget WHERE id=1",
            [],
            |r| {
                Ok((
                    r.get::<_, i64>(0)? as u64,
                    r.get::<_, i64>(1)? as u64,
                    r.get::<_, i64>(2)? as u64,
                ))
            },
        )?;
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
}
