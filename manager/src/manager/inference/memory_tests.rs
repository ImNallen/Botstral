use super::*;

fn chat_request(app: &App) -> SubmitRun {
    let mut input = request(app, ComputerSessionId::new());
    input.session = None;
    input.execution_policy = RunExecutionPolicy {
        computer: None,
        process: None,
        vibe: None,
        vision: None,
        connectors: None,
    };
    input.prompt = "Manage the requested preference through memory.".into();
    input
}
fn tool_reply(name: &str, arguments: Value) -> Reply {
    let mut body: Value = serde_json::from_str(COMPLETION).unwrap();
    body["choices"][0]["finish_reason"] = json!("tool_calls");
    body["choices"][0]["message"] = json!({"role":"assistant","content":null,"tool_calls":[{"id":"memory-http-call","type":"function","function":{"name":name,"arguments":arguments.to_string()}}]});
    Reply {
        body: body.to_string(),
        ..Reply::default()
    }
}
async fn operation(
    app: &App,
    fixture: &Fixture,
    name: &str,
    arguments: Value,
    fail_followup: bool,
) -> RunView {
    *fixture.state.script.lock().unwrap() = [
        tool_reply(name, arguments),
        if fail_followup {
            Reply {
                status: StatusCode::SERVICE_UNAVAILABLE,
                body: ERROR.into(),
                ..Reply::default()
            }
        } else {
            Reply::default()
        },
    ]
    .into();
    let input = chat_request(app);
    app.call("POST", "/v1/runs", json!(input), 200).await;
    phase(app, input.id, RunPhase::terminal).await
}
fn outcome(run: &RunView) -> &MemoryOutcome {
    let Some(ManagerOutcome::Memory { outcome }) =
        &run.turns[0].manager_result.as_ref().unwrap().outcome
    else {
        panic!("committed memory outcome")
    };
    outcome
}
fn created(run: &RunView) -> MemoryEntry {
    let MemoryOutcome::Created { entry } = outcome(run) else {
        panic!("created entry")
    };
    assert_eq!(
        entry.source,
        MemorySource::Run {
            bot: run.request.bot,
            run: run.request.id,
            turn: run.turns[0].id
        }
    );
    entry.clone()
}

#[tokio::test]
async fn conversational_memory_real_provider_crud_both_scopes_restart_and_failed_followup() {
    let root = tempfile::tempdir().unwrap();
    let local = private_local(&root);
    let fixture = Fixture::start().await;
    let mut app = App::start(local.directory().into(), &fixture).await;
    app.initialize().await;
    let bot = app.bot;
    let mut evidence = Vec::new();
    let mut survivors = Vec::new();
    for scope in [
        MemoryScopeSelection::Individual,
        MemoryScopeSelection::Shared,
    ] {
        let first_run = operation(
            &app,
            &fixture,
            "memory",
            json!(MemoryProposal::Create {
                scope,
                markdown: "Use concise answers.".into()
            }),
            false,
        )
        .await;
        let first = created(&first_run);
        assert_eq!(first.scope, scope.resolve(bot));
        evidence.push(first_run);
        let second_run = operation(
            &app,
            &fixture,
            "memory",
            json!(MemoryProposal::Create {
                scope,
                markdown: "Keep morning meetings.".into()
            }),
            false,
        )
        .await;
        let second = created(&second_run);
        survivors.push(second.clone());
        evidence.push(second_run);
        let update = operation(
            &app,
            &fixture,
            "memory",
            json!(MemoryProposal::Update {
                scope,
                id: first.id,
                expected_revision: 1,
                markdown: "Use short, complete sentences.".into()
            }),
            true,
        )
        .await;
        assert!(matches!(
            update.summary.phase,
            RunPhase::AwaitingAttention { .. }
        ));
        let MemoryOutcome::Updated { entry } = outcome(&update) else {
            panic!()
        };
        assert_eq!(entry.revision, 2);
        assert_eq!(entry.markdown, "Use short, complete sentences.");
        assert_eq!(
            WorkspaceRun::project(update.clone(), true).turns[0].manager_outcome,
            update.turns[0].manager_result.as_ref().unwrap().outcome
        );
        evidence.push(update);
        let list = operation(
            &app,
            &fixture,
            "memory",
            json!(MemoryProposal::List { scope }),
            true,
        )
        .await;
        let MemoryOutcome::Listed { entries, .. } = outcome(&list) else {
            panic!()
        };
        assert_eq!(entries.len(), 2);
        assert!(entries.contains(&second));
        evidence.push(list);
        let conflict = operation(
            &app,
            &fixture,
            "memory",
            json!(MemoryProposal::Delete {
                scope,
                id: first.id,
                expected_revision: 1
            }),
            false,
        )
        .await;
        assert_eq!(
            outcome(&conflict),
            &MemoryOutcome::Refused {
                reason: MemoryRefusal::RevisionConflict
            }
        );
        evidence.push(conflict);
        let deleted = operation(
            &app,
            &fixture,
            "memory",
            json!(MemoryProposal::Delete {
                scope,
                id: first.id,
                expected_revision: 2
            }),
            false,
        )
        .await;
        assert_eq!(
            outcome(&deleted),
            &MemoryOutcome::Deleted {
                id: first.id,
                scope: scope.resolve(bot),
                revision: 3
            }
        );
        evidence.push(deleted);
    }
    for purpose in [VaultPurpose::ProviderKey, VaultPurpose::ReusableLogin] {
        let handoff = operation(
            &app,
            &fixture,
            "request_vault_entry",
            json!(RequestVaultEntry { purpose }),
            false,
        )
        .await;
        assert!(matches!(
            handoff.turns[0].manager_result.as_ref().unwrap().outcome,
            Some(ManagerOutcome::Vault { .. })
        ));
        evidence.push(handoff);
    }
    let captured = fixture.state.requests.lock().unwrap().clone();
    for body in &captured {
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["function"]["name"], "memory");
        assert_eq!(tools[1]["function"]["name"], "request_vault_entry");
        assert_eq!(body["messages"][0]["role"], "system");
    }
    let db = rusqlite::Connection::open(local.directory().join("manager.sqlite3")).unwrap();
    assert_eq!(
        db.query_row::<i64, _, _>("SELECT count(*) FROM tasks", [], |r| r.get(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row::<i64, _, _>("SELECT count(*) FROM workers", [], |r| r.get(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row::<i64, _, _>(
            "SELECT count(*) FROM memory_publications WHERE state='applied'",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        8
    );
    assert_eq!(
        db.query_row::<i64, _, _>(
            "SELECT count(*) FROM runs WHERE memory_entries_snapshot IS NULL",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        0
    );
    drop(db);
    let other_bot = BotId::new();
    app.bot = other_bot;
    app.call(
        "PUT",
        &format!("/v1/bots/{other_bot}"),
        json!({"name":"Second bot"}),
        200,
    )
    .await;
    app.configure(0).await;
    app.verify(1, 200).await;
    let other_shared = operation(
        &app,
        &fixture,
        "memory",
        json!(MemoryProposal::List {
            scope: MemoryScopeSelection::Shared
        }),
        false,
    )
    .await;
    assert_eq!(
        outcome(&other_shared),
        &MemoryOutcome::Listed {
            scope: MemoryScope::Shared,
            entries: vec![survivors[1].clone()]
        }
    );
    let foreign = operation(
        &app,
        &fixture,
        "memory",
        json!(MemoryProposal::Delete {
            scope: MemoryScopeSelection::Individual,
            id: survivors[0].id,
            expected_revision: 1
        }),
        false,
    )
    .await;
    assert_eq!(
        outcome(&foreign),
        &MemoryOutcome::Refused {
            reason: MemoryRefusal::NotFound
        }
    );
    let other_context = json!(other_shared.turns[0].messages).to_string();
    assert!(other_context.contains(&survivors[1].id.to_string()));
    assert!(!other_context.contains(&survivors[0].id.to_string()));
    app.shutdown().await;
    let mut app = App::start(local.directory().into(), &fixture).await;
    app.bot = bot;
    app.call(
        "POST",
        "/v1/vault/unlock",
        json!({"passphrase":PASSPHRASE}),
        204,
    )
    .await;
    for scope in [
        MemoryScopeSelection::Individual,
        MemoryScopeSelection::Shared,
    ] {
        let listed = operation(
            &app,
            &fixture,
            "memory",
            json!(MemoryProposal::List { scope }),
            false,
        )
        .await;
        let MemoryOutcome::Listed { entries, .. } = outcome(&listed) else {
            panic!()
        };
        assert_eq!(entries.len(), 1);
        assert!(survivors.contains(&entries[0]));
    }
    for run in &evidence {
        let recovered = view(&app, run.request.id).await;
        assert_eq!(
            recovered.turns[0].manager_result,
            run.turns[0].manager_result
        );
    }
    if let Ok(path) = std::env::var("BOTSTRAL_MEMORY_EVIDENCE") {
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(std::path::Path::new(&path).join("manager-conversations.json"), serde_json::to_vec_pretty(&json!({"bot":bot,"runs":evidence.iter().cloned().map(|r|WorkspaceRun::project(r,false)).collect::<Vec<_>>(),"requests":captured})).unwrap()).unwrap();
    }
    app.shutdown().await;
}

#[tokio::test]
async fn conversational_memory_malformed_foreign_source_duplicate_secret_and_disabled_worker_calls_are_not_published()
 {
    let root = tempfile::tempdir().unwrap();
    let local = private_local(&root);
    let fixture = Fixture::start().await;
    let app = App::start(local.directory().into(), &fixture).await;
    app.initialize().await;
    for (name, arguments) in [
        (
            "memory",
            json!({"action":"create","scope":"individual","markdown":"x","bot":BotId::new()}),
        ),
        (
            "memory",
            json!({"action":"create","scope":"individual","markdown":"x","source":{"type":"run","run":RunId::new()}}),
        ),
        (
            "request_vault_entry",
            json!({"purpose":"provider_key","value":"unknown-secret"}),
        ),
        (
            "memory",
            json!({"action":"create","scope":"shared","markdown":KEY}),
        ),
        (
            "memory",
            json!({"action":"create","scope":"shared","markdown":serde_json::to_string(KEY).unwrap()}),
        ),
        (
            "process",
            json!({"executable":"/bin/true","args":[],"cwd":"/"}),
        ),
        ("computer", json!({"operations":[],"outputs":[]})),
    ] {
        fixture.state.script.lock().unwrap().clear();
        fixture.reply(tool_reply(name, arguments));
        let input = chat_request(&app);
        app.call("POST", "/v1/runs", json!(input), 200).await;
        let run = phase(&app, input.id, RunPhase::terminal).await;
        assert!(run.turns[0].manager_result.is_none());
        assert!(run.turns[0].intent.is_none());
    }
    let mut reply = tool_reply(
        "memory",
        json!(MemoryProposal::Create {
            scope: MemoryScopeSelection::Shared,
            markdown: "Do not save duplicated calls.".into()
        }),
    );
    let mut body: Value = serde_json::from_str(&reply.body).unwrap();
    let call = body["choices"][0]["message"]["tool_calls"][0].clone();
    body["choices"][0]["message"]["tool_calls"]
        .as_array_mut()
        .unwrap()
        .push(call);
    reply.body = body.to_string();
    fixture.reply(reply);
    let input = chat_request(&app);
    app.call("POST", "/v1/runs", json!(input), 200).await;
    phase(&app, input.id, RunPhase::terminal).await;
    let db = rusqlite::Connection::open(local.directory().join("manager.sqlite3")).unwrap();
    assert_eq!(
        db.query_row::<i64, _, _>("SELECT count(*) FROM memory_publications", [], |r| r.get(0))
            .unwrap(),
        0
    );
    for body in db
        .prepare("SELECT view FROM run_turns")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
    {
        assert!(!body.unwrap().contains(KEY));
    }
    drop(db);
    app.shutdown().await;
}
