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
    assert_eq!(f.state.inbox().unwrap().len(), 20);
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
    command(&mut f.state, RoutineAction::DeleteBot { bot: f.input.bot }).unwrap();
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
    command(
        &mut f.state,
        RoutineAction::RevokePublisher {
            id: publisher,
            expected_revision: 1,
        },
    )
    .unwrap();
    assert_eq!(
        f.state
            .ingest_routine_event(publisher, credential, serde_json::to_vec(&body).unwrap()),
        Err(OperationError::Unauthorized)
    );
}
