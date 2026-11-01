use super::*;

pub(super) async fn verify() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("owner");
    let (service, store, fixture) = paired(&root).await;
    let before = service.inspect();
    let workspace = before.workspace.as_ref().unwrap();
    let bot: BotId = fixture["bot"].as_str().unwrap().parse().unwrap();
    let session = workspace.sessions.iter().find(|s| s.bot_id == bot).unwrap();
    let configuration = workspace
        .state
        .inference_configurations
        .iter()
        .filter(|c| c.bot == bot)
        .max_by_key(|c| c.revision)
        .unwrap();
    let permission = workspace
        .state
        .permissions
        .iter()
        .find(|p| p.bot == bot)
        .unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let routine = RoutineId::new();
    let mut definition = RoutineDefinition {
        bot,
        name: "Daily briefing".into(),
        trigger: RoutineTrigger::Daily {
            hour: 9,
            minute: 0,
            timezone: "Europe/Stockholm".into(),
        },
        catchup_seconds: 300,
        wakeups: Some(WakeupGrant {
            horizon_seconds: 86400,
            minimum_spacing_seconds: 300,
            max_pending: 2,
            max_total: 10,
        }),
        template: RunTemplate {
            session: session.id,
            session_revision: session.revision,
            configuration_revision: configuration.revision,
            permission_revision: permission.revision,
            prompt: "Prepare the daily briefing.".into(),
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
            duration_seconds: 120,
            max_turns: 4,
            max_total_tokens: 200000,
            max_cost_microusd: 200000,
            max_output_tokens: 1024,
            max_operations: 16,
        },
    };
    let mut stages = std::collections::BTreeMap::new();
    stages.insert("before", serde_json::to_value(&before).unwrap());
    let budget = service
        .command(IntentCommand::Routine(RoutineCommand {
            id: RequestId::new(),
            action: RoutineAction::SetBudget {
                budget: RoutineBudget {
                    max_runs: 100,
                    max_tokens: 2_000_000,
                    max_cost_microusd: 1_000_000,
                },
            },
        }))
        .await
        .unwrap();
    stages.insert("budget", serde_json::to_value(&budget).unwrap());
    let create = RoutineCommand {
        id: RequestId::new(),
        action: RoutineAction::Create {
            id: routine,
            definition: Box::new(definition.clone()),
        },
    };
    let created = service
        .command(IntentCommand::Routine(create.clone()))
        .await
        .unwrap();
    assert_eq!(
        created
            .workspace
            .as_ref()
            .unwrap()
            .state
            .routine_state
            .routines
            .len(),
        1
    );
    stages.insert("created", serde_json::to_value(&created).unwrap());
    service.shutdown().await;
    let path = root.join("intents.json");
    let mut journal: Vec<Intent> = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for i in &mut journal {
        if matches!(&i.operation,OwnerOperation::RoutineCommand {input} if input.id==create.id) {
            i.status = "unknown".into();
        }
    }
    std::fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();
    let service = RemoteService::new(root.clone(), store.clone());
    let recovered = service.restore().await.unwrap();
    assert_eq!(
        recovered
            .workspace
            .as_ref()
            .unwrap()
            .state
            .routine_state
            .routines
            .len(),
        1
    );
    assert!(recovered.intents.iter().all(|i| i.status == "confirmed"));
    service.shutdown().await;
    let mut journal: Vec<Intent> = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for intent in &mut journal {
        if let OwnerOperation::RoutineCommand { input } = &mut intent.operation
            && input.id == create.id
        {
            if let RoutineAction::Create { definition, .. } = &mut input.action {
                definition.name = "Conflicting lost reply".into();
            }
            intent.status = "unknown".into();
        }
    }
    std::fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();
    let service = RemoteService::new(root.clone(), store.clone());
    let refused = service.restore().await.unwrap();
    assert!(
        refused
            .intents
            .iter()
            .any(|i| i.identity == create.id.to_string() && i.status == "refused")
    );
    assert_eq!(
        refused
            .workspace
            .as_ref()
            .unwrap()
            .state
            .routine_state
            .routines[0]
            .definition
            .name,
        "Daily briefing"
    );

    let paused = service
        .command(IntentCommand::RoutineText {
            bot,
            text: format!("/routine pause {routine} 1"),
        })
        .await
        .unwrap();
    assert_eq!(
        paused
            .workspace
            .as_ref()
            .unwrap()
            .state
            .routine_state
            .routines[0]
            .state,
        RoutineState::Paused
    );
    stages.insert("paused", serde_json::to_value(&paused).unwrap());
    let resumed = service
        .command(IntentCommand::RoutineText {
            bot,
            text: format!("/routine resume {routine} 2"),
        })
        .await
        .unwrap();
    stages.insert("resumed", serde_json::to_value(&resumed).unwrap());
    let preview = service
        .command(IntentCommand::RoutinePreview {
            trigger: definition.trigger.clone(),
            after: now,
        })
        .await
        .unwrap();
    assert_eq!(preview.routine_panel.preview.len(), 5);
    stages.insert("preview", serde_json::to_value(&preview).unwrap());
    service
        .command(IntentCommand::Routine(RoutineCommand {
            id: RequestId::new(),
            action: RoutineAction::Test {
                id: routine,
                expected_revision: 3,
            },
        }))
        .await
        .unwrap();
    let completed = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let snapshot = service.command(IntentCommand::Refresh).await.unwrap();
            if snapshot
                .workspace
                .as_ref()
                .unwrap()
                .state
                .routine_state
                .inbox
                .iter()
                .any(|e| {
                    e.routine == routine && matches!(e.status, OccurrenceStatus::Finished { .. })
                })
            {
                break snapshot;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    stages.insert("completed", serde_json::to_value(&completed).unwrap());
    let history = service
        .command(IntentCommand::RoutineHistory {
            id: routine,
            before: None,
        })
        .await
        .unwrap();
    assert_eq!(history.routine_panel.history.len(), 1);
    stages.insert("history", serde_json::to_value(&history).unwrap());
    definition.name = "Edited briefing".into();
    definition.template.prompt = "Prepare the revised daily briefing.".into();
    let edited = service
        .command(IntentCommand::Routine(RoutineCommand {
            id: RequestId::new(),
            action: RoutineAction::Replace {
                id: routine,
                expected_revision: 3,
                definition: Box::new(definition),
            },
        }))
        .await
        .unwrap();
    stages.insert("edited", serde_json::to_value(&edited).unwrap());
    let help = service
        .command(IntentCommand::RoutineText {
            bot,
            text: "/routine help".into(),
        })
        .await
        .unwrap();
    assert!(
        help.routine_panel
            .message
            .as_ref()
            .unwrap()
            .contains("Owner commands")
    );
    stages.insert("help", serde_json::to_value(&help).unwrap());
    let copy = BotId::new();
    let duplicated = service
        .command(IntentCommand::Routine(RoutineCommand {
            id: RequestId::new(),
            action: RoutineAction::DuplicateBot {
                bot,
                copy,
                name: "Inference bot copy".into(),
            },
        }))
        .await
        .unwrap();
    let copied = duplicated
        .workspace
        .as_ref()
        .unwrap()
        .state
        .routine_state
        .routines
        .iter()
        .find(|r| r.definition.bot == copy)
        .unwrap();
    assert_eq!(copied.state, RoutineState::Paused);
    stages.insert("duplicated", serde_json::to_value(&duplicated).unwrap());
    let deleted = service
        .command(IntentCommand::Routine(RoutineCommand {
            id: RequestId::new(),
            action: RoutineAction::Delete {
                id: routine,
                expected_revision: 4,
            },
        }))
        .await
        .unwrap();
    stages.insert("deleted", serde_json::to_value(&deleted).unwrap());
    let inbox_id = deleted
        .workspace
        .as_ref()
        .unwrap()
        .state
        .routine_state
        .inbox[0]
        .id;
    let ack = service
        .command(IntentCommand::Routine(RoutineCommand {
            id: RequestId::new(),
            action: RoutineAction::AcknowledgeInbox { id: inbox_id },
        }))
        .await
        .unwrap();
    assert!(
        ack.workspace
            .as_ref()
            .unwrap()
            .state
            .routine_state
            .inbox
            .is_empty()
    );
    stages.insert("acknowledged", serde_json::to_value(&ack).unwrap());
    use sha2::{Digest, Sha256};
    let publisher = PublisherId::new();
    let credential = format!("synthetic-p03-{}-credential", uuid::Uuid::new_v4());
    service
        .command(IntentCommand::Routine(RoutineCommand {
            id: RequestId::new(),
            action: RoutineAction::InstallPublisher {
                id: publisher,
                bot,
                name: "Build publisher".into(),
                topics: vec!["build.done".into()],
                credential_sha256: format!("{:x}", Sha256::digest(credential.as_bytes())),
            },
        }))
        .await
        .unwrap();
    let event_routine = RoutineId::new();
    let mut event_definition = created
        .workspace
        .as_ref()
        .unwrap()
        .state
        .routine_state
        .routines[0]
        .definition
        .clone();
    event_definition.name = "Build subscription".into();
    event_definition.trigger = RoutineTrigger::Event {
        publisher,
        topic: "build.done".into(),
        equals: std::collections::BTreeMap::from([(
            "branch".into(),
            EventScalar::Text("main".into()),
        )]),
    };
    service
        .command(IntentCommand::Routine(RoutineCommand {
            id: RequestId::new(),
            action: RoutineAction::Create {
                id: event_routine,
                definition: Box::new(event_definition),
            },
        }))
        .await
        .unwrap();
    let fixture_root = PathBuf::from(std::env::var_os("BOTSTRAL_C02_FIXTURE_ROOT").unwrap());
    let route = format!("/events/v1/publishers/{publisher}");
    macro_rules! publish {
        ($bytes:expr, $bearer:expr) => {
            publish_public(&fixture_root, &route, $bytes, $bearer).await
        };
    }
    let payload = PublisherEvent {
        id: "build-1".into(),
        timestamp: now,
        topic: "build.done".into(),
        fields: std::collections::BTreeMap::from([(
            "branch".into(),
            EventScalar::Text("main".into()),
        )]),
    };
    let bytes = serde_json::to_vec(&payload).unwrap();
    assert_eq!(publish!(&bytes, None).0, 401);
    assert_eq!(publish!(&bytes, Some(&"x".repeat(48))).0, 401);
    let first = publish!(&bytes, Some(&credential));
    assert_eq!(first.0, 202);
    assert_eq!(first.1["matched"], 1);
    assert_eq!(publish!(&bytes, Some(&credential)), first);
    let mut changed = payload.clone();
    changed
        .fields
        .insert("branch".into(), EventScalar::Text("other".into()));
    assert_eq!(
        publish!(&serde_json::to_vec(&changed).unwrap(), Some(&credential)).0,
        409
    );
    changed.id = "old".into();
    changed.timestamp = now - EVENT_REPLAY_SECONDS - 1;
    assert_eq!(
        publish!(&serde_json::to_vec(&changed).unwrap(), Some(&credential)).0,
        400
    );
    changed.id = "future".into();
    changed.timestamp = now + 3600;
    assert_eq!(
        publish!(&serde_json::to_vec(&changed).unwrap(), Some(&credential)).0,
        400
    );
    assert_eq!(
        publish!(&vec![b'x'; MAX_EVENT_BYTES + 1], Some(&credential)).0,
        413
    );
    service
        .command(IntentCommand::Routine(RoutineCommand {
            id: RequestId::new(),
            action: RoutineAction::SetPaused {
                id: event_routine,
                expected_revision: 1,
                paused: true,
            },
        }))
        .await
        .unwrap();
    changed.id = "paused".into();
    changed.timestamp = now;
    changed.fields = payload.fields.clone();
    let paused = publish!(&serde_json::to_vec(&changed).unwrap(), Some(&credential));
    assert_eq!(paused.0, 202);
    assert_eq!(paused.1["matched"], 0);
    service
        .command(IntentCommand::Routine(RoutineCommand {
            id: RequestId::new(),
            action: RoutineAction::SetPaused {
                id: event_routine,
                expected_revision: 2,
                paused: false,
            },
        }))
        .await
        .unwrap();
    assert_eq!(
        publish!(&serde_json::to_vec(&changed).unwrap(), Some(&credential)),
        paused
    );
    let mut limited = false;
    for _ in 0..64 {
        if publish!(&bytes, Some(&credential)).0 == 429 {
            limited = true;
            break;
        }
    }
    assert!(limited);
    service
        .command(IntentCommand::Routine(RoutineCommand {
            id: RequestId::new(),
            action: RoutineAction::RevokePublisher {
                id: publisher,
                expected_revision: 1,
            },
        }))
        .await
        .unwrap();
    assert_eq!(publish!(&bytes, Some(&credential)).0, 401);
    let received = service.command(IntentCommand::Refresh).await.unwrap();
    let state = &received.workspace.as_ref().unwrap().state.routine_state;
    assert_eq!(
        state
            .occurrences
            .iter()
            .filter(|o| o.trigger_key == format!("event:{publisher}:build-1"))
            .count(),
        1
    );
    assert!(state.publishers[0].revoked);
    assert_eq!(
        state.publishers[0].last_error.as_deref(),
        Some("RateLimited")
    );
    service.shutdown().await;
    let mut client = OwnerClient::open_with(&root, store.clone()).unwrap();
    let mut owner = client.connect().await.unwrap();
    let completed_run = completed.workspace.as_ref().unwrap().runs[0].summary.id;
    let OwnerReply::Run(full) = owner
        .call(OwnerOperation::Run { id: completed_run })
        .await
        .unwrap()
    else {
        panic!("full run reply")
    };
    let mut cached = serde_json::to_value(client.installed_workspace().unwrap().unwrap()).unwrap();
    cached["runs"] = serde_json::to_value(vec![*full]).unwrap();
    cached["state"]
        .as_object_mut()
        .unwrap()
        .remove("routine_state");
    std::fs::write(
        root.join("workspace.json"),
        serde_json::to_vec(&cached).unwrap(),
    )
    .unwrap();
    let migrated = client.installed_workspace().unwrap().unwrap();
    assert_eq!(migrated.runs[0].summary.id, completed_run);
    assert!(migrated.runs[0].turns[0].text.is_some());
    drop(owner);
    drop(client);
    let mut journal: Vec<Intent> = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let unsent = RoutineCommand {
        id: RequestId::new(),
        action: RoutineAction::DeleteBot { bot },
    };
    journal.push(Intent {
        id: uuid::Uuid::new_v4(),
        operation: OwnerOperation::RoutineCommand { input: unsent },
        status: "unknown".into(),
    });
    std::fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();
    let service = RemoteService::new(root, store);
    let replayed = service.restore().await.unwrap();
    assert!(
        replayed
            .workspace
            .as_ref()
            .unwrap()
            .bots
            .iter()
            .all(|b| b.id != bot)
    );
    assert!(replayed.intents.iter().all(|i| i.status
        == if i.identity == create.id.to_string() {
            "refused"
        } else {
            "confirmed"
        }));
    stages.insert("deleted_bot", serde_json::to_value(&replayed).unwrap());
    if let Some(output) = std::env::var_os("BOTSTRAL_P03_VIEWS") {
        std::fs::write(output, serde_json::to_vec_pretty(&stages).unwrap()).unwrap();
    }
    service.sign_out().await.unwrap();
    service.shutdown().await;
}
