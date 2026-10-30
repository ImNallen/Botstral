use super::*;
use botstral_desktop::remote::RemoteSnapshot;
use serde_json::Value;

async fn wait_task(session: &mut botstral_owner_client::OwnerSession, id: TaskId) -> TaskView {
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let OwnerReply::Task(task) = session.call(OwnerOperation::Task { id }).await.unwrap()
            else {
                panic!("Expected task")
            };
            if task.state.terminal() {
                return *task;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap()
}
async fn wait_file(path: &Path) {
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        while !path.exists() {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
}
fn record(views: &mut serde_json::Map<String, Value>, key: &str, value: &impl serde::Serialize) {
    views.insert(key.into(), serde_json::to_value(value).unwrap());
}
fn recent(snapshot: &RemoteSnapshot, id: TaskId) -> bool {
    snapshot
        .workspace
        .as_ref()
        .unwrap()
        .state
        .tasks
        .iter()
        .any(|t| t.id == id)
}
pub(super) async fn verify() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("owner");
    let fixture_root = PathBuf::from(std::env::var_os("BOTSTRAL_C02_FIXTURE_ROOT").unwrap());
    let (service, store, fixture) = paired(&root).await;
    let computer: ComputerSessionId = fixture["session"].as_str().unwrap().parse().unwrap();
    let mut views = serde_json::Map::new();
    record(&mut views, "before", &service.inspect());
    let local = LocalManager::new().unwrap();
    local
        .sign_in(
            fixture["local"].as_str().unwrap().into(),
            Zeroizing::new(fixture["password"].as_str().unwrap().into()),
        )
        .await
        .unwrap();
    let other = BotId::new();
    local
        .create_bot(other.0, "Other result bot".into())
        .await
        .unwrap();
    service.shutdown().await;
    let mut client = OwnerClient::open_with(&root, store.clone()).unwrap();
    let mut session = client.connect().await.unwrap();
    let workspace = service.inspect().workspace.unwrap();
    let worker = workspace
        .sessions
        .iter()
        .find(|s| s.id == computer)
        .unwrap()
        .worker_id;
    let OwnerReply::ComputerSession(second) = session
        .call(OwnerOperation::OpenComputerSession { bot: other, worker })
        .await
        .unwrap()
    else {
        panic!()
    };
    session
        .call(OwnerOperation::SetPermission {
            bot: other,
            expected_revision: 1,
            allow: true,
        })
        .await
        .unwrap();
    let deadline = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        + 600;
    let mut ids = Vec::new();
    for index in 0..35 {
        let id = TaskId::new();
        let input = SubmitTask {
            id,
            session_id: if index == 1 { second.id } else { computer },
            conversation_id: None,
            settings: TaskSettings {
                model: "P05 synthetic worker".into(),
                max_turns: 1,
                max_tokens: 64,
                deadline,
                max_cost_micros: 0,
            },
            work: WorkSpec::Computer {
                spec: Box::new(ComputerSpec {
                    profile: ComputerProfile::LinuxPlaywrightV1,
                    account: ProcessAccount {
                        uid: 1000,
                        gid: 1000,
                    },
                    runtime: ComputerRuntime {
                        sha256: fixture["runtime"]
                            .as_str()
                            .unwrap()
                            .to_owned()
                            .try_into()
                            .unwrap(),
                    },
                    deadline,
                    limits: ProcessLimits {
                        runtime_seconds: 120,
                        output_bytes: 1024,
                        processes: 64,
                        memory_bytes: 512 * 1024 * 1024,
                    },
                    viewport: Viewport {
                        width: 800,
                        height: 600,
                    },
                    operations: vec![
                        ComputerOperation::WriteFile {
                            path: "/tmp/p05-result.txt".into(),
                            text: "Exact ordinary action contents must remain visible.".into(),
                        },
                        ComputerOperation::ReadFile {
                            path: "/tmp/p05-result.txt".into(),
                            output: "text".into(),
                        },
                    ],
                    outputs: vec![OutputDeclaration {
                        slot: "text".into(),
                        name: if index == 0 {
                            "00-old-result.txt".into()
                        } else if index == 1 {
                            "literal %_*.txt".into()
                        } else {
                            "Duplicate.txt".into()
                        },
                        media_type: MediaType::TextPlain,
                        max_bytes: 1024,
                    }],
                }),
            },
        };
        session
            .call(OwnerOperation::SubmitTask { input })
            .await
            .unwrap();
        session
            .call(OwnerOperation::GrantApproval {
                id,
                expires_at: deadline,
            })
            .await
            .unwrap();
        wait_task(&mut session, id).await;
        ids.push(id);
    }
    let old = wait_task(&mut session, ids[0]).await;
    for id in &ids[1..] {
        wait_task(&mut session, *id).await;
    }
    let ArtifactState::Available { id: artifact, .. } = old.outputs[0].state else {
        panic!("Expected sealed output")
    };
    for _ in 0..34 {
        let id = TaskId::new();
        session
            .call(OwnerOperation::SubmitTask {
                input: SubmitTask {
                    id,
                    session_id: computer,
                    conversation_id: None,
                    settings: old.request.settings.clone(),
                    work: WorkSpec::Unavailable,
                },
            })
            .await
            .unwrap();
        session
            .call(OwnerOperation::CancelTask { id })
            .await
            .unwrap();
        wait_task(&mut session, id).await;
    }
    drop(session);
    drop(client);
    let service = RemoteService::new(root.clone(), store.clone());
    let image = service.restore().await.unwrap();
    assert!(!recent(&image, old.id));
    record(&mut views, "library_workspace", &image);
    let query = ArtifactQuery {
        bot: None,
        name: String::new(),
        after: None,
        limit: 32,
    };
    let page = service.saved_results(query.clone()).await.unwrap();
    assert_eq!(page.page.entries.len(), 32);
    assert_eq!(page.page.entries[0].task, old.id);
    record(&mut views, "page", &page);
    let second_page = service
        .saved_results(ArtifactQuery {
            after: page.page.next.clone(),
            ..query.clone()
        })
        .await
        .unwrap();
    assert_eq!(second_page.page.entries.len(), 3);
    record(&mut views, "next", &second_page);
    let filtered = service
        .saved_results(ArtifactQuery {
            bot: Some(other),
            ..query.clone()
        })
        .await
        .unwrap();
    assert_eq!(filtered.page.entries.len(), 1);
    record(&mut views, "other_bot", &filtered);
    let literal = service
        .saved_results(ArtifactQuery {
            name: "%_*".into(),
            ..query.clone()
        })
        .await
        .unwrap();
    assert_eq!(literal.page.entries.len(), 1);
    record(&mut views, "literal", &literal);
    let source = service.source_task(old.id).await.unwrap();
    let public = serde_json::to_value(&source).unwrap();
    assert!(
        public["task"]["action"]
            .as_str()
            .unwrap()
            .contains("Exact ordinary action contents must remain visible.")
    );
    record(&mut views, "source", &source);
    assert!(!recent(&service.inspect(), old.id));
    let bytes = service.artifact(old.id, artifact).await.unwrap().bytes;
    assert_eq!(
        bytes,
        b"Synthetic C02 recorded worker output. No production process ran.\n"
    );
    service.shutdown().await;
    for stage in ["before-restart", "after-restart"] {
        if stage == "after-restart" {
            std::fs::write(fixture_root.join("restart-manager"), b"restart").unwrap();
            wait_file(&fixture_root.join("manager-restarted")).await;
        }
        let native = RemoteService::new(root.clone(), store.clone());
        native.restore().await.unwrap();
        let results = native.saved_results(query.clone()).await.unwrap();
        assert!(results.page.entries.iter().any(|e| e.task == old.id));
        assert_eq!(
            serde_json::to_value(native.source_task(old.id).await.unwrap()).unwrap()["task"]["id"],
            old.id.to_string()
        );
        let verified = native.artifact(old.id, artifact).await.unwrap();
        assert_eq!(verified.bytes, bytes);
        let destination = directory.path().join(format!("{stage}.txt"));
        native
            .save_export(
                native.owner_scope().unwrap(),
                old.id,
                &destination,
                verified,
            )
            .await
            .unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), bytes);
        assert_eq!(
            std::fs::metadata(&destination)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        native.shutdown().await;
    }
    let native = RemoteService::new(root.clone(), store);
    native.restore().await.unwrap();
    let scope = native.owner_scope().unwrap();
    let mismatched = botstral_desktop::remote::OwnerReadScope {
        manager: ManagerId::new(),
        ..scope
    };
    let wrong_manager = directory.path().join("wrong-manager.txt");
    assert!(
        native
            .save_export(
                mismatched,
                old.id,
                &wrong_manager,
                native.artifact(old.id, artifact).await.unwrap()
            )
            .await
            .is_err()
    );
    assert!(!wrong_manager.exists());

    let verified = native.artifact(old.id, artifact).await.unwrap();
    native.sign_out().await.unwrap();
    let destination = directory.path().join("signed-out.txt");
    assert!(
        native
            .save_export(scope, old.id, &destination, verified)
            .await
            .is_err()
    );
    assert!(!destination.exists());
    native.shutdown().await;
    local.sign_out();
    if let Some(path) = std::env::var_os("BOTSTRAL_P05_NATIVE_VIEWS") {
        std::fs::write(path, serde_json::to_vec_pretty(&views).unwrap()).unwrap();
    }
    println!(
        "PASS P05: 35 sealed results, two bots, literal search, 32+3 pages, retained source absent from recent history, authoritative byte verification, manager and native restart, real private local saves, sign-out export refusal. Synthetic worker only."
    );
}
