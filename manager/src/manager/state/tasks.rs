mod recovery;
use super::*;
use botstral_link::*;

#[derive(PartialEq)]
pub(in crate::manager) enum ProcessControlStatus {
    AwaitingStop,
    Unproved,
}

pub(super) fn json<T: serde::Serialize>(value: &T) -> Result<String, OperationError> {
    serde_json::to_string(value).map_err(|_| OperationError::Internal)
}
pub(super) fn parse<T: serde::de::DeserializeOwned>(value: String) -> Result<T, OperationError> {
    serde_json::from_str(&value).map_err(|_| OperationError::Internal)
}
pub(super) fn event(
    db: &Connection,
    kind: &str,
    entity: impl ToString,
) -> Result<(), OperationError> {
    db.execute(
        "UPDATE event_head SET sequence = sequence + 1 WHERE id = 1",
        [],
    )?;
    db.execute(
        "INSERT INTO events SELECT sequence, ?1, ?2 FROM event_head",
        params![kind, entity.to_string()],
    )?;
    db.execute(
        "DELETE FROM events WHERE sequence <= (SELECT sequence - ?1 FROM event_head)",
        [EVENT_RETENTION as i64],
    )?;
    Ok(())
}
fn count_bound(db: &Connection, table: &str, bound: usize) -> Result<(), OperationError> {
    let n: i64 = db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?;
    if n >= bound as i64 {
        return Err(OperationError::Busy);
    }
    Ok(())
}
pub(super) fn save(db: &Connection, task: &TaskView) -> Result<(), OperationError> {
    let body = json(task)?;
    if body.len() > 24 * 1024 {
        return Err(OperationError::InvalidInput);
    }
    db.execute(
        "UPDATE tasks SET view = ?1 WHERE id = ?2",
        params![body, task.id.to_string()],
    )?;
    Ok(())
}
impl State {
    pub(in crate::manager) fn process_control_status(
        &self,
        worker: WorkerId,
    ) -> Result<std::collections::HashMap<ComputerSessionId, ProcessControlStatus>, OperationError>
    {
        let mut status = std::collections::HashMap::new();
        let mut statement = self
            .connection
            .prepare("SELECT view FROM tasks WHERE worker=?1")?;
        let rows = statement.query_map([worker.to_string()], |row| row.get::<_, String>(0))?;
        for row in rows {
            let task: TaskView = parse(row?)?;
            if task.state.terminal() || !task.approval.consumed || !task.command.work.supervised() {
                continue;
            }
            if recovery::missing_process_scope(&task) {
                status.insert(task.command.session, ProcessControlStatus::Unproved);
            } else {
                status
                    .entry(task.command.session)
                    .or_insert(ProcessControlStatus::AwaitingStop);
            }
        }
        Ok(status)
    }

    pub(in crate::manager) fn load_task(&self, id: TaskId) -> Result<TaskView, OperationError> {
        parse(
            self.connection
                .query_row(
                    "SELECT view FROM tasks WHERE id = ?1",
                    [id.to_string()],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or(OperationError::NotFound)?,
        )
    }
    pub(in crate::manager) fn task(
        &self,
        token: SessionToken,
        id: TaskId,
    ) -> Result<TaskView, OperationError> {
        self.authorize(&token)?;
        self.load_task(id)
    }
    pub(super) fn principal(&self) -> Result<OwnerId, OperationError> {
        let id: String = self.connection.query_row(
            "SELECT principal FROM owner_identity WHERE id = 1",
            [],
            |r| r.get(0),
        )?;
        id.parse().map_err(|_| OperationError::Internal)
    }
    fn permission(&self, bot: BotId) -> Result<Permission, OperationError> {
        Ok(self
            .connection
            .query_row(
                "SELECT revision, allow FROM permissions WHERE bot = ?1",
                [bot.to_string()],
                |r| {
                    Ok(Permission {
                        bot,
                        revision: r.get::<_, i64>(0)? as u64,
                        allow: r.get(1)?,
                    })
                },
            )
            .optional()?
            .unwrap_or(Permission {
                bot,
                revision: 1,
                allow: true,
            }))
    }
    pub(in crate::manager) fn submit_task(
        &mut self,
        token: SessionToken,
        input: SubmitTask,
    ) -> Result<TaskView, OperationError> {
        self.authorize(&token)?;
        input.settings.validate()?;
        match self.load_task(input.id) {
            Ok(previous) => {
                return if previous.request == input {
                    Ok(previous)
                } else {
                    Err(OperationError::Conflict)
                };
            }
            Err(OperationError::NotFound) => {}
            Err(e) => return Err(e),
        }
        count_bound(&self.connection, "tasks", MAX_TASKS)?;
        let session = self.find_computer_session(input.session_id)?;
        if let Some(conversation) = input.conversation_id {
            let bot: Option<String> = self
                .connection
                .query_row(
                    "SELECT bot FROM conversations WHERE id = ?1",
                    [conversation.to_string()],
                    |r| r.get(0),
                )
                .optional()?;
            if bot != Some(session.bot_id.to_string()) {
                return Err(OperationError::InvalidInput);
            }
        }
        match &input.work {
            WorkSpec::TestInput { steps } if !(1..=10000).contains(steps) => {
                return Err(OperationError::InvalidInput);
            }
            WorkSpec::TestEffect { value, .. } if *value > 10000 => {
                return Err(OperationError::InvalidInput);
            }
            WorkSpec::Computer { spec } => {
                spec.validate()?;
                if !self
                    .worker_connections
                    .get(&session.worker_id)
                    .is_some_and(|c| {
                        c.recovered && c.capabilities.contains(&Capability::ComputerV1)
                    })
                {
                    return Err(ErrorCode::UnsupportedExecution.into());
                }
                if spec.deadline != input.settings.deadline {
                    return Err(OperationError::InvalidInput);
                }
            }
            WorkSpec::Process { spec } => {
                spec.validate()?;
                if spec.deadline != input.settings.deadline {
                    return Err(OperationError::InvalidInput);
                }
            }
            _ => {}
        }
        if matches!(
            input.work,
            WorkSpec::TestInput { .. } | WorkSpec::TestEffect { .. }
        ) && !cfg!(feature = "nonproduction-test-worker")
        {
            return Err(ErrorCode::UnsupportedExecution.into());
        }
        let permission = self.permission(session.bot_id)?;
        let body = CommandBody {
            id: CommandId::new(),
            owner: self.principal()?,
            task: input.id,
            bot: session.bot_id,
            worker: session.worker_id,
            session: session.id,
            operation: OperationId::new(),
            execution: ExecutionId::new(),
            work: input.work.clone(),
        };
        let task = TaskView {
            id: input.id,
            request: input,
            command: body.clone(),
            state: TaskState::Queued,
            progress: 0,
            outcome: None,
            effect_evidence: Vec::new(),
            process: None,
            outputs: body
                .work
                .outputs()
                .iter()
                .cloned()
                .map(|declaration| OutputView {
                    declaration,
                    state: ArtifactState::Pending,
                })
                .collect(),
            approval: ApprovalView {
                id: ApprovalId::new(),
                binding: None,
                permission_revision: permission.revision,
                revoked: false,
                consumed: false,
            },
        };
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO tasks VALUES(?1, ?2, ?3, ?4, ?5)",
            params![
                task.id.to_string(),
                body.bot.to_string(),
                body.worker.to_string(),
                json(&task.request)?,
                json(&task)?
            ],
        )?;
        tx.execute(
            "INSERT INTO commands(id, task, body) VALUES(?1, ?2, ?3)",
            params![body.id.to_string(), task.id.to_string(), json(&body)?],
        )?;
        event(&tx, "task_submitted", task.id)?;
        tx.commit()?;
        self.fault("task_committed");
        self.pump_tasks(body.worker)?;
        self.load_task(task.id)
    }
    pub(in crate::manager) fn cancel_task(
        &mut self,
        token: SessionToken,
        id: TaskId,
    ) -> Result<TaskView, OperationError> {
        self.authorize(&token)?;
        let mut task = self.load_task(id)?;
        if task.state.terminal() || task.state == TaskState::Cancelling {
            return Ok(task);
        }
        task.state = TaskState::Cancelling;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO cancellations VALUES(?1, ?2)",
            params![id.to_string(), CommandId::new().to_string()],
        )?;
        save(&tx, &task)?;
        event(&tx, "cancellation_requested", id)?;
        tx.commit()?;
        self.pump_tasks(task.command.worker)?;
        Ok(task)
    }
    pub(in crate::manager) fn grant_approval(
        &mut self,
        token: SessionToken,
        id: TaskId,
        expires_at: i64,
    ) -> Result<TaskView, OperationError> {
        self.authorize(&token)?;
        let mut task = self.load_task(id)?;
        let policy = self.permission(task.command.bot)?;
        if task.state.terminal()
            || matches!(
                task.state,
                TaskState::Cancelling | TaskState::AwaitingAttention
            )
            || task.approval.consumed
            || !policy.allow
            || expires_at <= self.now()
            || expires_at > self.now() + 3600
            || expires_at > task.request.settings.deadline
        {
            return Err(OperationError::Conflict);
        }
        task.approval.binding = Some(ExecutionBinding {
            owner: task.command.owner,
            bot: task.command.bot,
            task: id,
            operation: task.command.operation,
            arguments: task.command.argument_digest(),
            permission_revision: policy.revision,
            approval: task.approval.id,
            execution: task.command.execution,
            expires_at,
        });
        task.approval.permission_revision = policy.revision;
        task.approval.revoked = false;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        save(&tx, &task)?;
        event(&tx, "approval_granted", id)?;
        tx.commit()?;
        self.pump_tasks(task.command.worker)?;
        Ok(task)
    }
    pub(in crate::manager) fn revoke_approval(
        &mut self,
        token: SessionToken,
        id: TaskId,
    ) -> Result<TaskView, OperationError> {
        self.authorize(&token)?;
        let mut task = self.load_task(id)?;
        task.approval.revoked = true;
        let stop =
            task.approval.consumed && task.command.work.supervised() && !task.state.terminal();
        if stop {
            task.state = TaskState::Cancelling;
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if stop {
            tx.execute(
                "INSERT OR IGNORE INTO cancellations VALUES(?1,?2)",
                params![id.to_string(), CommandId::new().to_string()],
            )?;
        }
        save(&tx, &task)?;
        event(&tx, "approval_revoked", id)?;
        tx.commit()?;
        if task.command.work.supervised() {
            self.pump_tasks(task.command.worker)?;
        }
        Ok(task)
    }
    pub(in crate::manager) fn set_permission(
        &mut self,
        token: SessionToken,
        bot: BotId,
        expected_revision: u64,
        allow: bool,
    ) -> Result<Permission, OperationError> {
        self.authorize(&token)?;
        self.find_bot(bot)?.ok_or(OperationError::NotFound)?;
        let exists: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM permissions WHERE bot=?1)",
            [bot.to_string()],
            |r| r.get(0),
        )?;
        if !exists {
            count_bound(&self.connection, "permissions", MAX_RECORDS)?;
        }
        let old = self.permission(bot)?;
        if old.revision != expected_revision || old.revision >= MAX_COUNTER {
            return Err(OperationError::Conflict);
        }
        let views = self
            .connection
            .prepare("SELECT view FROM tasks WHERE bot=?1")?
            .query_map([bot.to_string()], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let policy = Permission {
            bot,
            revision: old.revision + 1,
            allow,
        };
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("INSERT INTO permissions VALUES(?1, ?2, ?3) ON CONFLICT(bot) DO UPDATE SET revision=excluded.revision, allow=excluded.allow", params![bot.to_string(), policy.revision as i64, allow])?;
        let mut workers = std::collections::HashSet::new();
        for view in views {
            let mut task: TaskView = parse(view)?;
            if task.state.terminal() {
                continue;
            }
            if task.command.work.supervised() {
                workers.insert(task.command.worker);
            }
            if !task.approval.consumed {
                task.approval.revoked = true;
            } else if !allow && task.command.work.supervised() {
                task.state = TaskState::Cancelling;
                tx.execute(
                    "INSERT OR IGNORE INTO cancellations VALUES(?1,?2)",
                    params![task.id.to_string(), CommandId::new().to_string()],
                )?;
            }
            save(&tx, &task)?;
        }
        event(&tx, "permission_changed", bot)?;
        tx.commit()?;
        for worker in workers {
            self.pump_tasks(worker)?;
        }
        Ok(policy)
    }
    pub(in crate::manager) fn conversation(
        &mut self,
        token: SessionToken,
        input: Conversation,
    ) -> Result<Conversation, OperationError> {
        self.authorize(&token)?;
        self.find_bot(input.bot)?.ok_or(OperationError::NotFound)?;
        if let Some(body) = self
            .connection
            .query_row(
                "SELECT body FROM conversations WHERE id = ?1",
                [input.id.to_string()],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            return if parse::<Conversation>(body)? == input {
                Ok(input)
            } else {
                Err(OperationError::Conflict)
            };
        }
        count_bound(&self.connection, "conversations", MAX_RECORDS)?;
        let tx = self.connection.transaction()?;
        tx.execute(
            "INSERT INTO conversations VALUES(?1, ?2, ?3)",
            params![input.id.to_string(), input.bot.to_string(), json(&input)?],
        )?;
        event(&tx, "conversation_created", input.id)?;
        tx.commit()?;
        Ok(input)
    }
    pub(in crate::manager) fn append_message(
        &mut self,
        token: SessionToken,
        input: ConversationMessage,
    ) -> Result<ConversationMessage, OperationError> {
        self.authorize(&token)?;
        if input.text.is_empty() || input.text.len() > 4096 {
            return Err(OperationError::InvalidInput);
        }
        if let Some(body) = self
            .connection
            .query_row(
                "SELECT body FROM messages WHERE id = ?1",
                [input.id.to_string()],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            return if parse::<ConversationMessage>(body)? == input {
                Ok(input)
            } else {
                Err(OperationError::Conflict)
            };
        }
        count_bound(&self.connection, "messages", MAX_RECORDS)?;
        let exists: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM conversations WHERE id = ?1)",
            [input.conversation.to_string()],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(OperationError::NotFound);
        }
        let tx = self.connection.transaction()?;
        tx.execute(
            "INSERT INTO messages(id, conversation, body) VALUES(?1, ?2, ?3)",
            params![
                input.id.to_string(),
                input.conversation.to_string(),
                json(&input)?
            ],
        )?;
        event(&tx, "message_appended", input.id)?;
        tx.commit()?;
        Ok(input)
    }
    pub(in crate::manager) fn resume(
        &mut self,
        token: SessionToken,
        cursor: u64,
        limit: u16,
    ) -> Result<EventReplay, OperationError> {
        self.authorize(&token)?;
        if !(1..=64).contains(&limit) {
            return Err(OperationError::InvalidInput);
        }
        let documents = self.reconcile_documents()?;
        let head: i64 = self
            .connection
            .query_row("SELECT sequence FROM event_head", [], |r| r.get(0))?;
        let head = head as u64;
        if cursor > head {
            return Err(OperationError::InvalidInput);
        }
        if cursor == 0 || cursor < head.saturating_sub(EVENT_RETENTION) {
            let mut tasks = Vec::new();
            for row in self
                .connection
                .prepare("SELECT view FROM tasks ORDER BY id")?
                .query_map([], |r| r.get::<_, String>(0))?
            {
                tasks.push(parse(row?)?);
            }
            let mut conversations = Vec::new();
            for row in self
                .connection
                .prepare("SELECT body FROM conversations ORDER BY id")?
                .query_map([], |r| r.get::<_, String>(0))?
            {
                conversations.push(parse(row?)?);
            }
            let mut messages = Vec::new();
            for row in self
                .connection
                .prepare("SELECT body FROM messages ORDER BY sequence")?
                .query_map([], |r| r.get::<_, String>(0))?
            {
                messages.push(parse(row?)?);
            }
            let permissions = self
                .connection
                .prepare("SELECT bot, revision, allow FROM permissions ORDER BY bot")?
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)? as u64,
                        r.get(2)?,
                    ))
                })?
                .map(|row| {
                    let (bot, revision, allow) = row?;
                    Ok(Permission {
                        bot: bot.parse().map_err(|_| OperationError::Internal)?,
                        revision,
                        allow,
                    })
                })
                .collect::<Result<Vec<_>, OperationError>>()?;
            return Ok(EventReplay::Snapshot {
                state: OwnerSnapshot {
                    tasks,
                    inferences: self.inference_views(None)?,
                    inference_configurations: self.inference_configurations()?,
                    conversations,
                    messages,
                    documents,
                    permissions,
                },
                cursor: head,
            });
        }
        let entries = self.connection.prepare("SELECT sequence, kind, entity FROM events WHERE sequence > ?1 ORDER BY sequence LIMIT ?2")?.query_map(params![cursor as i64, limit], |r| Ok(AuthoritativeEvent { sequence: r.get::<_,i64>(0)? as u64, kind: r.get(1)?, entity: r.get(2)? }))?.collect::<Result<Vec<_>, _>>()?;
        let cursor = entries.last().map_or(cursor, |e| e.sequence);
        Ok(EventReplay::Events { entries, cursor })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, State, ComputerSessionId) {
        let root = tempfile::tempdir().unwrap();
        let mut db = Connection::open_in_memory().unwrap();
        migrate(&mut db).unwrap();
        db.execute_batch(
            "INSERT INTO owner VALUES(1,'Owner','unused'); INSERT INTO vault VALUES(1,1,X'00');",
        )
        .unwrap();
        let bot = BotId::new();
        let worker = WorkerId::new();
        let session = ComputerSessionId::new();
        db.execute("INSERT INTO bots VALUES(?1,'Bot',1)", [bot.to_string()])
            .unwrap();
        db.execute(
            "INSERT INTO workers(id,fingerprint) VALUES(?1,?2)",
            params![worker.to_string(), "a".repeat(64)],
        )
        .unwrap();
        db.execute(
            "INSERT INTO computer_sessions VALUES(?1,?2,?3,'bot',1)",
            params![session.to_string(), bot.to_string(), worker.to_string()],
        )
        .unwrap();
        let (configured, _) = watch::channel(false);
        let mut state = State::from_connection(db, root.path().into(), configured).unwrap();
        state.test_time = Some(1000);
        state
            .connection
            .execute(
                "INSERT INTO sessions VALUES(?1,?2,1,1,2000)",
                params![
                    Uuid::new_v4().to_string(),
                    crypto::token_hash(&"x".repeat(43)).as_slice()
                ],
            )
            .unwrap();
        (root, state, session)
    }
    fn token() -> SessionToken {
        SessionToken::new("x".repeat(43)).unwrap()
    }
    fn input(session: ComputerSessionId) -> SubmitTask {
        SubmitTask {
            id: TaskId::new(),
            session_id: session,
            conversation_id: None,
            settings: TaskSettings {
                model: "unavailable".into(),
                max_turns: 2,
                max_tokens: 100,
                deadline: 3000,
                max_cost_micros: 10,
            },
            work: WorkSpec::Unavailable,
        }
    }
    #[test]
    fn task_outbox_and_event_roll_back_together() {
        let (_root, mut state, session) = fixture();
        let input = input(session);
        state.connection.execute_batch("CREATE TRIGGER reject_event BEFORE INSERT ON events BEGIN SELECT RAISE(ABORT,'fault'); END;").unwrap();
        assert_eq!(
            state.submit_task(token(), input.clone()),
            Err(OperationError::Internal)
        );
        for table in ["tasks", "commands", "events"] {
            assert_eq!(
                state
                    .connection
                    .query_row::<i64, _, _>(&format!("SELECT count(*) FROM {table}"), [], |r| r
                        .get(0))
                    .unwrap(),
                0
            );
        }
        state
            .connection
            .execute_batch("DROP TRIGGER reject_event")
            .unwrap();
        let task = state.submit_task(token(), input.clone()).unwrap();
        assert_eq!(state.submit_task(token(), input.clone()).unwrap(), task);
        let mut changed = input;
        changed.settings.max_turns += 1;
        assert_eq!(
            state.submit_task(token(), changed),
            Err(OperationError::Conflict)
        );
        assert!(
            state
                .connection
                .execute("UPDATE commands SET body='{}'", [])
                .is_err()
        );
    }
    #[test]
    fn approval_mutations_require_owner_and_survive_login_changes() {
        let (_root, mut state, session) = fixture();
        let task = state.submit_task(token(), input(session)).unwrap();
        assert_eq!(
            state.grant_approval(SessionToken::new("z".repeat(43)).unwrap(), task.id, 1500),
            Err(OperationError::Unauthorized)
        );
        let approved = state.grant_approval(token(), task.id, 1500).unwrap();
        assert_eq!(
            approved.approval.binding.as_ref().unwrap().owner,
            task.command.owner
        );
        state.sign_out(token()).unwrap();
        assert_eq!(state.load_task(task.id).unwrap(), approved);
    }
    #[test]
    fn populated_version_two_migration_preserves_worker_and_session() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../../../migrations/0001.sql"))
            .unwrap();
        db.execute_batch(include_str!("../../../migrations/0002.sql"))
            .unwrap();
        let bot = BotId::new();
        let worker = WorkerId::new();
        let session = ComputerSessionId::new();
        db.execute(
            "INSERT INTO bots VALUES(?1,'Existing bot',123)",
            [bot.to_string()],
        )
        .unwrap();
        db.execute(
            "INSERT INTO workers VALUES(?1,?2,0,19)",
            params![worker.to_string(), "b".repeat(64)],
        )
        .unwrap();
        db.execute(
            "INSERT INTO computer_sessions VALUES(?1,?2,?3,'owner',23)",
            params![session.to_string(), bot.to_string(), worker.to_string()],
        )
        .unwrap();
        migrate(&mut db).unwrap();
        assert_eq!(db.query_row::<(i64,String,i64),_,_>("SELECT workers.generation,computer_sessions.owner,computer_sessions.revision FROM workers JOIN computer_sessions ON worker_id=workers.id",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap(),(19,"owner".into(),23));
        assert_eq!(
            db.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
                .unwrap(),
            5
        );
    }
}
