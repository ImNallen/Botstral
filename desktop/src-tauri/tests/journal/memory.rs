use super::*;

pub(super) async fn verify() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("owner");
    let (service, store, fixture) = paired(&root).await;
    let bot: BotId = fixture["bot"].as_str().unwrap().parse().unwrap();
    let mut stages = std::collections::BTreeMap::new();
    assert_eq!(
        service
            .command(IntentCommand::WriteMemory {
                bot,
                expected_revision: 0,
                markdown: "No initial authoring".into()
            })
            .await
            .err()
            .as_deref(),
        Some("memory_absent")
    );
    stages.insert("absent", serde_json::to_value(service.inspect()).unwrap());
    service.shutdown().await;
    let mut client = OwnerClient::open_with(&root, store.clone()).unwrap();
    let mut owner = client.connect().await.unwrap();
    let sources = vec![
        SourceReference::External {
            reference: "https://example.test/owner-preference".into(),
        },
        SourceReference::External {
            reference: "https://example.test/trusted/notes.md".into(),
        },
    ];
    for kind in [DocumentKind::Instructions, DocumentKind::Memory] {
        let reply = owner
            .call(OwnerOperation::WriteDocument {
                input: WriteDocument {
                    bot,
                    kind,
                    expected_revision: 0,
                    sources: sources.clone(),
                    markdown: "Original saved preference.".into(),
                },
            })
            .await
            .unwrap();
        assert!(matches!(reply, OwnerReply::Document(_)));
    }
    drop(owner);
    drop(client);
    let service = RemoteService::new(root.clone(), store.clone());
    let before = service.restore().await.unwrap();
    stages.insert("before", serde_json::to_value(&before).unwrap());
    let projection = serde_json::to_value(&before).unwrap();
    assert_eq!(
        projection["workspace"]["documents"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(projection["workspace"]["documents"][0]["kind"], "memory");
    let edited = service
        .command(IntentCommand::WriteMemory {
            bot,
            expected_revision: 1,
            markdown: "Corrected saved preference.\nUse concise answers.".into(),
        })
        .await
        .unwrap();
    let memory = |snapshot: &botstral_desktop::remote::RemoteSnapshot| {
        snapshot
            .workspace
            .as_ref()
            .unwrap()
            .state
            .documents
            .iter()
            .find(|d| d.bot == bot && d.kind == DocumentKind::Memory)
            .unwrap()
            .clone()
    };
    assert_eq!(memory(&edited).revision, 2);
    assert_eq!(memory(&edited).sources, sources);
    assert!(edited.intents.iter().all(|i| i.status == "confirmed"));
    stages.insert("edited", serde_json::to_value(&edited).unwrap());
    assert_eq!(
        service
            .command(IntentCommand::WriteMemory {
                bot,
                expected_revision: 2,
                markdown: "é".repeat(2049)
            })
            .await
            .err()
            .as_deref(),
        Some("invalid_input")
    );

    assert_eq!(
        service
            .command(IntentCommand::WriteMemory {
                bot,
                expected_revision: 1,
                markdown: "Stale draft".into()
            })
            .await
            .err()
            .as_deref(),
        Some("memory_conflict")
    );
    stages.insert("conflict", serde_json::to_value(service.inspect()).unwrap());
    assert_eq!(memory(&service.inspect()), memory(&edited));
    service.shutdown().await;
    let mut client = OwnerClient::open_with(&root, store.clone()).unwrap();
    let mut owner = client.connect().await.unwrap();
    assert!(matches!(
        owner
            .call(OwnerOperation::WriteDocument {
                input: WriteDocument {
                    bot,
                    kind: DocumentKind::Memory,
                    expected_revision: 1,
                    sources: sources.clone(),
                    markdown: "Manager CAS must refuse".into(),
                }
            })
            .await,
        Err(ClientError::Refused(_))
    ));
    drop(owner);
    drop(client);
    let service = RemoteService::new(root.clone(), store.clone());
    let persisted_edit = service.restore().await.unwrap();
    assert_eq!(memory(&persisted_edit), memory(&edited));

    let cleared = service
        .command(IntentCommand::WriteMemory {
            bot,
            expected_revision: 2,
            markdown: String::new(),
        })
        .await
        .unwrap();
    assert_eq!(memory(&cleared).revision, 3);
    assert_eq!(memory(&cleared).markdown, "");
    assert_eq!(memory(&cleared).sources, sources);
    stages.insert("cleared", serde_json::to_value(&cleared).unwrap());
    service.shutdown().await;
    let path = root.join("intents.json");
    let mut intents = Vec::new();
    let exact = WriteDocument {
        bot,
        kind: DocumentKind::Memory,
        expected_revision: 2,
        sources: sources.clone(),
        markdown: String::new(),
    };
    let mut wrong_revision = exact.clone();
    wrong_revision.expected_revision = 3;
    let mut later_revision = exact.clone();
    later_revision.expected_revision = 0;
    let mut wrong_sources = exact.clone();
    wrong_sources.sources.reverse();
    let mut wrong_markdown = exact.clone();
    wrong_markdown.markdown = "Never replay this edit".into();
    let mut wrong_bot = exact.clone();
    wrong_bot.bot = BotId::new();
    let mut overflow = exact.clone();
    overflow.expected_revision = u64::MAX;
    for input in [
        exact,
        wrong_revision,
        later_revision,
        wrong_sources,
        wrong_markdown,
        wrong_bot,
        overflow,
    ] {
        intents.push(Intent {
            id: uuid::Uuid::new_v4(),
            operation: OwnerOperation::WriteDocument { input },
            status: "unknown".into(),
        });
    }
    std::fs::write(&path, serde_json::to_vec(&intents).unwrap()).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let service = RemoteService::new(root.clone(), store.clone());
    let recovered = service.restore().await.unwrap();
    assert_eq!(memory(&recovered), memory(&cleared));
    assert_eq!(recovered.intents[0].status, "confirmed");
    assert!(recovered.intents[1..].iter().all(|i| i.status == "unknown"));
    stages.insert("uncertain", serde_json::to_value(&recovered).unwrap());
    assert_eq!(
        service
            .command(IntentCommand::WriteMemory {
                bot,
                expected_revision: 3,
                markdown: "Blocked".into()
            })
            .await
            .err()
            .as_deref(),
        Some("unresolved_intent")
    );
    for intent in &intents[1..] {
        service
            .command(IntentCommand::AcknowledgeUncertainty(intent.id))
            .await
            .unwrap();
    }
    let acknowledged = service.command(IntentCommand::Refresh).await.unwrap();
    assert_eq!(memory(&acknowledged), memory(&cleared));
    assert!(
        acknowledged.intents[1..]
            .iter()
            .all(|i| i.status == "attention")
    );
    stages.insert("acknowledged", serde_json::to_value(&acknowledged).unwrap());
    service.shutdown().await;
    let service = RemoteService::new(root.clone(), store);
    let restarted = service.restore().await.unwrap();
    assert_eq!(memory(&restarted), memory(&cleared));
    assert!(
        restarted.intents[1..]
            .iter()
            .all(|i| i.status == "attention")
    );
    let restored = service
        .command(IntentCommand::WriteMemory {
            bot,
            expected_revision: 3,
            markdown: "Deliberate new edit after current-state review.".into(),
        })
        .await
        .unwrap();
    assert_eq!(memory(&restored).revision, 4);
    assert_eq!(memory(&restored).sources, sources);
    assert_eq!(
        restored.intents[1..7]
            .iter()
            .filter(|i| i.status == "attention")
            .count(),
        6
    );
    stages.insert("restored", serde_json::to_value(&restored).unwrap());
    let reviewed = service
        .command(IntentCommand::WriteMemory {
            bot,
            expected_revision: 4,
            markdown: "Reviewed draft after conflict.".into(),
        })
        .await
        .unwrap();
    assert_eq!(memory(&reviewed).revision, 5);
    assert_eq!(memory(&reviewed).sources, sources);
    stages.insert("reviewed", serde_json::to_value(&reviewed).unwrap());

    if let Some(output) = std::env::var_os("BOTSTRAL_MEMORY_VIEWS") {
        std::fs::write(output, serde_json::to_vec_pretty(&stages).unwrap()).unwrap();
    }
    service.sign_out().await.unwrap();
    service.shutdown().await;
    println!(
        "PASS Memory projection, existing-only writes, source order, stale refusal, edit/clear persistence, native restart, exact reconciliation, unchanged/later/source/bot/overflow non-confirmation, no replay and acknowledgement"
    );
}
