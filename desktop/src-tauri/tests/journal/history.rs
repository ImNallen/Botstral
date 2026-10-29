use super::*;

pub(super) async fn verify() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("owner");
    let (service, store, fixture) = paired(&root).await;
    let workspace = service.inspect().workspace.unwrap();
    let bot: BotId = fixture["bot"].as_str().unwrap().parse().unwrap();
    let configuration = workspace
        .state
        .inference_configurations
        .iter()
        .filter(|c| c.bot == bot)
        .max_by_key(|c| c.revision)
        .unwrap();
    let input = SubmitRun {
        id: RunId::new(),
        bot,
        session: Some(fixture["session"].as_str().unwrap().parse().unwrap()),
        configuration_revision: configuration.revision,
        prompt: "Recover this exact submitted run after its history is trimmed.".into(),
        images: vec![],
        execution_policy: RunExecutionPolicy {
            computer: None,
            connectors: None,
            vibe: None,
            vision: None,
            process: Some(ProcessToolPolicy {
                profile: ProcessProfile::LinuxSystemdV1,
                account: ProcessAccount {
                    uid: 1000,
                    gid: 1000,
                },
                environment: Default::default(),
                limits: ProcessLimits {
                    runtime_seconds: 120,
                    output_bytes: 1024,
                    processes: 8,
                    memory_bytes: 67108864,
                },
            }),
        },
        limits: RunLimits {
            deadline: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64
                + 120,
            max_turns: 1,
            max_total_tokens: 200_000,
            max_cost_microusd: 200_000,
            max_output_tokens: 64,
            max_operations: 1,
            max_concurrent_operations: 1,
        },
    };
    service.shutdown().await;
    let mut client = OwnerClient::open_with(&root, store.clone()).unwrap();
    let mut session = client.connect().await.unwrap();
    for index in 0..10 {
        let mut request = input.clone();
        if index > 0 {
            request.id = RunId::new();
        }
        let id = request.id;
        session
            .call(OwnerOperation::SubmitRun {
                input: Box::new(request),
            })
            .await
            .unwrap();
        session
            .call(OwnerOperation::CancelRun { id })
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let OwnerReply::Run(run) = session.call(OwnerOperation::Run { id }).await.unwrap()
                else {
                    panic!("Expected authoritative run");
                };
                if matches!(
                    run.summary.phase,
                    RunPhase::Completed | RunPhase::Cancelled | RunPhase::Stopped { .. }
                ) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
    }
    let oldest_task = TaskId::new();
    for index in 0..34 {
        let id = if index == 0 {
            oldest_task
        } else {
            TaskId::new()
        };
        session
            .call(OwnerOperation::SubmitTask {
                input: SubmitTask {
                    id,
                    session_id: input.session.unwrap(),
                    conversation_id: None,
                    settings: TaskSettings {
                        model: "native recovery fixture".into(),
                        max_turns: 1,
                        max_tokens: 64,
                        deadline: input.limits.deadline,
                        max_cost_micros: 0,
                    },
                    work: WorkSpec::Process {
                        spec: Box::new(ProcessSpec {
                            profile: ProcessProfile::LinuxSystemdV1,
                            account: ProcessAccount {
                                uid: 1000,
                                gid: 1000,
                            },
                            executable: "/usr/bin/true".into(),
                            args: vec![],
                            environment: Default::default(),
                            cwd: "/tmp".into(),
                            deadline: input.limits.deadline,
                            limits: ProcessLimits {
                                runtime_seconds: 120,
                                output_bytes: 1024,
                                processes: 8,
                                memory_bytes: 67108864,
                            },
                        }),
                    },
                },
            })
            .await
            .unwrap();
        if index == 0 {
            session
                .call(OwnerOperation::RevokeApproval { id })
                .await
                .unwrap();
        }
        session
            .call(OwnerOperation::CancelTask { id })
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let OwnerReply::Task(task) =
                    session.call(OwnerOperation::Task { id }).await.unwrap()
                else {
                    panic!("Expected authoritative task");
                };
                if task.state.terminal() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
    }
    let image = client.recover_workspace(&mut session).await.unwrap();
    assert!(!image.runs.iter().any(|r| r.summary.id == input.id));
    assert!(!image.state.tasks.iter().any(|task| task.id == oldest_task));
    drop(session);
    drop(client);
    let mut intents = vec![
        Intent {
            id: uuid::Uuid::new_v4(),
            status: "unknown".into(),
            operation: OwnerOperation::SubmitRun {
                input: Box::new(input.clone()),
            },
        },
        Intent {
            id: uuid::Uuid::new_v4(),
            status: "unknown".into(),
            operation: OwnerOperation::CancelRun { id: input.id },
        },
        Intent {
            id: uuid::Uuid::new_v4(),
            status: "unknown".into(),
            operation: OwnerOperation::CancelTask { id: oldest_task },
        },
        Intent {
            id: uuid::Uuid::new_v4(),
            status: "unknown".into(),
            operation: OwnerOperation::RevokeApproval { id: oldest_task },
        },
    ];
    let journal = root.join("intents.json");
    std::fs::write(&journal, serde_json::to_vec(&intents).unwrap()).unwrap();
    let reopened = RemoteService::new(root.clone(), store.clone());
    let restored = reopened.restore().await.unwrap();
    assert!(
        restored.intents.iter().all(|i| i.status == "confirmed"),
        "Old committed runs and tasks must resolve journal intents outside the history window: {:?}",
        restored
            .intents
            .iter()
            .map(|i| &i.status)
            .collect::<Vec<_>>()
    );
    assert!(
        !restored
            .workspace
            .unwrap()
            .runs
            .iter()
            .any(|r| r.summary.id == input.id)
    );
    reopened
        .command(IntentCommand::Routine(RoutineCommand {
            id: RequestId::new(),
            action: RoutineAction::SetBudget {
                budget: RoutineBudget {
                    max_runs: 100,
                    max_tokens: 1_000_000,
                    max_cost_microusd: 1_000_000,
                },
            },
        }))
        .await
        .unwrap();
    reopened.shutdown().await;

    let mut conflicting = input;
    conflicting.prompt = "This request was never committed.".into();
    intents.push(Intent {
        id: uuid::Uuid::new_v4(),
        status: "unknown".into(),
        operation: OwnerOperation::SubmitRun {
            input: Box::new(conflicting),
        },
    });
    intents.push(Intent {
        id: uuid::Uuid::new_v4(),
        status: "unknown".into(),
        operation: OwnerOperation::CancelRun { id: RunId::new() },
    });
    std::fs::write(&journal, serde_json::to_vec(&intents).unwrap()).unwrap();
    let reopened = RemoteService::new(root, store);
    let restored = reopened.restore().await.unwrap();
    assert_eq!(
        restored
            .intents
            .iter()
            .filter(|i| i.status == "unknown")
            .count(),
        2
    );
    let mutation = reopened
        .command(IntentCommand::Routine(RoutineCommand {
            id: RequestId::new(),
            action: RoutineAction::SetBudget {
                budget: RoutineBudget {
                    max_runs: 100,
                    max_tokens: 1_000_000,
                    max_cost_microusd: 1_000_000,
                },
            },
        }))
        .await;
    assert!(matches!(mutation, Err(code) if code == "unresolved_intent"));
    reopened.sign_out().await.unwrap();
}
