use super::*;

#[tokio::test]
async fn confirmed_takeover_two_sessions_cancellation_and_results() {
    let mut f = Fixture::new().await;
    let grant = f.invitation(&f.worker, 600).await;
    let enrolled = worker_http(
        &f.worker,
        "POST",
        "/worker/v1/enroll",
        json!({"invitation":grant["invitation"]}),
    )
    .await;
    assert_eq!(enrolled.0, 201);
    let worker: WorkerId = serde_json::from_value(enrolled.1["worker_id"].clone()).unwrap();
    let a = f.create_session(worker, "A").await;
    let b = f.create_session(worker, "B").await;
    let state = f.root.path().join("test-agent");
    fs::create_dir(&state).unwrap();
    fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
    private(
        &state.join("config.json"),
        &serde_json::to_vec(&f.worker).unwrap(),
    );
    let mut daemon = launch(
        Path::new(env!("CARGO_BIN_EXE_botstral-test-agent")),
        &["run", "--state-dir", state.to_str().unwrap()],
        f.root.path(),
        "test-agent",
    );
    let ready = f.wait_phase("ready", 1).await;
    let generation = Generation::try_from(ready["generation"].as_u64().unwrap()).unwrap();
    let a = f.confirmed(a.id).await;
    let b = f.confirmed(b.id).await;
    let start_a = f
        .command(
            &a,
            generation,
            TaskId::new(),
            CommandAction::Start {
                work: WorkSpec::TestInput { steps: 1000 },
            },
        )
        .await;
    let start_b = f
        .command(
            &b,
            generation,
            TaskId::new(),
            CommandAction::Start {
                work: WorkSpec::TestInput { steps: 1000 },
            },
        )
        .await;
    f.receipt(worker, start_a.id, |v| {
        v["last_event"]["applied"].as_u64().unwrap_or(0) >= 3
    })
    .await;
    f.receipt(worker, start_b.id, |v| {
        v["last_event"]["applied"].as_u64().unwrap_or(0) >= 3
    })
    .await;
    let owner_a = f.takeover(&a, InputOwner::Owner).await;
    let b_after = f.confirmed(b.id).await;
    assert_eq!(b_after, b);
    let a_status = f
        .command(&owner_a, generation, start_a.task_id, CommandAction::Status)
        .await;
    let stopped_a = f
        .receipt(worker, a_status.id, |v| {
            v["last_event"]["status"] == "suspended"
        })
        .await;
    let before_b = f.receipt(worker, start_b.id, |_| true).await["last_event"]["applied"]
        .as_u64()
        .unwrap();
    f.receipt(worker, start_b.id, |v| {
        v["last_event"]["applied"].as_u64().unwrap_or(0) > before_b + 2
    })
    .await;
    let a_status = f
        .command(&owner_a, generation, start_a.task_id, CommandAction::Status)
        .await;
    let still_stopped = f
        .receipt(worker, a_status.id, |v| {
            v["last_event"]["status"] == "suspended"
        })
        .await;
    assert_eq!(
        stopped_a["last_event"]["applied"],
        still_stopped["last_event"]["applied"]
    );
    let mut stale = a.fence(generation);
    stale.owner = InputOwner::Owner;
    let refusal = f
        .owner(
            "POST",
            "/v1/worker-input",
            json!({"id":CommandId::new(),"fence":stale,"value":1}),
        )
        .await;
    assert_eq!(refusal, (409, json!({"error":"stale_revision"})));
    let owner_input = CommandId::new();
    assert_eq!(
        f.owner(
            "POST",
            "/v1/worker-input",
            json!({"id":owner_input,"fence":owner_a.fence(generation),"value":1})
        )
        .await
        .0,
        200
    );
    f.receipt(worker, owner_input, |v| {
        v["acknowledgment"]["type"] == "applied"
    })
    .await;
    let bot_a = f.takeover(&owner_a, InputOwner::Bot).await;
    assert_eq!(
        f.owner(
            "POST",
            "/v1/worker-input",
            json!({"id":CommandId::new(),"fence":owner_a.fence(generation),"value":1})
        )
        .await
        .1["error"],
        "stale_revision"
    );
    let cancel = f
        .command(&b, generation, start_b.task_id, CommandAction::Cancel)
        .await;
    let cancelled = f
        .receipt(worker, cancel.id, |v| {
            v["last_event"]["type"] == "cancelled"
        })
        .await;
    assert!(cancelled["last_event"]["applied"].as_u64().unwrap() < 1000);
    let result = f
        .command(&b, generation, start_b.task_id, CommandAction::Result)
        .await;
    let result = f
        .receipt(worker, result.id, |v| {
            v["last_event"]["status"] == "cancelled"
        })
        .await;
    assert_eq!(
        result["last_event"]["applied"],
        cancelled["last_event"]["applied"]
    );
    let small = f
        .command(
            &bot_a,
            generation,
            TaskId::new(),
            CommandAction::Start {
                work: WorkSpec::TestInput { steps: 3 },
            },
        )
        .await;
    f.receipt(worker, small.id, |v| {
        v["last_event"] == json!({"type":"completed","applied":3})
    })
    .await;
    let result = f
        .command(&bot_a, generation, small.task_id, CommandAction::Result)
        .await;
    f.receipt(worker, result.id, |v| {
        v["last_event"] == json!({"type":"completed","applied":3})
    })
    .await;
    let same = f
        .owner(
            "POST",
            "/v1/worker-commands",
            serde_json::to_value(&small).unwrap(),
        )
        .await;
    assert_eq!(same.0, 200);
    let mut changed = small.clone();
    changed.action = CommandAction::Cancel;
    assert_eq!(
        f.owner(
            "POST",
            "/v1/worker-commands",
            serde_json::to_value(changed).unwrap()
        )
        .await
        .1["error"],
        "conflict"
    );
    daemon.stop().await;
    f.wait_phase("disconnected", generation.get()).await;
    let mut daemon = launch(
        Path::new(env!("CARGO_BIN_EXE_botstral-test-agent")),
        &["run", "--state-dir", state.to_str().unwrap()],
        f.root.path(),
        "test-agent-reconnect",
    );
    let ready = f.wait_phase("ready", generation.get() + 1).await;
    let new_generation = Generation::try_from(ready["generation"].as_u64().unwrap()).unwrap();
    let new_a = f.confirmed(a.id).await;
    assert!(new_a.revision > bot_a.revision);
    assert_eq!(
        f.owner(
            "POST",
            "/v1/worker-commands",
            serde_json::to_value(small.clone()).unwrap()
        )
        .await
        .1["error"],
        "stale_generation"
    );
    let missing = f
        .command(&new_a, new_generation, small.task_id, CommandAction::Result)
        .await;
    f.receipt(worker, missing.id, |v| {
        v["acknowledgment"] == json!({"type":"rejected","code":"unknown_task"})
    })
    .await;
    f.manager.stop().await;
    daemon.stop().await;
}
