use botstral_desktop::{
    local::LocalManager,
    remote::{Intent, IntentCommand, RemoteService, SendInput},
};
use botstral_link::*;
use botstral_owner_client::{ClientError, SecretStore};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use zeroize::Zeroizing;

#[derive(Default)]
struct MemorySecrets(Mutex<Option<Zeroizing<Vec<u8>>>>);
impl SecretStore for MemorySecrets {
    fn read(&self) -> Result<Option<Zeroizing<Vec<u8>>>, ClientError> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn replace(&self, value: Zeroizing<Vec<u8>>) -> Result<(), ClientError> {
        *self.0.lock().unwrap() = Some(value);
        Ok(())
    }
    fn delete(&self) -> Result<(), ClientError> {
        self.0.lock().unwrap().take();
        Ok(())
    }
}
async fn paired(root: &Path) -> (RemoteService, Arc<MemorySecrets>, serde_json::Value) {
    let fixture_root = PathBuf::from(std::env::var_os("BOTSTRAL_C02_FIXTURE_ROOT").unwrap());
    let fixture: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture_root.join("fixture.json")).unwrap()).unwrap();
    let store = Arc::new(MemorySecrets::default());
    let service = RemoteService::new(root.to_owned(), store.clone());
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
            label: "Independent review".into(),
        })
        .await
        .unwrap()
        .pairing
        .unwrap();
    let trust =
        serde_json::from_slice(&std::fs::read(fixture_root.join("trust.json")).unwrap()).unwrap();
    let bundle = local.invite_device(request, trust).await.unwrap();
    let candidate = service
        .command(IntentCommand::Stage(bundle))
        .await
        .unwrap()
        .candidate
        .unwrap();
    assert_eq!(
        service
            .command(IntentCommand::Confirm(candidate.id))
            .await
            .unwrap()
            .status,
        "online"
    );
    local.sign_out();
    (service, store, fixture)
}
#[tokio::test]
#[ignore = "Requires desktop/fixtures/paired_manager.py with its isolated actual manager."]
async fn journal_read_failure_must_preserve_unknown_and_block_mutation() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("owner");
    let (service, store, fixture) = paired(&root).await;
    let bot: BotId = fixture["bot"].as_str().unwrap().parse().unwrap();
    let permission = service
        .inspect()
        .workspace
        .unwrap()
        .state
        .permissions
        .into_iter()
        .find(|p| p.bot == bot)
        .unwrap();
    service.shutdown().await;
    let intent = Intent {
        id: uuid::Uuid::new_v4(),
        status: "unknown".into(),
        operation: OwnerOperation::SetPermission {
            bot,
            expected_revision: permission.revision,
            allow: !permission.allow,
        },
    };
    let journal = root.join("intents.json");
    let original = serde_json::to_vec(&vec![intent]).unwrap();
    std::fs::write(&journal, &original).unwrap();
    std::fs::set_permissions(&journal, std::fs::Permissions::from_mode(0o400)).unwrap();
    let reopened = RemoteService::new(root, store);
    let restored = reopened.restore().await;
    let after_restore = std::fs::read(&journal).unwrap();
    let begin = reopened
        .command(IntentCommand::Begin {
            label: "Do not reset uncertain work".into(),
        })
        .await;
    let mutation = reopened
        .command(IntentCommand::Permission {
            bot,
            expected: permission.revision,
            allow: !permission.allow,
        })
        .await;
    let current = reopened.inspect();
    reopened.shutdown().await;
    let (observer, _, _) = paired(&directory.path().join("observer")).await;
    let observed = observer.inspect().workspace.unwrap();
    let resulting_permission = observed
        .state
        .permissions
        .iter()
        .find(|p| p.bot == bot)
        .unwrap();
    println!(
        "JOURNAL_READ_FAILURE restored_status={:?} restored_disk_records={} pairing_ok={} mutation_ok={} permission_before={} permission_after={} unknown_visible={}",
        restored.as_ref().map(|v| v.status.as_str()),
        serde_json::from_slice::<Vec<Intent>>(&after_restore)
            .unwrap()
            .len(),
        begin.is_ok(),
        mutation.is_ok(),
        permission.revision,
        resulting_permission.revision,
        current
            .intents
            .iter()
            .filter(|i| i.status == "unknown")
            .count()
    );
    observer.shutdown().await;
    assert!(
        restored.is_err() && begin.is_err() && mutation.is_err(),
        "Required storage failure to retain unknown intent and block mutation; actual recovery erased journal and admitted mutation"
    );
    assert_eq!(resulting_permission.revision, permission.revision);
    assert_eq!(after_restore, original);
    assert_eq!(std::fs::read(&journal).unwrap(), original);
    std::fs::set_permissions(&journal, std::fs::Permissions::from_mode(0o600)).unwrap();
    let repaired = reopened.restore().await.unwrap();
    assert_eq!(repaired.intents.len(), 1);
    assert_eq!(repaired.intents[0].status, "unknown");
    reopened.shutdown().await;
}
#[tokio::test]
#[ignore = "Requires desktop/fixtures/paired_manager.py with its isolated actual manager."]
async fn admitted_journal_must_remain_readable_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("owner");
    let (service, store, fixture) = paired(&root).await;
    let bot = fixture["bot"].as_str().unwrap().parse().unwrap();
    let session = fixture["session"].as_str().unwrap().parse().unwrap();
    service.shutdown().await;
    let attention = uuid::Uuid::new_v4();
    let retained = Intent {
        id: attention,
        status: "attention".into(),
        operation: OwnerOperation::SetPermission {
            bot,
            expected_revision: 1_000_000,
            allow: false,
        },
    };
    std::fs::write(
        root.join("intents.json"),
        serde_json::to_vec(&vec![retained]).unwrap(),
    )
    .unwrap();
    let service = RemoteService::new(root.clone(), store.clone());
    service.restore().await.unwrap();
    for _ in 0..45 {
        let result = service
            .command(IntentCommand::Send(SendInput {
                bot,
                session,
                text: format!("a{}", "\u{1}".repeat(4095)),
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
            .await;
        assert!(
            matches!(result, Err(ref code) if code == "refused"),
            "Unexpected native admission result"
        );
    }
    let path = root.join("intents.json");
    let bytes = std::fs::metadata(&path).unwrap().len();
    let before: Vec<Intent> = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    service.shutdown().await;
    let reopened = RemoteService::new(root, store);
    let restored = reopened.restore().await;
    let after: Vec<Intent> = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    println!(
        "JOURNAL_OVERFLOW generated_bytes={bytes} admitted_records={} restore_status={:?} retained_records={}",
        before.len(),
        restored.as_ref().map(|v| v.status.as_str()),
        after.len()
    );
    reopened.shutdown().await;
    assert!(
        bytes <= 1024 * 1024 && after.len() == before.len(),
        "Native writer exceeded its own reader bound and restart erased admitted records"
    );
    assert!(
        after
            .iter()
            .any(|i| i.id == attention && i.status == "attention")
    );
}
#[tokio::test]
#[ignore = "Requires desktop/fixtures/paired_manager.py with its isolated actual manager."]
async fn missing_journal_does_not_become_an_empty_paired_profile() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("owner");
    let (service, store, _) = paired(&root).await;
    service.shutdown().await;
    let journal = root.join("intents.json");
    std::fs::remove_file(&journal).unwrap();
    let reopened = RemoteService::new(root, store);
    assert!(matches!(reopened.restore().await, Err(code) if code == "journal_unavailable"));
    assert!(
        reopened
            .command(IntentCommand::Begin {
                label: "Do not discard a missing journal".into(),
            })
            .await
            .is_err()
    );
    assert!(!journal.exists());
    assert!(reopened.inspect().workspace.is_none());
    reopened.shutdown().await;
}
#[tokio::test]
#[ignore = "Requires desktop/fixtures/paired_manager.py with its isolated actual manager."]
async fn readable_unknown_journal_blocks_mutation_without_replay() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("owner");
    let (service, store, fixture) = paired(&root).await;
    let bot: BotId = fixture["bot"].as_str().unwrap().parse().unwrap();
    let permission = service
        .inspect()
        .workspace
        .unwrap()
        .state
        .permissions
        .into_iter()
        .find(|p| p.bot == bot)
        .unwrap();
    service.shutdown().await;
    let intent = Intent {
        id: uuid::Uuid::new_v4(),
        status: "unknown".into(),
        operation: OwnerOperation::SetPermission {
            bot,
            expected_revision: permission.revision,
            allow: !permission.allow,
        },
    };
    let journal = root.join("intents.json");
    std::fs::write(&journal, serde_json::to_vec(&vec![intent]).unwrap()).unwrap();
    std::fs::set_permissions(&journal, std::fs::Permissions::from_mode(0o600)).unwrap();
    let reopened = RemoteService::new(root, store);
    let restored = reopened.restore().await.unwrap();
    assert_eq!(
        restored
            .intents
            .iter()
            .filter(|i| i.status == "unknown")
            .count(),
        1
    );
    let mutation = reopened
        .command(IntentCommand::Permission {
            bot,
            expected: permission.revision,
            allow: !permission.allow,
        })
        .await;
    assert!(matches!(mutation, Err(ref code) if code == "unresolved_intent"));
    let current = reopened
        .inspect()
        .workspace
        .unwrap()
        .state
        .permissions
        .into_iter()
        .find(|p| p.bot == bot)
        .unwrap();
    assert_eq!(current.revision, permission.revision);
    let saved: Vec<Intent> = serde_json::from_slice(&std::fs::read(&journal).unwrap()).unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].status, "unknown");
    println!(
        "PASS readable unknown journal remains durable, blocks mutations, and leaves manager permission unchanged"
    );
    reopened.shutdown().await;
}
