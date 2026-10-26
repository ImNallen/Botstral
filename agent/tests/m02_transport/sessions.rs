use super::*;

#[tokio::test]
async fn confirmed_takeover_two_sessions_cancellation_and_results() {
    let mut f = Fixture::new().await;
    let a = f.durable_session().await;
    let b = f.create_session(a.worker_id, "Second bot").await;
    let mut daemon = f.start_agent(None);
    let ready = f.wait_phase("ready", 1).await;
    let generation = Generation::try_from(ready["generation"].as_u64().unwrap()).unwrap();
    let a = f.confirmed(a.id).await;
    let b = f.confirmed(b.id).await;
    let task_a = f.submit(&a, WorkSpec::TestInput { steps: 5 }).await;
    f.task_state(&task_a, "awaiting_approval").await;
    let owner_a = f.takeover(&a, InputOwner::Owner).await;
    assert_eq!(f.confirmed(b.id).await, b);
    f.approve(&task_a).await;
    let task_b = f.submit(&b, WorkSpec::TestInput { steps: 3 }).await;
    f.approve(&task_b).await;
    let completed = f.task_state(&task_b, "completed").await;
    assert_eq!(completed["progress"], 3);
    assert_eq!(f.effects(), (1, 1));
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
    f.receipt(a.worker_id, owner_input, |v| {
        v["acknowledgment"]["type"] == "applied"
    })
    .await;
    f.owner(
        "POST",
        &format!("/v1/tasks/{}/cancel", task_a["id"].as_str().unwrap()),
        json!({}),
    )
    .await;
    f.task_state(&task_a, "cancelled").await;
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
    // Cancellation is admitted before a fresh claim after input control changes.
    f.owner(
        "POST",
        &format!("/v1/tasks/{}/cancel", task_a["id"].as_str().unwrap()),
        json!({}),
    )
    .await;
    let stopped = f.task_state(&task_a, "cancelled").await;
    assert_eq!(stopped["outcome"]["evidence"], "no_process_group");
    let original = f
        .owner("POST", "/v1/tasks", task_b["request"].clone())
        .await;
    assert_eq!(original.1, completed);
    let legacy = botstral_link::Command {
        id: CommandId::new(),
        task_id: TaskId::new(),
        fence: bot_a.fence(generation),
        action: CommandAction::Start {
            work: WorkSpec::TestInput { steps: 1 },
        },
    };
    assert_eq!(
        f.owner(
            "POST",
            "/v1/worker-commands",
            serde_json::to_value(&legacy).unwrap()
        )
        .await
        .1["error"],
        "unsupported_execution"
    );
    daemon.stop().await;
    let mut daemon = f.start_agent(None);
    f.wait_phase("ready", generation.get() + 1).await;
    assert_eq!(
        f.owner(
            "GET",
            &format!("/v1/tasks/{}", task_b["id"].as_str().unwrap()),
            json!({})
        )
        .await
        .1,
        completed
    );
    assert_eq!(
        f.owner(
            "POST",
            "/v1/worker-commands",
            serde_json::to_value(legacy).unwrap()
        )
        .await
        .1["error"],
        "stale_generation"
    );
    daemon.stop().await;
    f.manager.stop().await;
}
