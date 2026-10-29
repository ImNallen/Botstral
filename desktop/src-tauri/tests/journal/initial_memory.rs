use super::*;
use botstral_desktop::remote::RemoteSnapshot;

const MARKDOWN: &str = "Use concise answers.\nKeep the preference across new runs.";

fn memory(snapshot: &RemoteSnapshot, bot: BotId) -> Document {
    snapshot
        .workspace
        .as_ref()
        .unwrap()
        .state
        .documents
        .iter()
        .find(|document| document.bot == bot && document.kind == DocumentKind::Memory)
        .unwrap()
        .clone()
}

async fn wait_file(path: &Path) {
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        while !path.exists() {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
}

async fn send(
    service: &RemoteService,
    bot: BotId,
    session: ComputerSessionId,
    text: &str,
) -> RunId {
    service
        .command(IntentCommand::Send(SendInput {
            bot,
            session,
            text: text.into(),
            account: ProcessAccount {
                uid: 1000,
                gid: 1000,
            },
            runtime: None,
            process: true,
            seconds: 120,
            turns: 4,
            tokens: 200_000,
            cost_microusd: 200_000,
            output_tokens: 1024,
        }))
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            let snapshot = service.command(IntentCommand::Refresh).await.unwrap();
            if let Some(run) = snapshot
                .workspace
                .as_ref()
                .unwrap()
                .runs
                .iter()
                .find(|run| {
                    run.request.prompt == text && matches!(run.summary.phase, RunPhase::Completed)
                })
            {
                break run.summary.id;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap()
}

async fn read_run(root: &Path, store: Arc<MemorySecrets>, id: RunId) -> RunView {
    let mut client = OwnerClient::open_with(root, store).unwrap();
    let mut owner = client.connect().await.unwrap();
    let OwnerReply::Run(run) = owner.call(OwnerOperation::Run { id }).await.unwrap() else {
        panic!("Expected run")
    };
    *run
}

pub(super) async fn verify() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("owner");
    let fixture_root = PathBuf::from(std::env::var_os("BOTSTRAL_C02_FIXTURE_ROOT").unwrap());
    let (service, store, fixture) = paired(&root).await;
    let bot: BotId = fixture["bot"].as_str().unwrap().parse().unwrap();
    let source: MessageId = fixture["memory_source"].as_str().unwrap().parse().unwrap();
    let second: MessageId = fixture["second_source"].as_str().unwrap().parse().unwrap();
    let foreign: MessageId = fixture["foreign_source"].as_str().unwrap().parse().unwrap();
    let mut stages = std::collections::BTreeMap::new();
    let absent = service.command(IntentCommand::Refresh).await.unwrap();
    stages.insert("absent", serde_json::to_value(&absent).unwrap());
    let projection = serde_json::to_value(&absent).unwrap();
    assert_eq!(projection["workspace"]["documents"], serde_json::json!([]));
    let previews = projection["workspace"]["memory_sources"]
        .as_array()
        .unwrap();
    assert_eq!(
        previews
            .iter()
            .find(|preview| preview["id"] == second.to_string())
            .unwrap()["text"]
            .as_str()
            .unwrap()
            .chars()
            .count(),
        240
    );
    let mut crowded = absent.clone();
    let state = &mut crowded.workspace.as_mut().unwrap().state;
    let foreign_conversation = state
        .messages
        .iter()
        .find(|message| message.id == foreign)
        .unwrap()
        .conversation;
    while state.messages.len() < MAX_RECORDS {
        state.messages.push(ConversationMessage {
            id: MessageId::new(),
            conversation: foreign_conversation,
            text: "A later message for another bot".into(),
        });
    }
    let crowded_projection = serde_json::to_value(&crowded).unwrap();
    let crowded_sources = crowded_projection["workspace"]["memory_sources"]
        .as_array()
        .unwrap();
    assert!(
        crowded_sources
            .iter()
            .any(|preview| preview["id"] == source.to_string()),
        "Later messages from another bot must not hide an existing Memory source"
    );
    assert_eq!(crowded_sources.len(), MAX_RECORDS);
    for selected in [MessageId::new(), foreign] {
        assert_eq!(
            service
                .command(IntentCommand::CreateMemory {
                    bot,
                    source_message: selected,
                    markdown: MARKDOWN.into()
                })
                .await
                .err()
                .as_deref(),
            Some("memory_source_invalid")
        );
    }
    assert_eq!(
        service
            .command(IntentCommand::CreateMemory {
                bot,
                source_message: source,
                markdown: "é".repeat(2049)
            })
            .await
            .err()
            .as_deref(),
        Some("invalid_input")
    );
    assert!(service.inspect().intents.is_empty());
    let old_prompt = "A greeting before any saved preference.";
    let old_id = send(
        &service,
        bot,
        fixture["session"].as_str().unwrap().parse().unwrap(),
        old_prompt,
    )
    .await;
    service.shutdown().await;
    let old = read_run(&root, store.clone(), old_id).await;
    assert_eq!(
        old.turns[0].messages,
        vec![RunMessage::User {
            content: old_prompt.into()
        }]
    );

    let service = RemoteService::new(root.clone(), store.clone());
    service.restore().await.unwrap();
    let created = service
        .command(IntentCommand::CreateMemory {
            bot,
            source_message: source,
            markdown: MARKDOWN.into(),
        })
        .await
        .unwrap();
    let expected = WriteDocument {
        bot,
        kind: DocumentKind::Memory,
        expected_revision: 0,
        markdown: MARKDOWN.into(),
        sources: vec![SourceReference::Message { id: source }],
    };
    let saved = memory(&created, bot);
    assert_eq!(saved.revision, 1);
    assert_eq!(saved.markdown, MARKDOWN);
    assert_eq!(saved.sources, expected.sources);
    assert!(
        created
            .intents
            .iter()
            .all(|intent| intent.status == "confirmed")
    );
    stages.insert("created", serde_json::to_value(&created).unwrap());
    assert_eq!(
        service
            .command(IntentCommand::CreateMemory {
                bot,
                source_message: source,
                markdown: "Competing creation".into()
            })
            .await
            .err()
            .as_deref(),
        Some("memory_conflict")
    );
    service.shutdown().await;
    let contender_root = directory.path().join("contender");
    let (contender_service, contender_store, _) = paired(&contender_root).await;
    contender_service.shutdown().await;
    let mut contender_client = OwnerClient::open_with(&contender_root, contender_store).unwrap();
    let mut contender = contender_client.connect().await.unwrap();
    let mut client = OwnerClient::open_with(&root, store.clone()).unwrap();
    let mut owner = client.connect().await.unwrap();
    let (first_reply, second_reply) = tokio::join!(
        owner.call(OwnerOperation::WriteDocument {
            input: expected.clone()
        }),
        contender.call(OwnerOperation::WriteDocument {
            input: expected.clone()
        })
    );
    for reply in [first_reply, second_reply] {
        match reply {
            Ok(OwnerReply::Document(identical)) => assert_eq!(identical, saved),
            Err(ClientError::Refused(OwnerError::Busy)) => {}
            other => panic!("Unexpected overlapping write result: {other:?}"),
        }
    }
    for session in [&mut owner, &mut contender] {
        let reply = session
            .call(OwnerOperation::WriteDocument {
                input: expected.clone(),
            })
            .await
            .unwrap();
        assert!(matches!(reply, OwnerReply::Document(document) if document == saved));
    }
    let mut different_text = expected.clone();
    different_text.markdown = "Competing creation".into();
    let mut different_source = expected.clone();
    different_source.sources = vec![SourceReference::Message { id: second }];
    for input in [different_text.clone(), different_source.clone()] {
        let (matching, conflicting) = tokio::join!(
            owner.call(OwnerOperation::WriteDocument {
                input: expected.clone()
            }),
            contender.call(OwnerOperation::WriteDocument {
                input: input.clone()
            })
        );
        match matching {
            Ok(OwnerReply::Document(document)) => assert_eq!(document, saved),
            Err(ClientError::Refused(OwnerError::Busy)) => {}
            other => panic!("Unexpected matching write result: {other:?}"),
        }
        assert!(matches!(
            conflicting,
            Err(ClientError::Refused(
                OwnerError::Conflict | OwnerError::Busy
            ))
        ));
        assert!(matches!(
            contender
                .call(OwnerOperation::WriteDocument { input })
                .await,
            Err(ClientError::Refused(OwnerError::Conflict))
        ));
    }
    drop(contender);
    drop(contender_client);
    contender_service.sign_out().await.unwrap();
    contender_service.shutdown().await;
    drop(owner);
    drop(client);

    let mut wrong_revision = expected.clone();
    wrong_revision.expected_revision = 1;
    let mut wrong_bot = expected.clone();
    wrong_bot.bot = BotId::new();
    let mut extra_source = expected.clone();
    extra_source
        .sources
        .push(SourceReference::Message { id: second });
    let intents: Vec<_> = [
        expected,
        different_text,
        different_source,
        wrong_revision,
        wrong_bot,
        extra_source,
    ]
    .into_iter()
    .map(|input| Intent {
        id: uuid::Uuid::new_v4(),
        operation: OwnerOperation::WriteDocument { input },
        status: "unknown".into(),
    })
    .collect();
    let journal = root.join("intents.json");
    std::fs::write(&journal, serde_json::to_vec(&intents).unwrap()).unwrap();
    std::fs::set_permissions(&journal, std::fs::Permissions::from_mode(0o600)).unwrap();
    let service = RemoteService::new(root.clone(), store.clone());
    let recovered = service.restore().await.unwrap();
    assert_eq!(memory(&recovered, bot), saved);
    assert_eq!(recovered.intents[0].status, "confirmed");
    assert!(
        recovered.intents[1..]
            .iter()
            .all(|intent| intent.status == "unknown")
    );
    stages.insert("recovered", serde_json::to_value(&recovered).unwrap());
    service.shutdown().await;
    std::fs::write(fixture_root.join("restart-manager"), b"").unwrap();
    wait_file(&fixture_root.join("manager-restarted")).await;
    let session: ComputerSession =
        serde_json::from_slice(&std::fs::read(fixture_root.join("restart-session.json")).unwrap())
            .unwrap();
    let service = RemoteService::new(root.clone(), store.clone());
    let restarted = service.restore().await.unwrap();
    assert_eq!(memory(&restarted, bot), saved);
    assert_eq!(restarted.intents[0].status, "confirmed");
    assert!(
        restarted.intents[1..]
            .iter()
            .all(|intent| intent.status == "unknown")
    );
    stages.insert("restarted", serde_json::to_value(&restarted).unwrap());
    for intent in &intents[1..] {
        service
            .command(IntentCommand::AcknowledgeUncertainty(intent.id))
            .await
            .unwrap();
    }
    stages.insert(
        "settled",
        serde_json::to_value(service.command(IntentCommand::Refresh).await.unwrap()).unwrap(),
    );
    let new_prompt = "A new greeting after the manager restart.";
    let new_id = send(&service, bot, session.id, new_prompt).await;
    service.shutdown().await;
    let new = read_run(&root, store.clone(), new_id).await;
    assert_eq!(old, read_run(&root, store.clone(), old_id).await);
    let RunMessage::User { content } = &new.turns[0].messages[0] else {
        panic!("Expected saved Memory")
    };
    assert_eq!(
        serde_json::from_str::<Document>(content.split_once('\n').unwrap().1).unwrap(),
        saved
    );
    let captures = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if let Ok(bytes) = std::fs::read(fixture_root.join("provider-requests.json"))
                && let Ok(captures) = serde_json::from_slice::<Vec<serde_json::Value>>(&bytes)
                && captures
                    .iter()
                    .any(|request| request["messages"].to_string().contains(new_prompt))
            {
                break captures;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    let before_request = captures
        .iter()
        .find(|request| request["messages"].to_string().contains(old_prompt))
        .unwrap();
    let after_request = captures
        .iter()
        .find(|request| request["messages"].to_string().contains(new_prompt))
        .unwrap();
    assert_eq!(
        before_request["messages"],
        serde_json::json!([{"role":"user","content":old_prompt}])
    );
    assert_eq!(
        after_request["messages"][0],
        serde_json::json!({"role":"user","content":content})
    );
    assert_eq!(
        after_request["messages"][1],
        serde_json::json!({"role":"user","content":new_prompt})
    );
    if let Some(output) = std::env::var_os("BOTSTRAL_INITIAL_MEMORY_VIEWS") {
        let output = PathBuf::from(output);
        std::fs::write(&output, serde_json::to_vec_pretty(&stages).unwrap()).unwrap();
        std::fs::write(output.with_file_name("provider-proof.json"), serde_json::to_vec_pretty(&serde_json::json!({"saved":saved,"old_run":old,"new_run":new,"provider_before":before_request,"provider_after":after_request,"historical_run_unchanged":true,"restart":serde_json::from_slice::<serde_json::Value>(&std::fs::read(fixture_root.join("restart-proof.json")).unwrap()).unwrap()})).unwrap()).unwrap();
    }
    let service = RemoteService::new(root, store);
    service.restore().await.unwrap();
    service.sign_out().await.unwrap();
    service.shutdown().await;
    println!(
        "PASS initial native creation, missing/foreign sources, byte bound, conflicting and identical manager writes, revision-one exact journal recovery without replay, manager and native restart, captured new-run provider Memory and unchanged historical run"
    );
}
