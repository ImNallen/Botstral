use botstral_desktop::{
    keychain::NativeSecrets,
    local::LocalManager,
    remote::{IntentCommand, RemoteService, SendInput, save_artifact},
};
use botstral_link::*;
use botstral_owner_client::{ClientError, SecretStore};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use zeroize::Zeroizing;

#[derive(Default)]
struct TestStore {
    bytes: Mutex<Option<Zeroizing<Vec<u8>>>>,
    locked: AtomicBool,
    delete_fails: AtomicBool,
}
impl SecretStore for TestStore {
    fn read(&self) -> Result<Option<Zeroizing<Vec<u8>>>, ClientError> {
        if self.locked.load(Ordering::SeqCst) {
            return Err(ClientError::Storage);
        }
        Ok(self.bytes.lock().unwrap().clone())
    }
    fn replace(&self, bytes: Zeroizing<Vec<u8>>) -> Result<(), ClientError> {
        if self.locked.load(Ordering::SeqCst) {
            return Err(ClientError::Storage);
        }
        *self.bytes.lock().unwrap() = Some(bytes);
        Ok(())
    }
    fn delete(&self) -> Result<(), ClientError> {
        if self.delete_fails.load(Ordering::SeqCst) {
            return Err(ClientError::Storage);
        }
        self.bytes.lock().unwrap().take();
        Ok(())
    }
}
#[tokio::test]
async fn unavailable_native_storage_never_publishes_a_fingerprint() {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(TestStore::default());
    store.locked.store(true, Ordering::SeqCst);
    let service = RemoteService::new(directory.path().join("owner"), store);
    assert!(
        service
            .command(IntentCommand::Begin {
                label: "Locked".into()
            })
            .await
            .is_err()
    );
    assert!(service.inspect().pairing.is_none());
    assert!(service.inspect().workspace.is_none());
    service.shutdown().await;
}
#[tokio::test]
async fn failed_secret_deletion_leaves_a_durable_signed_out_tombstone() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("owner");
    let store = Arc::new(TestStore::default());
    let service = RemoteService::new(root.clone(), store.clone());
    let first = service
        .command(IntentCommand::Begin {
            label: "Synthetic".into(),
        })
        .await
        .unwrap();
    store.delete_fails.store(true, Ordering::SeqCst);
    let signed_out = service.sign_out().await.unwrap();
    assert!(signed_out.generation > first.generation);
    assert_eq!(
        signed_out.issue.as_deref(),
        Some("credential_deletion_failed")
    );
    assert!(store.read().unwrap().is_some());
    assert!(root.join("signed-out.json").exists());
    let reopened = RemoteService::new(root, store.clone());
    assert!(reopened.restore().await.unwrap().workspace.is_none());
    assert!(
        reopened
            .command(IntentCommand::Begin {
                label: "Retry".into()
            })
            .await
            .is_err()
    );
    reopened.shutdown().await;
}
#[test]
fn pairing_links_are_bounded_data_and_cannot_supply_navigation_or_paths() {
    for value in [
        "https://example.com/",
        "botstral://pair?url=https://example.com",
        "botstral://pair?bundle=bad#fragment",
        "botstral://pair?bundle=e30",
    ] {
        assert!(botstral_desktop::remote::parse_pairing_link(value).is_err());
    }
    assert!(botstral_desktop::remote::parse_pairing_link(&"a".repeat(65537)).is_err());
}

#[tokio::test]
async fn sign_out_clears_native_authority_when_the_profile_directory_was_removed() {
    for deletion_fails in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("owner");
        let store = Arc::new(TestStore::default());
        let service = RemoteService::new(root.clone(), store.clone());
        service
            .command(IntentCommand::Begin {
                label: "Missing profile".into(),
            })
            .await
            .unwrap();
        service.shutdown().await;
        std::fs::remove_dir_all(&root).unwrap();
        assert!(store.read().unwrap().is_some());
        store.delete_fails.store(deletion_fails, Ordering::SeqCst);

        let signed_out = service.sign_out().await.unwrap();
        assert_eq!(signed_out.status, "signed_out");
        assert!(signed_out.workspace.is_none());
        if deletion_fails {
            assert_eq!(
                signed_out.issue.as_deref(),
                Some("credential_deletion_failed")
            );
            let reopened = RemoteService::new(root, store.clone());
            let restored = reopened.restore().await.unwrap();
            assert_eq!(restored.status, "signed_out");
            assert!(restored.workspace.is_none());
            reopened.shutdown().await;
        } else {
            assert!(
                store.read().unwrap().is_none(),
                "Successful sign-out must delete the native credential after profile removal"
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_sign_out_callers_wait_until_native_deletion_finishes() {
    struct BlockingDeletion {
        inner: TestStore,
        entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        released: (Mutex<bool>, std::sync::Condvar),
    }
    impl SecretStore for BlockingDeletion {
        fn read(&self) -> Result<Option<Zeroizing<Vec<u8>>>, ClientError> {
            self.inner.read()
        }
        fn replace(&self, bytes: Zeroizing<Vec<u8>>) -> Result<(), ClientError> {
            self.inner.replace(bytes)
        }
        fn delete(&self) -> Result<(), ClientError> {
            if let Some(entered) = self.entered.lock().unwrap().take() {
                let _ = entered.send(());
                let mut released = self.released.0.lock().unwrap();
                while !*released {
                    released = self.released.1.wait(released).unwrap();
                }
            }
            self.inner.delete()
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let (entered, deleting) = tokio::sync::oneshot::channel();
    let store = Arc::new(BlockingDeletion {
        inner: TestStore::default(),
        entered: Mutex::new(Some(entered)),
        released: (Mutex::new(false), std::sync::Condvar::new()),
    });
    let service = RemoteService::new(directory.path().join("owner"), store.clone());
    service
        .command(IntentCommand::Begin {
            label: "Concurrent sign-out".into(),
        })
        .await
        .unwrap();
    let first_service = service.clone();
    let first = tokio::spawn(async move { first_service.sign_out().await });
    deleting.await.unwrap();
    assert!(store.read().unwrap().is_some());
    let second_service = service.clone();
    let mut second = tokio::spawn(async move { second_service.sign_out().await });
    let returned_early = tokio::time::timeout(Duration::from_millis(100), &mut second)
        .await
        .is_ok();
    *store.released.0.lock().unwrap() = true;
    store.released.1.notify_all();
    first.await.unwrap().unwrap();
    if !returned_early {
        second.await.unwrap().unwrap();
    }
    assert!(store.read().unwrap().is_none());
    assert!(
        !returned_early,
        "A concurrent sign-out reported success while native deletion was blocked"
    );
}

async fn wait(
    service: &RemoteService,
    predicate: impl Fn(&OwnerWorkspace) -> bool,
) -> OwnerWorkspace {
    let mut changes = service.observe();
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let state = changes.borrow_and_update().clone();
            if let Some(workspace) = state.workspace
                && predicate(&workspace)
            {
                return workspace;
            }
            changes
                .changed()
                .await
                .expect("native activity channel remains open");
        }
    })
    .await
    .expect("authoritative fixture outcome")
}

#[tokio::test]
#[ignore = "Requires desktop/fixtures/paired_manager.py and the real unlocked OS keychain."]
async fn actual_manager_native_keychain_run_approval_artifact_and_lifecycle() {
    let fixture_root =
        PathBuf::from(std::env::var_os("BOTSTRAL_C02_FIXTURE_ROOT").expect("fixture root"));
    let fixture: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture_root.join("fixture.json")).unwrap()).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("native-owner");
    let slot = format!("c02-contract-{}", uuid::Uuid::new_v4());
    let store = Arc::new(NativeSecrets::new(&slot).unwrap());
    struct Cleanup(Arc<NativeSecrets>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = self.0.delete();
        }
    }
    let _cleanup = Cleanup(store.clone());
    assert!(store.read().unwrap().is_none());
    let service = RemoteService::new(root.clone(), store.clone());
    let local = LocalManager::new().unwrap();
    local
        .sign_in(
            fixture["local"].as_str().unwrap().into(),
            Zeroizing::new(fixture["password"].as_str().unwrap().into()),
        )
        .await
        .unwrap();
    let request = service
        .command(IntentCommand::Begin {
            label: "C02 native contract".into(),
        })
        .await
        .unwrap()
        .pairing
        .unwrap();
    let trust =
        serde_json::from_slice(&std::fs::read(fixture_root.join("trust.json")).unwrap()).unwrap();
    let bundle = local.invite_device(request.clone(), trust).await.unwrap();
    let invitation = Zeroizing::new(bundle.invitation.clone());
    let candidate_view = service.command(IntentCommand::Stage(bundle)).await.unwrap();
    let candidate = candidate_view.candidate.as_ref().unwrap().clone();
    let mut public_views = vec![serde_json::to_value(&candidate_view).unwrap()];
    let online = service
        .command(IntentCommand::Confirm(candidate.id))
        .await
        .unwrap();
    public_views.push(serde_json::to_value(&online).unwrap());
    assert_eq!(online.status, "online");
    assert!(!root.join("device.json").exists());
    let secret = store.read().unwrap().unwrap();
    let record: serde_json::Value = serde_json::from_slice(&secret).unwrap();
    let credential = Zeroizing::new(record["credential"].as_str().unwrap().to_owned());
    assert!(record["invitation"].is_null());
    let public = serde_json::to_string(&online).unwrap();
    assert!(!public.contains(credential.as_str()) && !public.contains(invitation.as_str()));
    let bot: BotId = fixture["bot"].as_str().unwrap().parse().unwrap();
    let session = fixture["session"].as_str().unwrap().parse().unwrap();
    let send = |text: &str, browser: bool| SendInput {
        bot,
        session,
        text: text.into(),
        account: ProcessAccount {
            uid: 1000,
            gid: 1000,
        },
        runtime: browser.then(|| {
            fixture["runtime"]
                .as_str()
                .unwrap()
                .to_owned()
                .try_into()
                .unwrap()
        }),
        process: true,
        seconds: 120,
        turns: 4,
        tokens: 200_000,
        cost_microusd: 200_000,
        output_tokens: 1024,
    };
    service
        .command(IntentCommand::Send(send("C02 native chat", false)))
        .await
        .unwrap();
    let chat = wait(&service, |w| {
        w.runs.iter().any(|r| {
            r.request.prompt == "C02 native chat" && r.summary.phase == RunPhase::Completed
        })
    })
    .await;
    let submitted = chat
        .runs
        .iter()
        .find(|r| r.request.prompt == "C02 native chat")
        .unwrap();
    assert_eq!(submitted.request.configuration_revision, 2);
    let projected = serde_json::to_value(service.inspect()).unwrap();
    let configurations = projected["workspace"]["configurations"].as_array().unwrap();
    assert_eq!(configurations.len(), 1);
    assert_eq!(configurations[0]["ready"], true);
    assert!(chat.runs.iter().flat_map(|r| &r.turns).any(|t| matches!(&t.response, Some(RunMessage::Assistant { content, .. }) if content.contains("durable recorded assistant"))));
    service
        .command(IntentCommand::Send(send("Create a file", true)))
        .await
        .unwrap();
    let proposed = wait(&service, |w| {
        w.state.tasks.iter().any(|t| !t.approval.consumed)
    })
    .await;
    let task = proposed
        .state
        .tasks
        .iter()
        .find(|t| !t.approval.consumed)
        .unwrap();
    let task_id = task.id;
    let review = service
        .command(IntentCommand::Review(task_id))
        .await
        .unwrap()
        .review
        .unwrap();
    let public_review = serde_json::to_value(service.inspect()).unwrap();
    let action = public_review["review"]["action"].as_str().unwrap();
    assert!(action.starts_with("1. Write file "));
    assert!(action.find("Exact contents:").unwrap() < action.find("Execution target").unwrap());
    assert!(action.contains(&serde_json::to_string_pretty(&review.command).unwrap()));
    public_views.push(public_review);
    service
        .command(IntentCommand::Approve(review.id))
        .await
        .unwrap();
    let completed = wait(&service, |w| {
        w.state.tasks.iter().any(|t| {
            t.id == task_id
                && t.state == TaskState::Completed
                && t.outputs
                    .iter()
                    .any(|o| matches!(o.state, ArtifactState::Available { .. }))
        })
    })
    .await;
    public_views.push(serde_json::to_value(service.inspect()).unwrap());
    let task = completed
        .state
        .tasks
        .iter()
        .find(|t| t.id == task_id)
        .unwrap();
    let artifact = task
        .outputs
        .iter()
        .find_map(|o| {
            if let ArtifactState::Available { id, .. } = o.state {
                Some(id)
            } else {
                None
            }
        })
        .unwrap();
    let verified = service.artifact(task_id, artifact).await.unwrap();
    let destination = directory.path().join("export.txt");
    save_artifact(&destination, verified).unwrap();
    assert!(
        std::fs::read_to_string(destination)
            .unwrap()
            .contains("Synthetic C02")
    );
    service
        .command(IntentCommand::Send(send(
            "Please cancel a long operation",
            false,
        )))
        .await
        .unwrap();
    let cancelling = wait(&service, |w| {
        w.runs
            .iter()
            .filter(|r| r.request.prompt == "Please cancel a long operation")
            .flat_map(|r| &r.turns)
            .any(|t| t.intent.is_some())
    })
    .await;
    let cancellation_task = cancelling
        .runs
        .iter()
        .find(|r| r.request.prompt == "Please cancel a long operation")
        .unwrap()
        .turns
        .iter()
        .find_map(|t| t.intent.as_ref().map(|i| i.task))
        .unwrap();
    let review = service
        .command(IntentCommand::Review(cancellation_task))
        .await
        .unwrap()
        .review
        .unwrap();
    service
        .command(IntentCommand::Approve(review.id))
        .await
        .unwrap();
    wait(&service, |w| {
        w.state
            .tasks
            .iter()
            .any(|t| t.id == cancellation_task && t.approval.consumed)
    })
    .await;
    service
        .command(IntentCommand::CancelTask(cancellation_task))
        .await
        .unwrap();
    let stopped = wait(&service, |w| {
        w.state
            .tasks
            .iter()
            .any(|t| t.id == cancellation_task && t.state == TaskState::Cancelled)
    })
    .await;
    assert!(matches!(
        stopped
            .state
            .tasks
            .iter()
            .find(|t| t.id == cancellation_task)
            .unwrap()
            .outcome,
        Some(ExecutionEvent::Stopped { .. })
    ));
    let suspended = service.command(IntentCommand::Suspend).await.unwrap();
    assert_eq!(suspended.status, "suspended");
    assert!(
        service
            .command(IntentCommand::Send(send("must not send", false)))
            .await
            .is_err()
    );
    assert_eq!(
        service.command(IntentCommand::Resume).await.unwrap().status,
        "online"
    );
    service.shutdown().await;
    let restored = RemoteService::new(root.clone(), store.clone());
    assert_eq!(restored.restore().await.unwrap().status, "online");
    for entry in std::fs::read_dir(&root).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        assert!(
            !bytes
                .windows(credential.len())
                .any(|v| v == credential.as_bytes())
        );
        assert!(
            !bytes
                .windows(invitation.len())
                .any(|v| v == invitation.as_bytes())
        );
    }
    let devices = local.devices().await.unwrap();
    let device = devices
        .iter()
        .find(|d| d.fingerprint == request.fingerprint)
        .unwrap()
        .id;
    local.revoke_device(device).await.unwrap();
    let revoked = restored.command(IntentCommand::Refresh).await;
    assert!(revoked.is_err());
    assert!(restored.inspect().workspace.is_none());
    let signed_out = restored.sign_out().await.unwrap();
    assert!(signed_out.workspace.is_none());
    public_views.push(serde_json::to_value(&signed_out).unwrap());
    if let Some(output) = std::env::var_os("BOTSTRAL_C02_VIEW_OUTPUT") {
        let bytes = serde_json::to_vec_pretty(&public_views).unwrap();
        assert!(
            !bytes
                .windows(credential.len())
                .any(|v| v == credential.as_bytes())
        );
        assert!(
            !bytes
                .windows(invitation.len())
                .any(|v| v == invitation.as_bytes())
        );
        std::fs::write(output, bytes).unwrap();
    }
    assert!(store.read().unwrap().is_none());
    local.sign_out();
    println!(
        "PASS actual manager, real native keychain, durable chat and linked task, exact review, verified export, suspension, restoration, credential scan, and revocation"
    );
}

#[tokio::test]
async fn a_journal_larger_than_secret_file_limits_restores_without_credentials_on_disk() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("owner");
    let store = Arc::new(TestStore::default());
    let service = RemoteService::new(root.clone(), store.clone());
    service
        .command(IntentCommand::Begin {
            label: "Journal".into(),
        })
        .await
        .unwrap();
    service.shutdown().await;
    let intents: Vec<_> = (0..16)
        .map(|_| botstral_desktop::remote::Intent {
            id: uuid::Uuid::new_v4(),
            status: "confirmed".into(),
            operation: OwnerOperation::SubmitRun {
                input: Box::new(SubmitRun {
                    id: RunId::new(),
                    bot: BotId::new(),
                    session: ComputerSessionId::new(),
                    configuration_revision: 1,
                    images: vec![],
                    prompt: "a".repeat(4096),
                    execution_policy: RunExecutionPolicy {
                        connectors: None,
                        vision: None,
                        vibe: None,
                        computer: None,
                        process: None,
                    },
                    limits: RunLimits {
                        deadline: 2000,
                        max_turns: 1,
                        max_total_tokens: 100,
                        max_cost_microusd: 100,
                        max_output_tokens: 20,
                        max_operations: 1,
                        max_concurrent_operations: 1,
                    },
                }),
            },
        })
        .collect();
    let bytes = serde_json::to_vec(&intents).unwrap();
    assert!(bytes.len() > 64 * 1024);
    let path = root.join("intents.json");
    std::fs::write(&path, bytes).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let reopened = RemoteService::new(root.clone(), store);
    assert_eq!(reopened.restore().await.unwrap().status, "pairing");
    assert!(!root.join("device.json").exists());
    reopened.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sign_out_blocks_new_intents_while_an_admitted_native_call_is_finishing() {
    struct GateStore {
        inner: TestStore,
        entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        released: (Mutex<bool>, std::sync::Condvar),
    }
    impl SecretStore for GateStore {
        fn read(&self) -> Result<Option<Zeroizing<Vec<u8>>>, ClientError> {
            if let Some(entered) = self.entered.lock().unwrap().take() {
                let _ = entered.send(());
                let mut released = self.released.0.lock().unwrap();
                while !*released {
                    released = self.released.1.wait(released).unwrap();
                }
            }
            self.inner.read()
        }
        fn replace(&self, bytes: Zeroizing<Vec<u8>>) -> Result<(), ClientError> {
            self.inner.replace(bytes)
        }
        fn delete(&self) -> Result<(), ClientError> {
            self.inner.delete()
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let (entered, reached) = tokio::sync::oneshot::channel();
    let store = Arc::new(GateStore {
        inner: TestStore::default(),
        entered: Mutex::new(Some(entered)),
        released: (Mutex::new(false), std::sync::Condvar::new()),
    });
    let service = RemoteService::new(directory.path().join("owner"), store.clone());
    let begin_service = service.clone();
    let beginning = tokio::spawn(async move {
        begin_service
            .command(IntentCommand::Begin {
                label: "Finishing".into(),
            })
            .await
    });
    reached.await.unwrap();
    let mut changes = service.observe();
    let signing_service = service.clone();
    let signing_out = tokio::spawn(async move { signing_service.sign_out().await });
    while changes.borrow().generation == 0 {
        changes.changed().await.unwrap();
    }
    let cleared = service.inspect().pairing.is_none();
    let blocked = tokio::time::timeout(
        Duration::from_secs(1),
        service.command(IntentCommand::Refresh),
    )
    .await;
    *store.released.0.lock().unwrap() = true;
    store.released.1.notify_all();
    assert!(cleared);
    assert!(matches!(blocked, Ok(Err(error)) if error == "stale"));
    let _ = beginning.await.unwrap();
    assert_eq!(signing_out.await.unwrap().unwrap().status, "signed_out");
    assert!(service.inspect().pairing.is_none());
    assert!(service.inspect().workspace.is_none());
    assert!(store.read().unwrap().is_none());
}
