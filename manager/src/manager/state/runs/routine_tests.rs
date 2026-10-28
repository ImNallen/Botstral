use super::*;
use sha2::Digest;

fn definition(f: &Fixture, trigger: RoutineTrigger) -> RoutineDefinition {
    RoutineDefinition {
        bot: f.input.bot,
        name: "Durable work".into(),
        trigger,
        catchup_seconds: 300,
        wakeups: None,
        template: RunTemplate {
            session: f.input.session,
            session_revision: f
                .state
                .find_computer_session(f.input.session)
                .unwrap()
                .revision,
            configuration_revision: 1,
            permission_revision: 1,
            prompt: f.input.prompt.clone(),
            execution_policy: f.input.execution_policy.clone(),
            duration_seconds: 500,
            max_turns: 4,
            max_total_tokens: 200_000,
            max_cost_microusd: 200_000,
            max_output_tokens: 64,
            max_operations: 12,
        },
    }
}
fn command(
    state: &mut State,
    action: RoutineAction,
) -> Result<RoutineControlResult, OperationError> {
    state.routine_command(
        token(),
        RoutineCommand {
            id: RequestId::new(),
            action,
        },
    )
}
fn create(f: &mut Fixture, trigger: RoutineTrigger) -> RoutineId {
    command(
        &mut f.state,
        RoutineAction::SetBudget {
            budget: RoutineBudget {
                max_runs: 100,
                max_tokens: 20_000_000,
                max_cost_microusd: 20_000_000,
            },
        },
    )
    .unwrap();
    let id = RoutineId::new();
    let definition = Box::new(definition(f, trigger));
    command(&mut f.state, RoutineAction::Create { id, definition }).unwrap();
    id
}
fn history(f: &Fixture, id: RoutineId) -> Vec<RoutineOccurrence> {
    f.state.routine_history(token(), id, None, 32).unwrap()
}
fn finish(f: &mut Fixture, id: RunId) {
    let p = claim(&mut f.state);
    assert_eq!(p.run, id);
    f.state
        .finish_run_call(id, p.turn.id, p.turn.attempt, response(None))
        .unwrap();
    f.state.reconcile_routines().unwrap();
}
#[test]
fn p03_atomic_admission_restart_identity_and_more_than_sixteen_completions() {
    let mut f = fixture();
    let id = create(
        &mut f,
        RoutineTrigger::Interval {
            anchor: 1000,
            seconds: 60,
        },
    );
    f.state.connection.execute_batch("CREATE TRIGGER fail_occurrence BEFORE INSERT ON routine_occurrences BEGIN SELECT RAISE(ABORT,'crash before admission'); END;").unwrap();
    assert_eq!(f.state.reconcile_routines(), Err(OperationError::Internal));
    assert!(f.state.run_summaries().unwrap().is_empty());
    assert_eq!(f.state.routine_views().unwrap()[0].next_at, Some(1000));
    f.state
        .connection
        .execute_batch("DROP TRIGGER fail_occurrence")
        .unwrap();
    for index in 0..20 {
        f.state.test_time = Some(1000 + index * 60);
        f.state.reconcile_routines().unwrap();
        let o = history(&f, id).remove(0);
        let run = o.run.unwrap_or_else(|| panic!("{o:?}"));
        f.state.reconcile_routines().unwrap();
        assert_eq!(history(&f, id)[0].run, Some(run));
        finish(&mut f, run);
    }
    assert_eq!(history(&f, id).len(), 20);
    assert!(f.state.run_summaries().unwrap().len() <= 16);
    let n: i64 = f
        .state
        .connection
        .query_row("SELECT count(*) FROM routine_reservations", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(n, 20);
    let counters: (i64, i64, i64) = f
        .state
        .connection
        .query_row(
            "SELECT admitted_runs,tokens,cost FROM routine_budget",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    let ledger: (i64, i64, i64) = f
        .state
        .connection
        .query_row(
            "SELECT count(*),sum(tokens),sum(cost) FROM routine_reservations",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(counters, ledger);

    assert_eq!(f.state.inbox().unwrap().len(), 20);
    let archived: Vec<String> = f
        .state
        .connection
        .prepare("SELECT receipt FROM run_turn_receipts")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(archived.len(), 4);
    for receipt in archived {
        let receipt: serde_json::Value = serde_json::from_str(&receipt).unwrap();
        assert_eq!(receipt["provider_request_id"], "recorded-response");
        assert_eq!(receipt["text"], "Recorded response");
    }
    let oldest = history(&f, id).last().unwrap().run.unwrap();
    let archived = WorkspaceRun::project(f.state.run(token(), oldest).unwrap(), false);
    assert_eq!(
        archived
            .turns
            .iter()
            .filter_map(|turn| turn.text.as_deref())
            .collect::<Vec<_>>(),
        ["Recorded response"]
    );

    f.state.test_time = Some(1000);
    command(
        &mut f.state,
        RoutineAction::SetPaused {
            id,
            expected_revision: 1,
            paused: true,
        },
    )
    .unwrap();
    command(
        &mut f.state,
        RoutineAction::SetPaused {
            id,
            expected_revision: 2,
            paused: false,
        },
    )
    .unwrap();
    f.state.test_time = Some(1060);
    f.state.reconcile_routines().unwrap();
    assert_eq!(history(&f, id).len(), 20);
    let admitted: i64 = f
        .state
        .connection
        .query_row("SELECT admitted_runs FROM routine_budget", [], |r| r.get(0))
        .unwrap();
    assert_eq!(admitted, 20);
}
#[test]
fn p03_archived_owner_run_preserves_ordered_text_and_preview() {
    let mut f = fixture();
    let first = f.input.id;
    let mut last = first;
    for index in 0..17 {
        let mut input = f.input.clone();
        if index > 0 {
            input.id = RunId::new();
        }
        last = input.id;
        f.state.submit_run(token(), input).unwrap();
        finish(&mut f, last);
    }
    let mut view = f.state.run(token(), first).unwrap();
    assert!(view.turns.is_empty());
    assert_eq!(view.archived_turns.len(), 1);
    assert_eq!(
        view.archived_turns[0].text.as_deref(),
        Some("Recorded response")
    );
    assert_eq!(
        view.archived_turns[0].provider_request_id.as_deref(),
        Some("recorded-response")
    );
    let long_response = "🦀".repeat(600);
    view.archived_turns[0].text = Some(long_response.clone());
    let mut live = f.state.run(token(), last).unwrap().turns.remove(0);
    live.ordinal = 2;
    view.turns.push(live);
    let preview = WorkspaceRun::project(view.clone(), true);
    assert_eq!(
        preview
            .turns
            .iter()
            .map(|turn| turn.ordinal)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    assert_eq!(
        preview.turns[0].text.as_deref(),
        Some("🦀".repeat(256).as_str())
    );
    assert!(preview.turns[0].truncated);
    let full = WorkspaceRun::project(view, false);
    assert_eq!(full.turns[0].text.as_deref(), Some(long_response.as_str()));
    assert!(!full.turns[0].truncated);
}

#[test]
fn p03_locked_vault_stale_bindings_budget_and_pause_missed_overlap() {
    let mut f = fixture();
    let id = create(
        &mut f,
        RoutineTrigger::Interval {
            anchor: 1000,
            seconds: 60,
        },
    );
    f.state.reconcile_routines().unwrap();
    let o = history(&f, id).remove(0);
    let run = o.run.unwrap_or_else(|| panic!("{o:?}"));
    f.state.test_time = Some(1060);
    f.state.reconcile_routines().unwrap();
    assert!(matches!(
        history(&f, id)[0].status,
        OccurrenceStatus::Skipped { .. }
    ));
    let p = claim(&mut f.state);
    f.state
        .finish_run_call(
            run,
            p.turn.id,
            p.turn.attempt,
            ModelOutcome::Unknown {
                reason: InferenceAttentionReason::Restart,
                request_id: None,
            },
        )
        .unwrap();
    f.state.test_time = Some(1120);
    f.state.reconcile_routines().unwrap();
    assert!(matches!(
        history(&f, id)[0].status,
        OccurrenceStatus::Skipped { .. }
    ));
    command(
        &mut f.state,
        RoutineAction::SetPaused {
            id,
            expected_revision: 1,
            paused: true,
        },
    )
    .unwrap();
    f.state.test_time = Some(1600);
    f.state.reconcile_routines().unwrap();
    let n = history(&f, id).len();
    command(
        &mut f.state,
        RoutineAction::SetPaused {
            id,
            expected_revision: 2,
            paused: false,
        },
    )
    .unwrap();
    assert_eq!(f.state.routine_views().unwrap()[0].next_at, Some(1660));
    f.state.reconcile_routines().unwrap();
    assert_eq!(history(&f, id).len(), n);
    let mut other = fixture();
    let one = create(&mut other, RoutineTrigger::Once { at: 1000 });
    other.state.lock_vault(token()).unwrap();
    other.state.reconcile_routines().unwrap();
    assert!(
        matches!(&history(&other,one)[0].status,OccurrenceStatus::Blocked {reason} if reason=="Locked")
    );
    let mut stale = fixture();
    let one = create(&mut stale, RoutineTrigger::Once { at: 1000 });
    stale
        .state
        .connection
        .execute("UPDATE computer_sessions SET revision=revision+1", [])
        .unwrap();
    stale.state.reconcile_routines().unwrap();
    assert!(matches!(
        history(&stale, one)[0].status,
        OccurrenceStatus::Blocked { .. }
    ));
    let mut budget = fixture();
    let one = create(&mut budget, RoutineTrigger::Once { at: 1000 });
    command(
        &mut budget.state,
        RoutineAction::SetBudget {
            budget: RoutineBudget {
                max_runs: 1,
                max_tokens: 1,
                max_cost_microusd: 1,
            },
        },
    )
    .unwrap();
    budget.state.reconcile_routines().unwrap();
    assert!(matches!(
        history(&budget, one)[0].status,
        OccurrenceStatus::Blocked { .. }
    ));
    let mut missed = fixture();
    let one = create(
        &mut missed,
        RoutineTrigger::Interval {
            anchor: 1000,
            seconds: 60,
        },
    );
    missed.state.test_time = Some(2000);
    missed.state.reconcile_routines().unwrap();
    assert_eq!(history(&missed, one)[0].scheduled_at, 1960);
    assert_eq!(history(&missed, one)[0].skipped_count, 16);
}
#[test]
fn p03_owner_commands_replay_tombstone_and_paused_copies() {
    let mut f = fixture();
    let id = create(&mut f, RoutineTrigger::Once { at: 1000 });
    let cmd = RoutineCommand {
        id: RequestId::new(),
        action: RoutineAction::Test {
            id,
            expected_revision: 1,
        },
    };
    assert_eq!(
        f.state.routine_command(token(), cmd.clone()),
        f.state.routine_command(token(), cmd.clone())
    );
    assert_eq!(history(&f, id).len(), 1);
    let mut conflict = cmd;
    conflict.action = RoutineAction::Delete {
        id,
        expected_revision: 1,
    };
    assert_eq!(
        f.state.routine_command(token(), conflict),
        Err(OperationError::Conflict)
    );
    let copy = BotId::new();
    command(
        &mut f.state,
        RoutineAction::DuplicateBot {
            bot: f.input.bot,
            copy,
            name: "Copy".into(),
        },
    )
    .unwrap();
    let copied = f
        .state
        .routine_views()
        .unwrap()
        .into_iter()
        .find(|r| r.definition.bot == copy)
        .unwrap();
    assert_eq!(copied.state, RoutineState::Paused);
    assert_ne!(copied.id, id);
    assert!(copied.definition.wakeups.is_none());
    assert_ne!(copied.definition.template.session, f.input.session);
    let admitted = history(&f, id)[0].run.unwrap();
    let call = claim(&mut f.state);
    f.state
        .finish_run_call(
            admitted,
            call.turn.id,
            call.turn.attempt,
            response(Some(process_call("delete-fence"))),
        )
        .unwrap();
    let task = f.state.load_run(admitted).unwrap().turns[0]
        .intent
        .as_ref()
        .unwrap()
        .task;
    let permit = f.approve_and_claim(task);
    command(&mut f.state, RoutineAction::DeleteBot { bot: f.input.bot }).unwrap();
    assert_eq!(
        f.state.load_task(task).unwrap().state,
        TaskState::Cancelling
    );
    let cancellation=f.drain().into_iter().find(|message|matches!(message,ManagerMessage::Delivery(delivery) if delivery.cancel && delivery.body.task==task)).unwrap();
    assert!(f.state.admit_delivery(f.lease, &cancellation).is_ok());
    assert!(f.state.admit_delivery(f.lease, &permit).is_err());
    assert_eq!(
        f.state.require_live_bot(f.input.bot),
        Err(OperationError::NotFound)
    );
    f.input.id = RunId::new();
    assert_eq!(
        f.state.submit_run(token(), f.input.clone()),
        Err(OperationError::NotFound)
    );
    assert!(
        f.state
            .routine_views()
            .unwrap()
            .iter()
            .all(|r| r.definition.bot != f.input.bot)
    );
}
#[test]
fn p03_authenticated_event_receipt_conflict_and_owner_conversation() {
    let mut f = fixture();
    let publisher = PublisherId::new();
    let credential = "e".repeat(48);
    let hash = format!("{:x}", sha2::Sha256::digest(credential.as_bytes()));
    command(
        &mut f.state,
        RoutineAction::InstallPublisher {
            id: publisher,
            bot: f.input.bot,
            name: "Builds".into(),
            topics: vec!["build.completed".into()],
            credential_sha256: hash,
        },
    )
    .unwrap();
    let id = create(
        &mut f,
        RoutineTrigger::Event {
            publisher,
            topic: "build.completed".into(),
            equals: BTreeMap::from([("branch".into(), EventScalar::Text("main".into()))]),
        },
    );
    let mut body = PublisherEvent {
        id: "build-1".into(),
        timestamp: 1000,
        topic: "build.completed".into(),
        fields: BTreeMap::from([("branch".into(), EventScalar::Text("main".into()))]),
    };
    let bytes = serde_json::to_vec(&body).unwrap();
    assert_eq!(
        f.state
            .ingest_routine_event(publisher, "x".repeat(48), bytes.clone()),
        Err(OperationError::Unauthorized)
    );
    let receipt = f
        .state
        .ingest_routine_event(publisher, credential.clone(), bytes.clone())
        .unwrap();
    assert_eq!(receipt.matched, 1);
    assert_eq!(
        f.state
            .ingest_routine_event(publisher, credential.clone(), bytes)
            .unwrap(),
        receipt
    );
    assert_eq!(history(&f, id).len(), 1);
    let admitted = f.state.load_run(history(&f, id)[0].run.unwrap()).unwrap();
    assert!(
        admitted
            .request
            .prompt
            .contains("Untrusted publisher event data")
    );
    assert!(admitted.request.prompt.contains("build-1"));
    body.fields
        .insert("branch".into(), EventScalar::Text("other".into()));
    assert_eq!(
        f.state.ingest_routine_event(
            publisher,
            credential.clone(),
            serde_json::to_vec(&body).unwrap()
        ),
        Err(OperationError::Conflict)
    );
    let help = f
        .state
        .routine_conversation(
            token(),
            f.input.bot,
            RequestId::new(),
            "/routine help".into(),
        )
        .unwrap();
    assert!(help.message.contains("Live") || help.message.contains("real bounded"));
    f.state
        .routine_conversation(
            token(),
            f.input.bot,
            RequestId::new(),
            format!("/routine pause {id} 1"),
        )
        .unwrap();
    assert_eq!(
        f.state.routine_views().unwrap()[0].state,
        RoutineState::Paused
    );
    let original = f
        .state
        .routine_history(token(), id, None, 32)
        .unwrap()
        .into_iter()
        .find_map(|o| o.run)
        .unwrap();
    let mut replacement = f.state.routine_views().unwrap()[0].definition.clone();
    replacement.trigger = RoutineTrigger::Interval {
        anchor: 1000,
        seconds: 60,
    };
    command(
        &mut f.state,
        RoutineAction::Replace {
            id,
            expected_revision: 2,
            definition: Box::new(replacement),
        },
    )
    .unwrap();
    command(
        &mut f.state,
        RoutineAction::RevokePublisher {
            id: publisher,
            expected_revision: 1,
        },
    )
    .unwrap();
    assert!(!f.state.routine_run_authorized(original).unwrap());
    assert_eq!(
        f.state
            .ingest_routine_event(publisher, credential, serde_json::to_vec(&body).unwrap()),
        Err(OperationError::Unauthorized)
    );
}
#[test]
fn p03_wakeup_atomic_result_replay_and_chain_share_one_budget() {
    let mut f = fixture();
    let id = create(&mut f, RoutineTrigger::Once { at: 1000 });
    let mut def = f.state.routine_views().unwrap()[0].definition.clone();
    def.wakeups = Some(WakeupGrant {
        horizon_seconds: 3600,
        minimum_spacing_seconds: 60,
        max_pending: 2,
        max_total: 2,
    });
    command(
        &mut f.state,
        RoutineAction::Replace {
            id,
            expected_revision: 1,
            definition: Box::new(def),
        },
    )
    .unwrap();
    command(
        &mut f.state,
        RoutineAction::Test {
            id,
            expected_revision: 2,
        },
    )
    .unwrap();
    let run = history(&f, id)[0].run.unwrap();
    let p = claim(&mut f.state);
    assert!(p.prepared.encoded().contains("request_wakeup"));
    let outcome = || {
        response(Some(RunToolCall {
            id: "wake-1".into(),
            proposal: ToolProposal::RequestWakeup(RequestWakeup {
                at: 1060,
                prompt: "Follow up".into(),
            }),
        }))
    };
    f.state.connection.execute_batch("CREATE TRIGGER fail_wakeup BEFORE INSERT ON routine_wakeups BEGIN SELECT RAISE(ABORT,'atomic local result'); END").unwrap();
    assert_eq!(
        f.state
            .finish_run_call(run, p.turn.id, p.turn.attempt, outcome()),
        Err(OperationError::Internal)
    );
    assert!(
        f.state.load_run(run).unwrap().turns[0]
            .manager_result
            .is_none()
    );
    assert_eq!(history(&f, id).len(), 1);
    f.state
        .connection
        .execute_batch("DROP TRIGGER fail_wakeup")
        .unwrap();
    f.state
        .finish_run_call(run, p.turn.id, p.turn.attempt, outcome())
        .unwrap();
    f.state
        .finish_run_call(run, p.turn.id, p.turn.attempt, outcome())
        .unwrap();
    assert_eq!(history(&f, id).len(), 2);
    let view = f.state.load_run(run).unwrap();
    assert!(view.turns[0].intent.is_none());
    assert!(view.turns[0].manager_result.is_some());
    finish(&mut f, run);
    f.state.test_time = Some(1060);
    f.state.reconcile_routines().unwrap();
    let child = history(&f, id)[0].run.unwrap();
    let p = claim(&mut f.state);
    assert!(
        matches!(p.turn.messages.first(),Some(RunMessage::User {content}) if content=="Follow up")
    );
    f.state
        .finish_run_call(
            child,
            p.turn.id,
            p.turn.attempt,
            response(Some(RunToolCall {
                id: "wake-2".into(),
                proposal: ToolProposal::RequestWakeup(RequestWakeup {
                    at: 1120,
                    prompt: "Second".into(),
                }),
            })),
        )
        .unwrap();
    finish(&mut f, child);
    f.state.test_time = Some(1120);
    f.state.reconcile_routines().unwrap();
    let child2 = history(&f, id)[0].run.unwrap();
    let p = claim(&mut f.state);
    f.state
        .finish_run_call(
            child2,
            p.turn.id,
            p.turn.attempt,
            response(Some(RunToolCall {
                id: "wake-3".into(),
                proposal: ToolProposal::RequestWakeup(RequestWakeup {
                    at: 1180,
                    prompt: "Cannot replenish".into(),
                }),
            })),
        )
        .unwrap();
    let view = f.state.load_run(child2).unwrap();
    assert!(
        view.turns[0]
            .manager_result
            .as_ref()
            .unwrap()
            .result
            .contains("blocked")
    );
    assert_eq!(history(&f, id).len(), 3);
}
#[test]
fn p03_sqlite_restart_before_and_after_admission_and_inbox_ack() {
    for boundary in ["before", "after", "completed", "completed_before_inbox"] {
        let mut f = fixture();
        let id = create(&mut f, RoutineTrigger::Once { at: 1000 });
        if boundary != "before" {
            f.state.reconcile_routines().unwrap();
        }
        let original = history(&f, id).first().and_then(|o| o.run);
        if boundary == "completed" {
            finish(&mut f, original.unwrap());
        }
        if boundary == "completed_before_inbox" {
            let call = claim(&mut f.state);
            f.state
                .finish_run_call(call.run, call.turn.id, call.turn.attempt, response(None))
                .unwrap();
            assert!(f.state.inbox().unwrap().is_empty());
        }
        super::restart_tests::reopen(&mut f);
        f.state.reconcile_routines().unwrap();
        let rows = history(&f, id);
        assert_eq!(rows.len(), 1);
        if let Some(original) = original {
            assert_eq!(rows[0].run, Some(original));
        }
        if matches!(boundary, "completed" | "completed_before_inbox") {
            let inbox = f.state.inbox().unwrap();
            assert_eq!(inbox.len(), 1);
            command(
                &mut f.state,
                RoutineAction::AcknowledgeInbox { id: inbox[0].id },
            )
            .unwrap();
            assert!(f.state.inbox().unwrap().is_empty());
        }
    }
}
#[test]
fn p03_current_configuration_and_shared_aggregate_reservations_block_admission() {
    let mut f = fixture();
    let a = create(&mut f, RoutineTrigger::Once { at: 1000 });
    let b = RoutineId::new();
    let definition = Box::new(definition(&f, RoutineTrigger::Once { at: 1000 }));
    command(&mut f.state, RoutineAction::Create { id: b, definition }).unwrap();
    command(
        &mut f.state,
        RoutineAction::SetBudget {
            budget: RoutineBudget {
                max_runs: 10,
                max_tokens: 200_000,
                max_cost_microusd: 200_000,
            },
        },
    )
    .unwrap();
    f.state.reconcile_routines().unwrap();
    assert!(history(&f, a)[0].run.is_some());
    assert!(matches!(
        history(&f, b)[0].status,
        OccurrenceStatus::Blocked { .. }
    ));
    let mut f = fixture();
    let id = create(&mut f, RoutineTrigger::Once { at: 1000 });
    let config = f
        .state
        .load_inference_configuration(f.input.bot, 1)
        .unwrap();
    f.state
        .configure_inference(
            token(),
            ConfigureInference {
                bot: f.input.bot,
                expected_revision: 1,
                policy: config.policy,
            },
        )
        .unwrap();
    f.state.reconcile_routines().unwrap();
    assert!(
        matches!(&history(&f,id)[0].status,OccurrenceStatus::Blocked {reason} if reason=="InferenceUnverified")
    );
}

#[test]
fn p03_compact_workspace_preview_keeps_full_text_available_and_fits_bound() {
    let mut f = fixture();
    f.state.submit_run(token(), f.input.clone()).unwrap();
    let p = claim(&mut f.state);
    f.state
        .finish_run_call(p.run, p.turn.id, p.turn.attempt, response(None))
        .unwrap();
    let mut view = f.state.load_run(p.run).unwrap();
    let full = "😀".repeat(1024);
    view.turns[0].response = Some(RunMessage::Assistant {
        content: full.clone(),
        tool_call: None,
    });
    view.turns[0].manager_result = Some(ManagerToolResult {
        call: RunToolCall {
            id: "local".into(),
            proposal: ToolProposal::RequestWakeup(RequestWakeup {
                at: 2000,
                prompt: "later".into(),
            }),
        },
        result: "\0".repeat(MAX_RUN_RESULT_BYTES),
    });
    view.turns = vec![view.turns[0].clone(); MAX_RUN_TURNS as usize];
    let preview = WorkspaceRun::project(view.clone(), true);
    assert!(preview.turns.iter().all(|t| t.truncated));
    assert_eq!(preview.turns[0].text.as_ref().unwrap().len(), 1024);
    assert!(json(&preview).unwrap().len() < MAX_WORKSPACE_RUN_BYTES);
    let detail = WorkspaceRun::project(view, false);
    assert_eq!(detail.turns[0].text.as_ref(), Some(&full));
    assert!(detail.turns.iter().all(|t| !t.truncated));
}

#[test]
fn p03_authenticated_conversation_crud_and_model_text_cannot_manage_routines() {
    let mut f = fixture();
    command(
        &mut f.state,
        RoutineAction::SetBudget {
            budget: RoutineBudget {
                max_runs: 10,
                max_tokens: 2_000_000,
                max_cost_microusd: 2_000_000,
            },
        },
    )
    .unwrap();
    let bot = f.input.bot;
    let request = RequestId::new();
    let id = RoutineId(request.0);
    let mut d = definition(&f, RoutineTrigger::Once { at: 1300 });
    let create = format!("/routine create {}", json(&d).unwrap());
    f.state
        .routine_conversation(token(), bot, request, create.clone())
        .unwrap();
    assert_eq!(f.state.routine_views().unwrap().len(), 1);
    d.name = "Edited in conversation".into();
    for text in [
        format!("/routine edit {id} 1 {}", json(&d).unwrap()),
        format!("/routine test {id} 2"),
        format!("/routine history {id}"),
        format!("/routine pause {id} 2"),
        format!("/routine resume {id} 3"),
        format!("/routine delete {id} 4"),
    ] {
        f.state
            .routine_conversation(token(), bot, RequestId::new(), text)
            .unwrap();
    }
    assert_eq!(history(&f, id).len(), 1);
    assert!(f.state.routine_views().unwrap().is_empty());
    f.state
        .routine_conversation(token(), bot, request, create)
        .unwrap();
    assert!(f.state.routine_views().unwrap().is_empty());
    let mut f = fixture();
    f.state.submit_run(token(), f.input.clone()).unwrap();
    let p = claim(&mut f.state);
    let mut text = response(None);
    if let ModelOutcome::Billed { response, .. } = &mut text {
        *response = Ok(RunMessage::Assistant {
            content: "/routine delete all".into(),
            tool_call: None,
        });
    }
    f.state
        .finish_run_call(p.run, p.turn.id, p.turn.attempt, text)
        .unwrap();
    assert!(f.state.routine_views().unwrap().is_empty());
    let mut f = fixture();
    f.state.submit_run(token(), f.input.clone()).unwrap();
    let p = claim(&mut f.state);
    f.state
        .finish_run_call(
            p.run,
            p.turn.id,
            p.turn.attempt,
            response(Some(RunToolCall {
                id: "ungranted".into(),
                proposal: ToolProposal::RequestWakeup(RequestWakeup {
                    at: 1100,
                    prompt: "Unauthorized wakeup".into(),
                }),
            })),
        )
        .unwrap();
    let run = f.state.load_run(p.run).unwrap();
    assert!(
        run.turns[0]
            .manager_result
            .as_ref()
            .unwrap()
            .result
            .contains("Unauthorized")
    );
    assert!(run.turns[0].intent.is_none());
    assert!(f.state.routine_recent().unwrap().is_empty());
}

#[test]
fn p03_reusable_template_refreshes_parent_and_nested_coding_deadlines() {
    let mut f = fixture();
    let id = create(&mut f, RoutineTrigger::Once { at: 9000 });
    let mut d = f.state.routine_views().unwrap()[0].definition.clone();
    d.template.execution_policy.vibe = Some(VibeToolPolicy {
        policy: VibePolicy {
            connectors: None,
            workspace: CodingWorkspace {
                id: CodingWorkspaceId::new(),
                path: "/srv/coding/fixture".into(),
            },
            runtime: CertificateFingerprint::from_der(b"routine Vibe"),
            account: ProcessAccount {
                uid: 1000,
                gid: 1000,
            },
            resources: ProcessLimits {
                runtime_seconds: 60,
                output_bytes: 1024,
                processes: 32,
                memory_bytes: 268435456,
            },
            configuration_revision: 1,
            model_id: "mistral-small-2506".into(),
            access: VibeAccess::EditFilesAsGuestAccount,
            outputs: vec![],
        },
        limits: RunLimits {
            deadline: 120,
            max_turns: 3,
            max_operations: 1,
            ..f.input.limits.clone()
        },
    });
    command(
        &mut f.state,
        RoutineAction::Replace {
            id,
            expected_revision: 1,
            definition: Box::new(d),
        },
    )
    .unwrap();
    for now in [1000, 2000] {
        f.state.test_time = Some(now);
        command(
            &mut f.state,
            RoutineAction::Test {
                id,
                expected_revision: 2,
            },
        )
        .unwrap();
        let run = f.state.load_run(history(&f, id)[0].run.unwrap()).unwrap();
        assert_eq!(run.request.limits.deadline, now + 500);
        assert_eq!(
            run.request
                .execution_policy
                .vibe
                .as_ref()
                .unwrap()
                .limits
                .deadline,
            now + 120
        );
        finish(&mut f, run.summary.id);
    }
    assert_eq!(
        f.state.routine_views().unwrap()[0]
            .definition
            .template
            .execution_policy
            .vibe
            .as_ref()
            .unwrap()
            .limits
            .deadline,
        120
    );
}

#[test]
fn p03_each_exact_approval_has_a_new_durable_inbox_decision() {
    let mut f = fixture();
    let id = create(&mut f, RoutineTrigger::Once { at: 1000 });
    f.state.reconcile_routines().unwrap();
    let run = history(&f, id)[0].run.unwrap();
    let mut decisions = Vec::new();
    for call in ["first-decision", "second-decision"] {
        let p = claim(&mut f.state);
        f.state
            .finish_run_call(
                run,
                p.turn.id,
                p.turn.attempt,
                response(Some(process_call(call))),
            )
            .unwrap();
        let view = f.state.load_run(run).unwrap();
        let mut task = f
            .state
            .load_task(view.turns.last().unwrap().intent.as_ref().unwrap().task)
            .unwrap();
        task.state = TaskState::AwaitingApproval;
        tasks::save(&f.state.connection, &task).unwrap();
        f.state.reconcile_routines().unwrap();
        let inbox = f.state.inbox().unwrap();
        assert_eq!(inbox.len(), 1);
        assert!(
            matches!(&inbox[0].status,OccurrenceStatus::Decision {reason} if reason.contains(&task.id.to_string()))
        );
        decisions.push(inbox[0].id);
        command(
            &mut f.state,
            RoutineAction::AcknowledgeInbox { id: inbox[0].id },
        )
        .unwrap();
        task.state = TaskState::Completed;
        tasks::save(&f.state.connection, &task).unwrap();
        f.state
            .finish_run_result(run, p.turn.id, task, Ok("Synthetic task result".into()))
            .unwrap();
        f.state.reconcile_routines().unwrap();
    }
    assert_ne!(decisions[0], decisions[1]);
    assert!(f.state.inbox().unwrap().is_empty());
}
