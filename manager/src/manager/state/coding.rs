use super::{
    tasks::{event, json, parse},
    *,
};
use crate::manager::{
    coding::{CodingAdmission, CodingPermit, CodingRequest, completion},
    provider::{EnvelopeOutcome, reflects_secret},
};
use botstral_link::*;
use serde_json::Value;

impl State {
    pub(super) fn validate_coding_task(
        &self,
        bot: BotId,
        worker: WorkerId,
        id: TaskId,
        spec: &VibeSpec,
    ) -> Result<(), OperationError> {
        spec.validate()?;
        if spec.limits.deadline <= self.now() || spec.limits.deadline > self.now() + MAX_RUN_SECONDS
        {
            return Err(OperationError::InvalidInput);
        }
        let configuration =
            self.load_inference_configuration(bot, spec.policy.configuration_revision)?;
        if !self.tools_authorized(&configuration)?
            || configuration.policy.model_id != spec.policy.model_id
            || spec.limits.deadline > configuration.policy.prices.expires_at
        {
            return Err(OperationError::InferenceUnverified);
        }
        let reservation = reserve(&configuration, spec)?;
        if reservation.total_tokens > spec.limits.max_total_tokens
            || reservation.cost_microusd > spec.limits.max_cost_microusd
        {
            return Err(OperationError::InferenceBudget);
        }
        if reflects_secret(
            &serde_json::to_value(spec).map_err(|_| OperationError::Internal)?,
            self.provider_secret()?.expose_secret(),
        ) {
            return Err(OperationError::InvalidInput);
        }
        let tasks = self
            .connection
            .prepare("SELECT view FROM tasks ORDER BY rowid")?
            .query_map([], |r| r.get::<_, String>(0))?
            .map(|r| parse::<TaskView>(r?))
            .collect::<Result<Vec<_>, OperationError>>()?;
        let mut previous = None;
        for task in &tasks {
            if task.id == id {
                continue;
            }
            if let WorkSpec::Vibe { spec: other } = &task.command.work {
                let a = &spec.policy.workspace;
                let b = &other.policy.workspace;
                if a.id == b.id {
                    if task.command.bot != bot
                        || task.command.worker != worker
                        || spec.policy != other.policy
                        || !task.state.terminal()
                        || self.task_calls(task.id)?.iter().any(|c| {
                            c.accounting == InferenceAccounting::Held
                                || c.phase == CodingCallPhase::Unknown
                        })
                    {
                        return Err(OperationError::Conflict);
                    }
                    previous = Some(task);
                } else if task.command.worker == worker
                    && (a.path == b.path
                        || a.path.starts_with(&(b.path.clone() + "/"))
                        || b.path.starts_with(&(a.path.clone() + "/")))
                {
                    return Err(OperationError::Conflict);
                }
            }
        }
        match (&spec.session, previous) {
            (VibeSession::New, None) => Ok(()),
            (
                VibeSession::Resume {
                    task,
                    native_session,
                },
                Some(previous),
            ) if *task == previous.id => {
                let result = coding_result(previous)?;
                if result.native_session != *native_session {
                    return Err(OperationError::Conflict);
                }
                Ok(())
            }
            _ => Err(OperationError::Conflict),
        }
    }
    fn task_calls(&self, task: TaskId) -> Result<Vec<CodingCallView>, OperationError> {
        self.connection
            .prepare("SELECT view FROM coding_calls WHERE task=?1 ORDER BY rowid")?
            .query_map([task.to_string()], |r| r.get::<_, String>(0))?
            .map(|r| parse(r?))
            .collect()
    }
    pub(in crate::manager) fn coding_calls(
        &self,
        token: SessionToken,
        task: TaskId,
    ) -> Result<Vec<CodingCallView>, OperationError> {
        self.authorize(&token)?;
        self.load_task(task)?;
        self.task_calls(task)
    }
    fn coding_authority(
        &self,
        task: &TaskView,
        generation: Generation,
    ) -> Result<VibeSpec, OperationError> {
        let WorkSpec::Vibe { spec } = &task.command.work else {
            return Err(OperationError::InvalidInput);
        };
        let binding = task
            .approval
            .binding
            .as_ref()
            .ok_or(OperationError::Unauthorized)?;
        let leased: bool = self.connection.query_row("SELECT EXISTS(SELECT 1 FROM coding_leases WHERE task=?1 AND execution=?2 AND generation=?3 AND closed=0)",
            params![task.id.to_string(),task.command.execution.to_string(),generation.get() as i64], |r| r.get(0))?;
        let policy = self.permission(task.command.bot)?;
        let session = self.find_computer_session(task.command.session)?;
        if self
            .inference_control
            .as_ref()
            .is_some_and(|c| c.stopping.load(std::sync::atomic::Ordering::Acquire))
            || !leased
            || !matches!(task.state, TaskState::Running)
            || !task.approval.consumed
            || task.approval.revoked
            || !task.command.matches_authority(binding)
            || !policy.allow
            || policy.revision != binding.permission_revision
            || binding.expires_at <= self.now()
            || spec.limits.deadline <= self.now()
            || !self
                .worker_connections
                .get(&task.command.worker)
                .is_some_and(|c| c.recovered && c.generation == generation)
            || session.owner != InputOwner::Bot
            || session.status != ControlStatus::Confirmed
        {
            return Err(OperationError::Unauthorized);
        }
        let config = self
            .load_inference_configuration(task.command.bot, spec.policy.configuration_revision)?;
        if !self.tools_authorized(&config)? {
            return Err(OperationError::InferenceUnverified);
        }
        self.provider_secret()?;
        if let Some(parent) = self.coding_parent(task.id)?
            && (parent.summary.cancellation_requested_at.is_some()
                || parent.request.limits.deadline <= self.now()
                || !matches!(parent.summary.phase, RunPhase::WaitingForTool { task: child, .. } if child == task.id))
        {
            return Err(OperationError::Unauthorized);
        }
        Ok(*spec.clone())
    }
    fn coding_parent(&self, task: TaskId) -> Result<Option<RunView>, OperationError> {
        let run: Option<String> = self
            .connection
            .query_row(
                "SELECT run FROM run_children WHERE task=?1",
                [task.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        run.map(|id| self.load_run(id.parse().map_err(|_| OperationError::Internal)?))
            .transpose()
    }
    pub(in crate::manager) fn admit_coding(
        &mut self,
        peer: crate::worker_http::VerifiedIdentity,
        task: TaskId,
        execution: ExecutionId,
        id: InferenceAttemptId,
        request: CodingRequest,
    ) -> Result<CodingAdmission, OperationError> {
        let task = self.load_task(task)?;
        let valid: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM workers WHERE id=?1 AND fingerprint=?2 AND revoked=0)",
            params![task.command.worker.to_string(), peer.fingerprint().as_str()],
            |r| r.get(0),
        )?;
        if !valid || task.command.execution != execution {
            return Err(OperationError::Unauthorized);
        }
        let generation = self
            .worker_connections
            .get(&task.command.worker)
            .ok_or(OperationError::Unauthorized)?
            .generation;
        let spec = self.coding_authority(&task, generation)?;
        let prepared = request.prepare(&spec)?;
        let encoded = prepared.encoded();
        let previous: Option<(String, String, String, Option<String>)> = self
            .connection
            .query_row(
                "SELECT task,request,view,response FROM coding_calls WHERE id=?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        if let Some((owner, request, view, response)) = previous {
            let view: CodingCallView = parse(view)?;
            if owner != task.id.to_string() || request != encoded {
                return Err(OperationError::Conflict);
            }
            return if view.phase == CodingCallPhase::Settled {
                Ok(CodingAdmission::Cached(parse(
                    response.ok_or(OperationError::Conflict)?,
                )?))
            } else {
                Err(OperationError::Conflict)
            };
        }
        let calls = self.task_calls(task.id)?;
        if calls.iter().any(|c| {
            c.accounting == InferenceAccounting::Held || c.phase == CodingCallPhase::Unknown
        }) {
            return Err(OperationError::Conflict);
        }
        let configuration = self
            .load_inference_configuration(task.command.bot, spec.policy.configuration_revision)?;
        let reservation = reserve(&configuration, &spec)?;
        let mut accounting = RunAccounting::default();
        for call in &calls {
            accounting.admitted_coding_calls += 1;
            if let (Some(usage), InferenceAccounting::Settled { charged_microusd }) =
                (&call.usage, &call.accounting)
            {
                accounting.settled_tokens += usage.total_tokens;
                accounting.settled_cost_microusd += charged_microusd;
            }
        }
        check_budget(&accounting, &spec.limits, &reservation)?;
        let mut parent = self.coding_parent(task.id)?;
        if let Some(parent) = &mut parent {
            check_budget(
                &parent.summary.accounting,
                &parent.request.limits,
                &reservation,
            )?;
            parent.summary.accounting.admitted_coding_calls += 1;
            parent.summary.accounting.held_tokens += reservation.total_tokens;
            parent.summary.accounting.held_cost_microusd += reservation.cost_microusd;
            parent.summary.updated_at = self.now();
        }
        let secret = self.provider_secret()?;
        if reflects_secret(&prepared.body, secret.expose_secret()) {
            return Err(OperationError::InvalidInput);
        }
        let view = CodingCallView {
            id,
            task: task.id,
            phase: CodingCallPhase::Reserved,
            reservation,
            accounting: InferenceAccounting::Held,
            usage: None,
            provider_request_id: None,
        };
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO coding_calls VALUES(?1,?2,?3,?4,?5,?6,?7,NULL)",
            params![
                id.to_string(),
                task.id.to_string(),
                execution.to_string(),
                generation.get() as i64,
                encoded,
                json(&configuration)?,
                json(&view)?
            ],
        )?;
        if let Some(parent) = parent {
            runs::save_summary(&tx, &parent.summary)?;
        }
        event(&tx, "coding_reserved", id)?;
        tx.commit()?;
        self.fault("coding_reserved");
        Ok(CodingAdmission::Call(Box::new(CodingPermit {
            view,
            configuration,
            spec,
            prepared,
            secret,
        })))
    }
    pub(in crate::manager) fn send_coding(
        &mut self,
        id: InferenceAttemptId,
    ) -> Result<bool, OperationError> {
        let (view, generation): (String, i64) = self.connection.query_row(
            "SELECT view,generation FROM coding_calls WHERE id=?1",
            [id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let mut view: CodingCallView = parse(view)?;
        if view.phase != CodingCallPhase::Reserved {
            return Err(OperationError::Conflict);
        }
        if self
            .coding_authority(&self.load_task(view.task)?, (generation as u64).try_into()?)
            .is_err()
        {
            return Ok(false);
        }
        view.phase = CodingCallPhase::Sent;
        self.connection.execute(
            "UPDATE coding_calls SET view=?1 WHERE id=?2",
            params![json(&view)?, id.to_string()],
        )?;
        self.fault("coding_sent");
        Ok(true)
    }
    pub(in crate::manager) fn finish_coding(
        &mut self,
        id: InferenceAttemptId,
        outcome: EnvelopeOutcome,
    ) -> Result<Value, OperationError> {
        let (view, config): (String, String) = self.connection.query_row(
            "SELECT view,configuration FROM coding_calls WHERE id=?1",
            [id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let mut view: CodingCallView = parse(view)?;
        let config: InferenceConfiguration = parse(config)?;
        if !matches!(
            view.phase,
            CodingCallPhase::Sent | CodingCallPhase::Reserved
        ) {
            return Err(OperationError::Conflict);
        }
        let response = match outcome {
            EnvelopeOutcome::Billed {
                usage,
                request_id,
                value,
            } => {
                view.accounting = InferenceAccounting::Settled {
                    charged_microusd: inference::cost(
                        &config.policy.prices,
                        usage.prompt_tokens,
                        usage.completion_tokens,
                    )?,
                };
                view.usage = Some(usage);
                view.provider_request_id = Some(request_id);
                if completion(&value).is_ok() {
                    view.phase = CodingCallPhase::Settled;
                    Some(value)
                } else {
                    view.phase = CodingCallPhase::Unknown;
                    None
                }
            }
            EnvelopeOutcome::NotSent { .. } => {
                view.phase = CodingCallPhase::NotSent;
                view.accounting = InferenceAccounting::Released;
                None
            }
            EnvelopeOutcome::Unknown { request_id, .. } => {
                view.phase = CodingCallPhase::Unknown;
                view.provider_request_id = request_id;
                None
            }
        };
        let mut parent = self.coding_parent(view.task)?;
        if let Some(parent) = &mut parent {
            if view.accounting != InferenceAccounting::Held {
                parent.summary.accounting.held_tokens -= view.reservation.total_tokens;
                parent.summary.accounting.held_cost_microusd -= view.reservation.cost_microusd;
                if let (Some(usage), InferenceAccounting::Settled { charged_microusd }) =
                    (&view.usage, &view.accounting)
                {
                    parent.summary.accounting.settled_tokens += usage.total_tokens;
                    parent.summary.accounting.settled_cost_microusd += charged_microusd;
                }
            }
            if view.phase == CodingCallPhase::Unknown {
                parent.summary.phase = RunPhase::AwaitingAttention {
                    reason: RunAttentionReason::Provider,
                };
            }
            parent.summary.updated_at = self.now();
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE coding_calls SET view=?1,response=?2 WHERE id=?3",
            params![
                json(&view)?,
                response.as_ref().map(json).transpose()?,
                id.to_string()
            ],
        )?;
        if let Some(parent) = parent {
            runs::save_summary(&tx, &parent.summary)?;
        }
        event(&tx, "coding_settled", id)?;
        tx.commit()?;
        self.fault("coding_settled");
        response.ok_or(OperationError::Conflict)
    }
    pub(super) fn recover_coding(&mut self) -> Result<(), OperationError> {
        self.connection
            .execute("UPDATE coding_leases SET closed=1", [])?;
        let calls = self
            .connection
            .prepare("SELECT view FROM coding_calls")?
            .query_map([], |r| r.get::<_, String>(0))?
            .map(|r| parse::<CodingCallView>(r?))
            .collect::<Result<Vec<_>, OperationError>>()?;
        for mut call in calls {
            if matches!(
                call.phase,
                CodingCallPhase::Reserved | CodingCallPhase::Sent
            ) {
                call.phase = CodingCallPhase::Unknown;
                self.connection.execute(
                    "UPDATE coding_calls SET view=?1 WHERE id=?2",
                    params![json(&call)?, call.id.to_string()],
                )?;
                if let Some(mut parent) = self.coding_parent(call.task)? {
                    parent.summary.phase = RunPhase::AwaitingAttention {
                        reason: RunAttentionReason::Restart,
                    };
                    runs::save_summary(&self.connection, &parent.summary)?;
                }
            }
        }
        Ok(())
    }
}
fn reserve(
    config: &InferenceConfiguration,
    spec: &VibeSpec,
) -> Result<InferenceReservation, OperationError> {
    if spec.limits.max_output_tokens >= config.policy.context_tokens {
        return Err(OperationError::InferenceBudget);
    }
    Ok(InferenceReservation {
        total_tokens: u64::from(config.policy.context_tokens)
            + u64::from(spec.limits.max_output_tokens),
        cost_microusd: inference::cost(
            &config.policy.prices,
            config.policy.context_tokens.into(),
            spec.limits.max_output_tokens.into(),
        )?,
    })
}
fn check_budget(
    a: &RunAccounting,
    l: &RunLimits,
    r: &InferenceReservation,
) -> Result<(), OperationError> {
    if a.admitted_turns + a.admitted_coding_calls >= l.max_turns
        || a.settled_tokens
            .checked_add(a.held_tokens)
            .and_then(|v| v.checked_add(r.total_tokens))
            .is_none_or(|v| v > l.max_total_tokens)
        || a.settled_cost_microusd
            .checked_add(a.held_cost_microusd)
            .and_then(|v| v.checked_add(r.cost_microusd))
            .is_none_or(|v| v > l.max_cost_microusd)
    {
        return Err(OperationError::InferenceBudget);
    }
    Ok(())
}
fn coding_result(task: &TaskView) -> Result<VibeResult, OperationError> {
    if !matches!(
        task.outcome,
        Some(ExecutionEvent::ProcessFinished { success: true, .. })
    ) {
        return Err(OperationError::Conflict);
    }
    let r = task
        .process
        .as_ref()
        .and_then(|e| e.result.as_ref())
        .ok_or(OperationError::Conflict)?;
    if r.exit != (ProcessExit::Exited { code: 0 }) || !r.stdout.complete || r.stdout.truncated {
        return Err(OperationError::Conflict);
    }
    let bytes = (0..r.stdout.hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&r.stdout.hex[i..i + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| OperationError::Conflict)?;
    let result: VibeResult =
        serde_json::from_slice(&bytes).map_err(|_| OperationError::Conflict)?;
    if result.exit_code != 0
        || result.calls > MAX_CODING_CALLS
        || result.approval_denied != 0
        || result.tool_errors != 0
    {
        return Err(OperationError::Conflict);
    }
    Ok(result)
}
