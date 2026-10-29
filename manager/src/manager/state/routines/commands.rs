use super::*;

impl State {
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
        let test = if let RoutineAction::Test {
            id,
            expected_revision,
        } = &input.action
        {
            let r = self.load_routine(*id)?;
            if r.revision != *expected_revision {
                return Err(OperationError::Conflict);
            }
            let (o, run) =
                self.prepare_occurrence(&r, format!("test:{}", input.id), now, 0, None, None)?;
            Some((r, o, run))
        } else {
            None
        };
        let tx = self.connection.unchecked_transaction()?;
        let message = match &input.action {
            RoutineAction::Create { id, definition } => {
                self.validate_definition(definition)?;
                routine_capacity(&tx)?;
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
            RoutineAction::Test { .. } => {
                let (r, o, run) = test.ok_or(OperationError::Internal)?;
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
                tx.execute("INSERT INTO routine_budget(id,view) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET view=excluded.view",[json(budget)?])?;
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
                let active:i64=tx.query_row("SELECT count(*) FROM routine_publishers WHERE json_extract(view,'$.revoked')=0",[],|row|row.get(0))?;
                if active >= MAX_PUBLISHERS as i64 {
                    return Err(OperationError::Busy);
                }
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
                    .prepare("SELECT view FROM tasks WHERE bot=?1 AND json_extract(view,'$.state') IN ('queued','running','awaiting_approval','cancelling','awaiting_attention')")?
                    .query_map([bot.to_string()], |r| r.get::<_, String>(0))?
                    .map(|r| parse::<TaskView>(r?))
                    .collect::<Result<Vec<_>, _>>()?;
                for mut task in tasks {
                    if !tasks::execution_pending(&task) {
                        continue;
                    }
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
                    routine_capacity(&tx)?;
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

fn routine_capacity(db: &Connection) -> Result<(), OperationError> {
    let count: i64 = db.query_row(
        "SELECT count(*) FROM routines WHERE state IN ('enabled','paused')",
        [],
        |row| row.get(0),
    )?;
    if count >= MAX_ROUTINES as i64 {
        Err(OperationError::Busy)
    } else {
        Ok(())
    }
}
