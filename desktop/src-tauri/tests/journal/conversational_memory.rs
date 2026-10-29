use super::*;
use botstral_desktop::remote::RemoteSnapshot;
use serde_json::{Value, json};

async fn wait_file(path: &Path) {
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        while !path.exists() {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
}
async fn script(root: &Path, proposal: ToolProposal, fail: bool) {
    let (name, args) = match proposal {
        ToolProposal::Memory(args) => ("memory", json!(args)),
        ToolProposal::RequestVaultEntry(args) => ("request_vault_entry", json!(args)),
        _ => unreachable!(),
    };
    let final_body = include_str!("../../../../manager/tests/fixtures/m04/completion.json");
    let mut body: Value = serde_json::from_str(final_body).unwrap();
    body["choices"][0]["finish_reason"] = json!("tool_calls");
    body["choices"][0]["message"] = json!({"role":"assistant","content":null,"tool_calls":[{"id":"native-memory-call","type":"function","function":{"name":name,"arguments":args.to_string()}}]});
    let bodies = [
        body.to_string(),
        if fail { "{}".into() } else { final_body.into() },
    ];
    let ack = root.join("provider-script-loaded");
    let _ = std::fs::remove_file(&ack);
    let temporary = root.join(".provider-script.tmp");
    std::fs::write(&temporary, serde_json::to_vec(&bodies).unwrap()).unwrap();
    std::fs::rename(temporary, root.join("provider-script.json")).unwrap();
    wait_file(&ack).await;
}
fn input(bot: BotId, text: &str) -> SendInput {
    SendInput {
        bot,
        session: None,
        text: text.into(),
        account: ProcessAccount {
            uid: 1000,
            gid: 1000,
        },
        runtime: None,
        process: false,
        seconds: 120,
        turns: 4,
        tokens: 200000,
        cost_microusd: 200000,
        output_tokens: 1024,
    }
}
async fn send(service: &RemoteService, bot: BotId, text: &str) -> RemoteSnapshot {
    service
        .command(IntentCommand::Send(input(bot, text)))
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            let snapshot = service.command(IntentCommand::Refresh).await.unwrap();
            if snapshot
                .workspace
                .as_ref()
                .unwrap()
                .runs
                .iter()
                .any(|r| r.request.prompt == text && r.summary.phase.terminal())
            {
                return snapshot;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap()
}
fn entry(snapshot: &RemoteSnapshot, prompt: &str) -> MemoryEntry {
    let run = snapshot
        .workspace
        .as_ref()
        .unwrap()
        .runs
        .iter()
        .find(|r| r.request.prompt == prompt)
        .unwrap();
    match run.turns[0].manager_outcome.as_ref().unwrap() {
        ManagerOutcome::Memory {
            outcome: MemoryOutcome::Created { entry },
        } => entry.clone(),
        _ => panic!("created receipt"),
    }
}
pub(super) async fn verify() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("owner");
    let fixture_root = PathBuf::from(std::env::var_os("BOTSTRAL_C02_FIXTURE_ROOT").unwrap());
    let (service, store, fixture) = paired(&root).await;
    let bot: BotId = fixture["bot"].as_str().unwrap().parse().unwrap();
    let secret = "synthetic-m04-provider-secret";
    for text in [secret.to_owned(), serde_json::to_string(secret).unwrap()] {
        assert!(
            service
                .command(IntentCommand::Send(input(bot, &text)))
                .await
                .is_err()
        );
        assert!(
            !std::fs::read_to_string(root.join("intents.json"))
                .unwrap()
                .contains(secret)
        );
        assert!(
            !serde_json::to_string(&service.inspect())
                .unwrap()
                .contains(secret)
        );
    }
    service.command(IntentCommand::Refresh).await.unwrap();
    let mut views = serde_json::Map::new();
    views.insert(
        "before".into(),
        serde_json::to_value(service.inspect()).unwrap(),
    );
    script(
        &fixture_root,
        ToolProposal::Memory(MemoryProposal::Create {
            scope: MemoryScopeSelection::Individual,
            markdown: "Use concise answers.".into(),
        }),
        false,
    )
    .await;
    let created = send(
        &service,
        bot,
        "Remember that I prefer concise answers for this bot.",
    )
    .await;
    let individual = entry(
        &created,
        "Remember that I prefer concise answers for this bot.",
    );
    views.insert("created".into(), serde_json::to_value(&created).unwrap());
    script(
        &fixture_root,
        ToolProposal::Memory(MemoryProposal::Create {
            scope: MemoryScopeSelection::Shared,
            markdown: "Keep morning meetings.".into(),
        }),
        false,
    )
    .await;
    let shared = send(&service, bot, "Remember morning meetings for all bots.").await;
    views.insert("shared".into(), serde_json::to_value(&shared).unwrap());
    script(
        &fixture_root,
        ToolProposal::Memory(MemoryProposal::Update {
            scope: MemoryScopeSelection::Individual,
            id: individual.id,
            expected_revision: 1,
            markdown: "Use short, complete sentences.".into(),
        }),
        true,
    )
    .await;
    let updated = send(
        &service,
        bot,
        "Update my individual preference to short, complete sentences.",
    )
    .await;
    let run = updated
        .workspace
        .as_ref()
        .unwrap()
        .runs
        .iter()
        .find(|r| r.request.prompt.starts_with("Update my"))
        .unwrap();
    assert!(matches!(
        run.summary.phase,
        RunPhase::AwaitingAttention { .. }
    ));
    assert!(matches!(
        run.turns[0].manager_outcome,
        Some(ManagerOutcome::Memory {
            outcome: MemoryOutcome::Updated { .. }
        })
    ));
    views.insert("updated".into(), serde_json::to_value(&updated).unwrap());
    script(
        &fixture_root,
        ToolProposal::Memory(MemoryProposal::List {
            scope: MemoryScopeSelection::Shared,
        }),
        true,
    )
    .await;
    views.insert(
        "listed".into(),
        serde_json::to_value(send(&service, bot, "List our shared memories.").await).unwrap(),
    );
    script(
        &fixture_root,
        ToolProposal::Memory(MemoryProposal::Delete {
            scope: MemoryScopeSelection::Individual,
            id: individual.id,
            expected_revision: 2,
        }),
        false,
    )
    .await;
    views.insert(
        "deleted".into(),
        serde_json::to_value(
            send(&service, bot, "Forget my individual response preference.").await,
        )
        .unwrap(),
    );
    script(
        &fixture_root,
        ToolProposal::RequestVaultEntry(RequestVaultEntry {
            purpose: VaultPurpose::ReusableLogin,
        }),
        false,
    )
    .await;
    views.insert(
        "vault".into(),
        serde_json::to_value(send(&service, bot, "Can you save a reusable login for me?").await)
            .unwrap(),
    );
    std::fs::write(fixture_root.join("restart-manager"), b"restart").unwrap();
    wait_file(&fixture_root.join("manager-restarted")).await;
    let restored = service.command(IntentCommand::Refresh).await.unwrap();
    views.insert("restored".into(), serde_json::to_value(&restored).unwrap());
    let workspace = restored.workspace.as_ref().unwrap();
    assert!(
        workspace
            .runs
            .iter()
            .all(|r| r.request.session.is_none() && !r.request.execution_policy.requires_session())
    );
    assert!(workspace.state.tasks.is_empty());
    service.shutdown().await;
    let obsolete = Intent {
        id: uuid::Uuid::new_v4(),
        status: "unknown".into(),
        operation: OwnerOperation::WriteDocument {
            input: WriteDocument {
                bot,
                kind: DocumentKind::Memory,
                expected_revision: 0,
                sources: vec![],
                markdown: "Obsolete mutation must never replay.".into(),
            },
        },
    };
    std::fs::write(
        root.join("intents.json"),
        serde_json::to_vec(&[obsolete]).unwrap(),
    )
    .unwrap();
    std::fs::set_permissions(
        root.join("intents.json"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let restarted = RemoteService::new(root, store);
    restarted.restore().await.unwrap();
    let stale = restarted.command(IntentCommand::Refresh).await.unwrap();
    assert_eq!(stale.intents.len(), 1);
    assert_eq!(stale.intents[0].status, "attention");
    assert!(
        stale.intents[0]
            .description
            .contains("will not be replayed")
    );
    assert_eq!(
        stale.workspace.as_ref().unwrap().runs.len(),
        workspace.runs.len()
    );
    assert!(
        stale
            .workspace
            .as_ref()
            .unwrap()
            .state
            .documents
            .iter()
            .all(|d| d.kind != DocumentKind::Memory)
    );
    views.insert("stale".into(), serde_json::to_value(stale).unwrap());
    if let Ok(path) = std::env::var("BOTSTRAL_MEMORY_NATIVE_VIEWS") {
        std::fs::write(path, serde_json::to_vec_pretty(&views).unwrap()).unwrap();
    }
    restarted.shutdown().await;
}
