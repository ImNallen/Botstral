use super::*;
use crate::worker_http::VerifiedIdentity;

impl State {
    fn connector_peer(
        &self,
        peer: &VerifiedIdentity,
        id: TaskId,
        execution: ExecutionId,
    ) -> Result<TaskView, OperationError> {
        let task = self.load_task(id)?;
        let valid: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM workers WHERE id=?1 AND fingerprint=?2 AND revoked=0)",
            params![task.command.worker.to_string(), peer.fingerprint().as_str()],
            |r| r.get(0),
        )?;
        if !valid || task.command.execution != execution {
            return Err(OperationError::Unauthorized);
        }
        Ok(task)
    }
    pub(in crate::manager) fn task_gateway_authority(
        &self,
        task: &TaskView,
        generation: Generation,
    ) -> Result<(), OperationError> {
        self.require_live_bot(task.command.bot)?;
        let binding = task
            .approval
            .binding
            .as_ref()
            .ok_or(OperationError::Unauthorized)?;
        let saved: Option<(String, i64, String)> = self
            .connection
            .query_row(
                "SELECT execution,generation,fence FROM connector_leases WHERE task=?1",
                [task.id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((execution, leased_generation, fence)) = saved else {
            return Err(OperationError::Unauthorized);
        };
        let fence: InputFence = parse(fence)?;
        let policy = self.permission(task.command.bot)?;
        if self
            .inference_control
            .as_ref()
            .is_some_and(|c| c.stopping.load(std::sync::atomic::Ordering::Acquire))
            || execution != task.command.execution.to_string()
            || leased_generation != generation.get() as i64
            || fence.generation != generation
            || fence.session_id != task.command.session
            || fence.owner != InputOwner::Bot
            || task.state != TaskState::Running
            || !task.approval.consumed
            || task.approval.revoked
            || !task.command.matches_authority(binding)
            || !policy.allow
            || policy.revision != binding.permission_revision
            || binding.expires_at <= self.now()
            || task.request.settings.deadline <= self.now()
            || !self
                .worker_connections
                .get(&task.command.worker)
                .is_some_and(|c| c.recovered && c.generation == generation)
            || self.parent_run(task.id)?.is_some_and(|run| {
                run.summary.cancellation_requested_at.is_some()
                    || run.request.limits.deadline <= self.now()
                    || !matches!(run.summary.phase, RunPhase::WaitingForTool { task: child, .. } if child == task.id)
            })
        {
            return Err(OperationError::Unauthorized);
        }
        self.check_input_fence(task.command.worker, fence)
    }
    fn connector_task_generation(&self, task: &TaskView) -> Result<Generation, OperationError> {
        let generation = self
            .worker_connections
            .get(&task.command.worker)
            .ok_or(OperationError::Unauthorized)?
            .generation;
        self.task_gateway_authority(task, generation)?;
        Ok(generation)
    }
    pub(in crate::manager) fn connector_gateway_catalog(
        &self,
        peer: VerifiedIdentity,
        task: TaskId,
        execution: ExecutionId,
    ) -> Result<Vec<ConnectorToolDefinition>, OperationError> {
        let task = self.connector_peer(&peer, task, execution)?;
        self.connector_task_generation(&task)?;
        if self.task_connector_calls(task.id)?.iter().any(unknown) {
            return Err(OperationError::Conflict);
        }
        let WorkSpec::Vibe { spec } = &task.command.work else {
            return Err(OperationError::InvalidInput);
        };
        let Some(policy) = &spec.policy.connectors else {
            return Ok(vec![]);
        };
        Ok(policy
            .tools
            .iter()
            .filter_map(|b| self.validate_connector_binding(task.command.bot, b).ok())
            .collect())
    }
    pub(in crate::manager) fn load_connector_call(
        &self,
        id: ConnectorCallId,
    ) -> Result<ConnectorCallView, OperationError> {
        parse(
            self.connection
                .query_row(
                    "SELECT view FROM connector_calls WHERE id=?1",
                    [id.to_string()],
                    |r| r.get::<_, String>(0),
                )
                .optional()?
                .ok_or(OperationError::NotFound)?,
        )
    }
    pub(in crate::manager) fn task_connector_calls(
        &self,
        task: TaskId,
    ) -> Result<Vec<ConnectorCallView>, OperationError> {
        self.connection
            .prepare("SELECT view FROM connector_calls WHERE task=?1 ORDER BY rowid")?
            .query_map([task.to_string()], |r| r.get::<_, String>(0))?
            .map(|r| parse(r?))
            .collect()
    }
    pub(in crate::manager) fn connector_calls(
        &self,
        owner: impl Into<OwnerAuthority>,
        task: TaskId,
    ) -> Result<Vec<ConnectorCallView>, OperationError> {
        self.authorize_owner(&owner.into())?;
        self.load_task(task)?;
        self.task_connector_calls(task)
    }
    pub(in crate::manager) fn connector_status(
        &mut self,
        peer: VerifiedIdentity,
        task: TaskId,
        execution: ExecutionId,
        id: ConnectorCallId,
    ) -> Result<ConnectorCallView, OperationError> {
        self.connector_peer(&peer, task, execution)?;
        let mut call = self.load_connector_call(id)?;
        if call.task != task || call.execution != execution {
            return Err(OperationError::Unauthorized);
        }
        if matches!(call.state, ConnectorCallState::DispatchPossible)
            && !self.connector_still_authorized(id)?
        {
            call = self.finish_connector(
                id,
                ConnectorOutcome::Unknown {
                    reason: ConnectorFailure::Authority,
                },
            )?;
        }
        Ok(call)
    }
    fn connector_operation_authority(
        &self,
        task: &TaskView,
        operation: &ConnectorOperation,
    ) -> Result<(u32, u32), OperationError> {
        match &task.command.work {
            WorkSpec::Connector { spec } if &spec.operation == operation => {
                self.validate_connector_work(
                    task.command.bot,
                    task.command.worker,
                    &task.command.work,
                )?;
                Ok((1, spec.result.max_bytes as u32))
            }
            WorkSpec::Vibe { spec } => {
                let policy = spec
                    .policy
                    .connectors
                    .as_ref()
                    .ok_or(OperationError::Unauthorized)?;
                let ConnectorOperation::Call { proposal } = operation else {
                    return Err(OperationError::Unauthorized);
                };
                if !policy.tools.contains(&proposal.tool) {
                    return Err(OperationError::Unauthorized);
                }
                let definition =
                    self.validate_connector_binding(task.command.bot, &proposal.tool)?;
                validate_arguments(&definition.input_schema, &proposal.arguments)?;
                if let ConnectionTransport::WorkerStdio {
                    worker, account, ..
                } = self
                    .load_connection(proposal.tool.connection.connection)?
                    .transport
                    && (worker != task.command.worker || account != spec.policy.account)
                {
                    return Err(OperationError::Unauthorized);
                }
                Ok((policy.max_calls, policy.max_result_bytes))
            }
            _ => Err(OperationError::Unauthorized),
        }
    }
    pub(in crate::manager) fn admit_connector(
        &mut self,
        peer: VerifiedIdentity,
        task: TaskId,
        execution: ExecutionId,
        id: ConnectorCallId,
        operation: ConnectorOperation,
    ) -> Result<ConnectorAdmission, OperationError> {
        let task = self.connector_peer(&peer, task, execution)?;
        if let Ok(previous) = self.load_connector_call(id) {
            return if previous.task == task.id
                && previous.execution == execution
                && previous.argument_digest == operation.digest()
            {
                Ok(ConnectorAdmission::Existing(Box::new(previous)))
            } else {
                Err(OperationError::Conflict)
            };
        }
        let authorize = || {
            let generation = self.connector_task_generation(&task)?;
            let (max_calls, max_result_bytes) =
                self.connector_operation_authority(&task, &operation)?;
            let calls = self.task_connector_calls(task.id)?;
            if calls.len() >= max_calls as usize
                || calls.iter().any(|c| {
                    unknown(c)
                        || matches!(
                            c.state,
                            ConnectorCallState::Reserved | ConnectorCallState::DispatchPossible
                        )
                })
            {
                return Err(OperationError::Conflict);
            }
            self.connector_identity()?;
            Ok((generation, max_result_bytes))
        };
        let (generation, max_result_bytes) = match authorize() {
            Ok(a) => a,
            Err(e) => {
                audit(
                    &self.connection,
                    operation.connection().connection,
                    Some(id),
                    "admission_denied",
                    Some(&operation.digest()),
                    self.now(),
                )?;
                return Err(e);
            }
        };
        let view = ConnectorCallView {
            id,
            task: task.id,
            execution,
            connection: operation.connection().clone(),
            tool: match &operation {
                ConnectorOperation::Call { proposal } => Some(proposal.tool.clone()),
                _ => None,
            },
            argument_digest: operation.digest(),
            state: ConnectorCallState::Reserved,
            created_at: self.now(),
            revoked_during_call_at: None,
            late_result: None,
        };
        let now = self.now();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO connector_calls VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                id.to_string(),
                task.id.to_string(),
                execution.to_string(),
                generation.get() as i64,
                json(&operation)?,
                json(&view)?,
                task.request.settings.deadline,
                max_result_bytes
            ],
        )?;
        audit(
            &tx,
            view.connection.connection,
            Some(id),
            "reserved",
            Some(&view.argument_digest),
            now,
        )?;
        tx.commit()?;
        self.fault("connector_reserved");
        Ok(ConnectorAdmission::Reserved(id))
    }
    pub(in crate::manager) fn connector_still_authorized(
        &self,
        id: ConnectorCallId,
    ) -> Result<bool, OperationError> {
        let call = self.load_connector_call(id)?;
        if !matches!(
            call.state,
            ConnectorCallState::Reserved | ConnectorCallState::DispatchPossible
        ) {
            return Ok(false);
        }
        let (generation, operation): (i64, String) = self.connection.query_row(
            "SELECT generation,operation FROM connector_calls WHERE id=?1",
            [id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let task = self.load_task(call.task)?;
        Ok(self
            .task_gateway_authority(&task, (generation as u64).try_into()?)
            .is_ok()
            && self
                .connector_operation_authority(&task, &parse(operation)?)
                .is_ok()
            && self.connector_identity().is_ok())
    }
    pub(in crate::manager) fn dispatch_connector(
        &mut self,
        id: ConnectorCallId,
    ) -> Result<Option<ConnectorDispatch>, OperationError> {
        let mut call = self.load_connector_call(id)?;
        if call.state != ConnectorCallState::Reserved {
            return Ok(None);
        }
        if !self.connector_still_authorized(id)? {
            self.finish_connector(
                id,
                ConnectorOutcome::NotSent {
                    reason: ConnectorFailure::Authority,
                },
            )?;
            return Ok(None);
        }
        let (operation, deadline, max_result_bytes): (String, i64, u32) =
            self.connection.query_row(
                "SELECT operation,deadline,max_result_bytes FROM connector_calls WHERE id=?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
        let connection = self.load_connection(call.connection.connection)?;
        let secret = self.connection_secret(&connection)?;
        let operation = parse(operation)?;
        call.state = ConnectorCallState::DispatchPossible;
        let now = self.now();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE connector_calls SET view=?1 WHERE id=?2",
            params![json(&call)?, id.to_string()],
        )?;
        audit(&tx, connection.id, Some(id), "dispatch_possible", None, now)?;
        tx.commit()?;
        self.fault("connector_dispatch_possible");
        Ok(Some(ConnectorDispatch {
            id,
            connection,
            operation,
            secret,
            deadline,
            max_result_bytes,
        }))
    }
    pub(in crate::manager) fn settle_local_connector(
        &mut self,
        peer: VerifiedIdentity,
        task: TaskId,
        execution: ExecutionId,
        id: ConnectorCallId,
        outcome: ConnectorOutcome,
    ) -> Result<ConnectorCallView, OperationError> {
        self.connector_peer(&peer, task, execution)?;
        let call = self.load_connector_call(id)?;
        let view = self.load_connection(call.connection.connection)?;
        if call.task != task
            || call.execution != execution
            || !matches!(view.transport, ConnectionTransport::WorkerStdio { .. })
        {
            return Err(OperationError::Unauthorized);
        }
        if call.state == ConnectorCallState::Reserved {
            return Err(OperationError::Conflict);
        }
        self.finish_connector(id, outcome)
    }
    pub(in crate::manager) fn finish_connector(
        &mut self,
        id: ConnectorCallId,
        mut outcome: ConnectorOutcome,
    ) -> Result<ConnectorCallView, OperationError> {
        let mut call = self.load_connector_call(id)?;
        if let ConnectorCallState::Resolved { outcome: previous } = &call.state
            && !matches!(previous, ConnectorOutcome::Unknown { .. })
        {
            return Ok(call);
        }
        let max_result: u32 = self.connection.query_row(
            "SELECT max_result_bytes FROM connector_calls WHERE id=?1",
            [id.to_string()],
            |r| r.get(0),
        )?;
        let old_view: String = self.connection.query_row(
            "SELECT view FROM connection_versions WHERE connection=?1 AND revision=?2",
            params![
                call.connection.connection.to_string(),
                call.connection.revision as i64
            ],
            |r| r.get(0),
        )?;
        let old_view: ConnectionView = parse(old_view)?;
        let mut updated_connection = None;
        if let ConnectorOutcome::Finished { result } = &mut outcome {
            match self.connection_secret(&old_view) {
                Ok(secret) => {
                    redact_result(result, &secret_values(&secret));
                    let valid = match &result {
                        ConnectorResult::Catalog { tools } => {
                            call.tool.is_none() && validate_catalog(tools).is_ok()
                        }
                        ConnectorResult::Tool { value } => {
                            call.tool.is_some()
                                && bounded_connector_json(value, max_result as usize)
                                && value.get("content").is_some_and(|v| v.is_array())
                        }
                    };
                    if !valid {
                        outcome = ConnectorOutcome::Unknown {
                            reason: ConnectorFailure::Protocol,
                        };
                    }
                }
                Err(_) => {
                    outcome = ConnectorOutcome::Unknown {
                        reason: ConnectorFailure::Interrupted,
                    }
                }
            }
        }
        let late = unknown(&call);
        if late {
            if let ConnectorOutcome::Finished { result } = outcome {
                if call
                    .late_result
                    .as_ref()
                    .is_some_and(|previous| previous != &result)
                {
                    return Err(OperationError::Conflict);
                }
                call.late_result = Some(result);
            }
        } else {
            if let ConnectorOutcome::Finished {
                result: ConnectorResult::Catalog { tools },
            } = &outcome
            {
                let mut connection = self.load_connection(call.connection.connection)?;
                if connection.version == call.connection
                    && connection.availability != ConnectionAvailability::Revoked
                {
                    connection.catalog_revision = advance(connection.catalog_revision)?;
                    connection.version.revision = advance(connection.version.revision)?;
                    connection.tools = tools.clone();
                    connection.availability = ConnectionAvailability::Ready;
                    updated_connection = Some(connection);
                }
            } else {
                let availability = match &outcome {
                    ConnectorOutcome::NotSent { reason } | ConnectorOutcome::Unknown { reason } => {
                        match reason {
                            ConnectorFailure::Authentication => {
                                Some(ConnectionAvailability::AuthenticationRequired)
                            }
                            ConnectorFailure::CatalogChanged => {
                                Some(ConnectionAvailability::NeedsDiscovery)
                            }
                            ConnectorFailure::Transport
                            | ConnectorFailure::Protocol
                            | ConnectorFailure::Deadline
                            | ConnectorFailure::Bound => Some(ConnectionAvailability::Unavailable),
                            ConnectorFailure::Authority
                            | ConnectorFailure::Interrupted
                            | ConnectorFailure::Restart => None,
                        }
                    }
                    ConnectorOutcome::Finished { .. } => None,
                };
                if let Some(availability) = availability {
                    let mut connection = self.load_connection(call.connection.connection)?;
                    if connection.version == call.connection {
                        connection.version.revision = advance(connection.version.revision)?;
                        connection.availability = availability;
                        connection.tools.clear();
                        updated_connection = Some(connection);
                    }
                }
            }
            call.state = ConnectorCallState::Resolved { outcome };
        }
        let now = self.now();
        let digest = CertificateFingerprint::from_der(json(&call.state)?.as_bytes());
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE connector_calls SET view=?1 WHERE id=?2",
            params![json(&call)?, id.to_string()],
        )?;
        if let Some(connection) = &updated_connection {
            save_connection(&tx, connection)?;
            if connection.availability == ConnectionAvailability::Ready {
                tx.execute(
                    "INSERT INTO connector_catalogs VALUES(?1,?2,?3)",
                    params![
                        connection.id.to_string(),
                        connection.catalog_revision as i64,
                        json(&connection.tools)?
                    ],
                )?;
            }
        }
        audit(
            &tx,
            call.connection.connection,
            Some(id),
            if late { "late_evidence" } else { "resolved" },
            Some(&digest),
            now,
        )?;
        if unknown(&call) {
            let run: Option<String> = tx
                .query_row(
                    "SELECT run FROM run_children WHERE task=?1",
                    [call.task.to_string()],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(run) = run {
                let summary: String =
                    tx.query_row("SELECT summary FROM runs WHERE id=?1", [run.clone()], |r| {
                        r.get(0)
                    })?;
                let mut summary: RunSummary = parse(summary)?;
                summary.phase = RunPhase::AwaitingAttention {
                    reason: RunAttentionReason::Worker,
                };
                summary.updated_at = now;
                runs::save_summary(&tx, &summary)?;
            }
        }
        tx.commit()?;
        if unknown(&call) {
            self.stop_connector_task(call.task)?;
        }
        self.fault("connector_settled");
        Ok(call)
    }
    pub(in crate::manager) fn connector_stop_requires_attention(
        db: &Connection,
        task: &TaskView,
    ) -> Result<bool, OperationError> {
        if !matches!(task.outcome, Some(ExecutionEvent::Stopped { .. })) {
            return Ok(false);
        }
        let mut statement = db.prepare("SELECT view FROM connector_calls WHERE task=?1")?;
        let calls = statement.query_map([task.id.to_string()], |r| r.get::<_, String>(0))?;
        for call in calls {
            if unknown(&parse(call?)?) {
                return Ok(true);
            }
        }
        Ok(false)
    }
    fn stop_connector_task(&mut self, id: TaskId) -> Result<(), OperationError> {
        let mut task = self.load_task(id)?;
        let attention = Self::connector_stop_requires_attention(&self.connection, &task)?;
        if attention {
            if task.state == TaskState::AwaitingAttention {
                return Ok(());
            }
            task.state = TaskState::AwaitingAttention;
        } else if task.state.terminal() || task.state == TaskState::Cancelling {
            return Ok(());
        } else {
            task.state = TaskState::Cancelling;
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !attention {
            tx.execute(
                "INSERT OR IGNORE INTO cancellations VALUES(?1,?2)",
                params![id.to_string(), CommandId::new().to_string()],
            )?;
        }
        tasks::save(&tx, &task)?;
        event(
            &tx,
            if attention {
                "connector_attention"
            } else {
                "connector_stop_requested"
            },
            id,
        )?;
        tx.commit()?;
        self.pump_tasks(task.command.worker)
    }
    pub(in crate::manager) fn invalidate_connector_calls(
        &mut self,
        connection: ConnectionId,
        bot: Option<BotId>,
    ) -> Result<(), OperationError> {
        let calls = self
            .connection
            .prepare("SELECT view FROM connector_calls")?
            .query_map([], |r| r.get::<_, String>(0))?
            .map(|r| parse::<ConnectorCallView>(r?))
            .collect::<Result<Vec<_>, _>>()?;
        for mut call in calls {
            if call.connection.connection != connection
                || bot.is_some_and(|bot| {
                    self.load_task(call.task)
                        .is_ok_and(|t| t.command.bot != bot)
                })
            {
                continue;
            }
            let outcome = match call.state {
                ConnectorCallState::Reserved => ConnectorOutcome::NotSent {
                    reason: ConnectorFailure::Authority,
                },
                ConnectorCallState::DispatchPossible => {
                    call.revoked_during_call_at = Some(self.now());
                    self.connection.execute(
                        "UPDATE connector_calls SET view=?1 WHERE id=?2",
                        params![json(&call)?, call.id.to_string()],
                    )?;
                    ConnectorOutcome::Unknown {
                        reason: ConnectorFailure::Authority,
                    }
                }
                _ => continue,
            };
            self.finish_connector(call.id, outcome)?;
        }
        Ok(())
    }
    pub(in crate::manager) fn reconcile_connectors(&mut self) -> Result<(), OperationError> {
        let calls = self
            .connection
            .prepare("SELECT view FROM connector_calls")?
            .query_map([], |r| r.get::<_, String>(0))?
            .map(|r| parse::<ConnectorCallView>(r?))
            .collect::<Result<Vec<_>, _>>()?;
        for call in calls {
            if matches!(call.state, ConnectorCallState::DispatchPossible)
                && !self.connector_still_authorized(call.id)?
            {
                self.finish_connector(
                    call.id,
                    ConnectorOutcome::Unknown {
                        reason: ConnectorFailure::Interrupted,
                    },
                )?;
            }
        }
        Ok(())
    }
    pub(in crate::manager) fn recover_connectors(&mut self) -> Result<(), OperationError> {
        self.connection
            .execute("DELETE FROM connector_leases", [])?;
        let calls = self
            .connection
            .prepare("SELECT view FROM connector_calls")?
            .query_map([], |r| r.get::<_, String>(0))?
            .map(|r| parse::<ConnectorCallView>(r?))
            .collect::<Result<Vec<_>, _>>()?;
        for call in calls {
            match call.state {
                ConnectorCallState::Reserved => {
                    self.finish_connector(
                        call.id,
                        ConnectorOutcome::NotSent {
                            reason: ConnectorFailure::Restart,
                        },
                    )?;
                }
                ConnectorCallState::DispatchPossible => {
                    self.finish_connector(
                        call.id,
                        ConnectorOutcome::Unknown {
                            reason: ConnectorFailure::Restart,
                        },
                    )?;
                }
                ConnectorCallState::Resolved {
                    outcome: ConnectorOutcome::Unknown { .. },
                } => self.stop_connector_task(call.task)?,
                _ => {}
            }
        }
        Ok(())
    }
}
pub(in crate::manager) fn unknown(call: &ConnectorCallView) -> bool {
    matches!(
        call.state,
        ConnectorCallState::Resolved {
            outcome: ConnectorOutcome::Unknown { .. }
        }
    )
}
