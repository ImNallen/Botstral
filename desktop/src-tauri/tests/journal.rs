use botstral_desktop::{
    local::LocalManager,
    remote::{Intent, IntentCommand, RemoteService, SendInput},
};
use botstral_link::*;
use botstral_owner_client::{ClientError, OwnerClient, SecretStore};
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
    paired_at(root, &fixture_root).await
}
async fn paired_at(
    root: &Path,
    fixture_root: &Path,
) -> (RemoteService, Arc<MemorySecrets>, serde_json::Value) {
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
    let code = local.invite_device(request.label.clone()).await.unwrap();
    let parsed = botstral_link::JoinCode::parse(&code).unwrap();
    let bundle = botstral_link::OwnerInvitation {
        trust,
        role: botstral_link::DeviceRole::Owner,
        fingerprint: request.fingerprint.clone(),
        expires_at: i64::MAX / 4,
        invitation: parsed.secret_text().to_string(),
    };
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

async fn publish_public(
    fixture_root: &Path,
    path: &str,
    bytes: &[u8],
    bearer: Option<&str>,
) -> (u16, serde_json::Value) {
    use rustls::pki_types::ServerName;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_rustls::TlsConnector;
    let trust: ManagerTrust =
        serde_json::from_slice(&std::fs::read(fixture_root.join("trust.json")).unwrap()).unwrap();
    let config = tls::pinned_manager_config(&trust).unwrap();
    let host = trust
        .endpoint
        .rsplit_once(':')
        .map(|(host, _)| host)
        .unwrap()
        .to_owned();
    let name = ServerName::try_from(host).unwrap();
    let tcp = tokio::net::TcpStream::connect(trust.endpoint.as_str())
        .await
        .unwrap();
    let mut stream = TlsConnector::from(Arc::new(config))
        .connect(name, tcp)
        .await
        .unwrap();
    let mut head = format!(
        "POST {path} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        trust.endpoint,
        bytes.len()
    );
    if let Some(token) = bearer {
        head.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await.unwrap();
    stream.write_all(bytes).await.unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    let text = String::from_utf8(response).unwrap();
    let (headers, body) = text.split_once("\r\n\r\n").unwrap();
    let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, serde_json::from_str(body).unwrap())
}

#[tokio::test]
#[ignore = "Requires desktop/fixtures/paired_manager.py with its isolated actual manager."]
async fn p06_locked_reads_preserve_the_paired_unlock_path() {
    let directory = tempfile::tempdir().unwrap();
    let (service, _, _) = paired(&directory.path().join("owner")).await;
    service.command(IntentCommand::LockVault).await.unwrap();
    assert!(service.credential_uses().await.is_err());
    assert_eq!(
        service.inspect().status,
        "online",
        "A locked Vault must preserve the paired owner connection needed to unlock it."
    );
    assert!(service.login_items().await.is_err());
    assert_eq!(service.inspect().status, "online");
    let status = service
        .protected(
            service.inspect().generation,
            protected::ProtectedOwnerRequest {
                id: RequestId::new(),
                operation: protected::ProtectedOwnerOperation::Unlock {
                    passphrase: Zeroizing::new("synthetic M04 vault passphrase".into()),
                },
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        status,
        CredentialWriteStatus::ClosedWithoutCommit { .. }
    ));
    assert!(service.login_items().await.is_ok());
    assert_eq!(service.inspect().status, "online");
    service.sign_out().await.unwrap();
}

#[tokio::test]
#[ignore = "Requires desktop/fixtures/paired_manager.py with its isolated actual manager."]
async fn p06_protected_entry_never_enters_native_journal() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("owner");
    let (service, store, fixture) = paired(&root).await;
    let bot: BotId = fixture["bot"].as_str().unwrap().parse().unwrap();
    let username = "synthetic-p06-native-user";
    let password = "synthetic-p06-native-\"\\\n-private";
    let request = RequestId::new();
    let status = service
        .protected(
            service.inspect().generation,
            protected::ProtectedOwnerRequest {
                id: request,
                operation: protected::ProtectedOwnerOperation::Write {
                    target: CredentialWriteTarget::Create {
                        metadata: LoginMetadata {
                            label: "Native fixture".into(),
                            bots: BotAccess::Selected { bots: vec![bot] },
                            allowed_origins: vec!["https://example.test".into()],
                            password_policy: PasswordPolicy::EveryFill,
                        },
                    },
                    values: protected::LoginValues {
                        username: username.into(),
                        password: password.into(),
                        totp_seed: String::new(),
                    },
                },
            },
        )
        .await
        .unwrap();
    let CredentialWriteStatus::Committed { item, .. } = status.clone() else {
        panic!("protected write did not commit")
    };
    assert_eq!(service.login_items().await.unwrap(), vec![item.clone()]);
    service.shutdown().await;
    simulate_lost_protected_reply(&root, request);
    let restarted = RemoteService::new(root.clone(), store);
    restarted.restore().await.unwrap();
    assert_eq!(restarted.protected_status().await.unwrap(), Some(status));
    assert_eq!(restarted.login_items().await.unwrap().len(), 1);
    restarted
        .command(IntentCommand::UpdateLogin {
            id: item.id,
            expected_revision: item.revision,
            metadata: None,
        })
        .await
        .unwrap();
    assert_eq!(
        restarted.login_items().await.unwrap()[0].state,
        CredentialState::Removed
    );
    fn scan(root: &Path, secrets: &[&str]) {
        for entry in std::fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                scan(&path, secrets);
            } else {
                let bytes = std::fs::read(path).unwrap();
                for secret in secrets {
                    assert!(
                        !bytes.windows(secret.len()).any(|v| v == secret.as_bytes()),
                        "private value persisted in operational files"
                    );
                    let escaped = serde_json::to_string(secret).unwrap();
                    assert!(
                        !bytes
                            .windows(escaped.len())
                            .any(|v| v == escaped.as_bytes()),
                        "escaped private value persisted in operational files"
                    );
                }
            }
        }
    }
    scan(&root, &[username, password]);
    let manager =
        PathBuf::from(std::env::var_os("BOTSTRAL_C02_FIXTURE_ROOT").unwrap()).join("manager-state");
    scan(&manager, &[username, password]);
    if let Some(evidence) = std::env::var_os("BOTSTRAL_P06_EVIDENCE") {
        std::fs::write(PathBuf::from(evidence).join("native-storage.json"), serde_json::to_vec_pretty(&serde_json::json!({"paired_protected_write":true,"metadata_only":true,"lost_reply_reconciled_without_resend":true,"removed":true,"desktop_and_manager_file_scan":"passed","native_controls":"not exercised by this service integration"})).unwrap()).unwrap();
    }
    restarted.sign_out().await.unwrap();
}

#[tokio::test]
#[ignore = "Requires desktop/fixtures/paired_manager.py with its isolated actual manager."]
async fn observed_revocation_blocks_cached_activity_after_offline_restart() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("owner");
    let (service, store, fixture) = paired(&root).await;
    assert!(service.inspect().workspace.is_some());
    let local = LocalManager::new().unwrap();
    local
        .sign_in(
            fixture["local"].as_str().unwrap().into(),
            Zeroizing::new(fixture["password"].as_str().unwrap().into()),
        )
        .await
        .unwrap();
    let device = local.devices().await.unwrap().remove(0);
    local.revoke_device(device.id).await.unwrap();
    assert!(service.restore().await.is_err());
    assert!(service.inspect().workspace.is_none());
    service.shutdown().await;
    let mut client = OwnerClient::open_with(&root, store.clone()).unwrap();
    let mut trust = client.trust().unwrap().clone();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    trust.endpoint = listener.local_addr().unwrap().to_string();
    drop(listener);
    client.import_trust(trust).unwrap();
    drop(client);
    let restarted = RemoteService::new(root.clone(), store);
    let result = restarted.restore().await;
    println!(
        "REVOKED_OFFLINE_RESTART result={:?} cached_activity={}",
        result.as_ref().map(|v| v.status.as_str()),
        restarted.inspect().workspace.is_some()
    );
    assert!(
        restarted.inspect().workspace.is_none(),
        "A known-revoked profile exposed cached activity after offline restart"
    );
    assert_eq!(restarted.inspect().issue.as_deref(), Some("revoked"));
    assert!(root.join("revoked.json").exists());
    let request = restarted
        .command(IntentCommand::Begin {
            label: "Explicit replacement after revocation".into(),
        })
        .await
        .unwrap()
        .pairing
        .unwrap();
    assert_ne!(request.fingerprint, device.fingerprint);
    assert!(!root.join("revoked.json").exists());
    let trust = serde_json::from_slice(
        &std::fs::read(
            PathBuf::from(std::env::var_os("BOTSTRAL_C02_FIXTURE_ROOT").unwrap())
                .join("trust.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let code = local.invite_device(request.label.clone()).await.unwrap();
    let parsed = botstral_link::JoinCode::parse(&code).unwrap();
    let bundle = botstral_link::OwnerInvitation {
        trust,
        role: botstral_link::DeviceRole::Owner,
        fingerprint: request.fingerprint,
        expires_at: i64::MAX / 4,
        invitation: parsed.secret_text().to_string(),
    };
    let candidate = restarted
        .command(IntentCommand::Stage(bundle))
        .await
        .unwrap()
        .candidate
        .unwrap();
    assert_eq!(
        restarted
            .command(IntentCommand::Confirm(candidate.id))
            .await
            .unwrap()
            .status,
        "online"
    );
    restarted.sign_out().await.unwrap();
    local.sign_out();
}

#[tokio::test]
#[ignore = "Requires desktop/fixtures/paired_manager.py with its isolated actual manager."]
async fn historical_approval_cannot_confirm_an_unknown_regrant() {
    let fixture_root = PathBuf::from(std::env::var_os("BOTSTRAL_C02_FIXTURE_ROOT").unwrap());
    let pause = fixture_root.join("pause-worker");
    std::fs::write(&pause, b"").unwrap();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("owner");
    let (service, store, fixture) = paired(&root).await;
    let bot = fixture["bot"].as_str().unwrap().parse().unwrap();
    service
        .command(IntentCommand::Send(SendInput {
            bot,
            session: Some(fixture["session"].as_str().unwrap().parse().unwrap()),
            text: "Create a file".into(),
            account: ProcessAccount {
                uid: 1000,
                gid: 1000,
            },
            runtime: Some(
                fixture["runtime"]
                    .as_str()
                    .unwrap()
                    .to_owned()
                    .try_into()
                    .unwrap(),
            ),
            process: true,
            seconds: 120,
            turns: 4,
            tokens: 200_000,
            cost_microusd: 200_000,
            output_tokens: 1024,
        }))
        .await
        .unwrap();
    let mut changes = service.observe();
    let task = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if let Some(task) = changes
                .borrow()
                .workspace
                .as_ref()
                .and_then(|w| w.state.tasks.first())
                .cloned()
            {
                break task;
            }
            changes.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    let review = service
        .command(IntentCommand::Review(task.id))
        .await
        .unwrap()
        .review
        .unwrap();
    service
        .command(IntentCommand::Approve(review.id))
        .await
        .unwrap();
    let prior = service
        .inspect()
        .workspace
        .unwrap()
        .state
        .tasks
        .into_iter()
        .find(|t| t.id == task.id)
        .unwrap();
    assert!(!prior.approval.consumed);
    service
        .command(IntentCommand::RevokeApproval(task.id))
        .await
        .unwrap();
    let revoked = service
        .inspect()
        .workspace
        .unwrap()
        .state
        .tasks
        .into_iter()
        .find(|t| t.id == task.id)
        .unwrap();
    assert!(revoked.approval.revoked);
    let expires_at = prior.approval.binding.as_ref().unwrap().expires_at + 1;
    let intent = Intent {
        id: uuid::Uuid::new_v4(),
        status: "unknown".into(),
        operation: OwnerOperation::GrantReviewedApproval {
            id: task.id,
            command: Box::new(revoked.command),
            permission_revision: revoked.approval.permission_revision,
            approval: revoked.approval,
            expires_at,
        },
    };
    service.shutdown().await;
    std::fs::write(
        root.join("intents.json"),
        serde_json::to_vec(&vec![intent]).unwrap(),
    )
    .unwrap();
    let restored = RemoteService::new(root, store);
    let current = restored.restore().await.unwrap();
    println!(
        "UNKNOWN_REGRANT historical_revoked_status={}",
        current.intents[0].status
    );
    assert_eq!(
        current.intents[0].status, "unknown",
        "A historical revoked binding falsely confirmed an uncommitted regrant"
    );
    let http = reqwest::Client::new();
    let endpoint = format!("http://{}", fixture["local"].as_str().unwrap());
    let auth: serde_json::Value = http
        .post(format!("{endpoint}/v1/sessions"))
        .json(&serde_json::json!({"password":fixture["password"]}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let token = auth["token"].as_str().unwrap();
    for (expiry, expected) in [(expires_at - 1, "unknown"), (expires_at, "confirmed")] {
        http.put(format!("{endpoint}/v1/tasks/{}/approval", task.id))
            .bearer_auth(token)
            .json(&serde_json::json!({"expires_at":expiry}))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();
        let current = restored.restore().await.unwrap();
        assert_eq!(current.intents[0].status, expected);
    }
    restored.sign_out().await.unwrap();
    std::fs::remove_file(pause).unwrap();
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
                session: Some(session),
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

#[path = "journal/p03.rs"]
mod p03;

#[path = "journal/history.rs"]
mod history;

#[tokio::test]
#[ignore = "Requires desktop/fixtures/paired_manager.py with its isolated actual manager."]
async fn trimmed_run_history_does_not_strand_unknown_intents() {
    history::verify().await;
}

#[tokio::test]
#[ignore = "Requires desktop/fixtures/paired_manager.py with its isolated actual manager."]
async fn p03_routine_controls_and_crash_journal_recovery() {
    p03::verify().await;
}

#[path = "journal/conversational_memory.rs"]
mod conversational_memory;

#[tokio::test]
#[ignore = "Requires desktop/fixtures/paired_manager.py --verify-memory."]
async fn conversational_memory_and_obsolete_journal() {
    conversational_memory::verify().await;
}

fn simulate_lost_protected_reply(root: &Path, request: RequestId) {
    let path = root.join("protected-status.json");
    let mut record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    record["status"] = serde_json::to_value(CredentialWriteStatus::Unresolved { request }).unwrap();
    std::fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
}

#[tokio::test]
#[ignore = "Requires two owned synthetic manager fixtures from p06-manager-switch.py."]
async fn p06_native_receipt_manager_binding() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("owner");
    let (service, _, _) = paired(&root).await;
    let request = RequestId::new();
    let make_request = |id| protected::ProtectedOwnerRequest {
        id,
        operation: protected::ProtectedOwnerOperation::Write {
            target: CredentialWriteTarget::Create {
                metadata: LoginMetadata {
                    label: "Authority bound receipt".into(),
                    bots: BotAccess::AllOwnerBots,
                    allowed_origins: vec!["https://example.test".into()],
                    password_policy: PasswordPolicy::EveryFill,
                },
            },
            values: protected::LoginValues {
                username: uuid::Uuid::new_v4().to_string(),
                password: uuid::Uuid::new_v4().to_string(),
                totp_seed: String::new(),
            },
        },
    };
    let stale = service
        .protected(
            service.inspect().generation + 1,
            make_request(RequestId::new()),
        )
        .await;
    assert!(matches!(stale, Err(ref e) if e == "stale"));
    assert!(service.login_items().await.unwrap().is_empty());
    let committed = service
        .protected(service.inspect().generation, make_request(request))
        .await
        .unwrap();
    assert!(matches!(committed, CredentialWriteStatus::Committed { .. }));
    service.shutdown().await;
    simulate_lost_protected_reply(&root, request);
    let original = std::fs::read(root.join("protected-status.json")).unwrap();
    service.sign_out().await.unwrap();
    let other_root = PathBuf::from(std::env::var_os("BOTSTRAL_REVIEW_OTHER_MANAGER").unwrap());
    let (other, _, _) = paired_at(&root, &other_root).await;
    assert!(other.login_items().await.unwrap().is_empty());
    assert!(
        matches!(other.protected_status().await, Err(ref e) if e == "receipt_manager_mismatch")
    );
    assert_eq!(
        std::fs::read(root.join("protected-status.json")).unwrap(),
        original
    );
    let refused = other
        .protected(other.inspect().generation, make_request(RequestId::new()))
        .await;
    assert!(matches!(refused, Err(ref e) if e == "receipt_manager_mismatch"));
    assert_eq!(
        std::fs::read(root.join("protected-status.json")).unwrap(),
        original
    );
    other.sign_out().await.unwrap();
    let (original_manager, _, _) = paired(&root).await;
    assert_eq!(
        original_manager.protected_status().await.unwrap(),
        Some(committed)
    );
    original_manager.sign_out().await.unwrap();
}

#[tokio::test]
#[ignore = "Requires desktop/fixtures/paired_manager.py with its isolated actual manager."]
async fn p06_collision_receipt_is_final_and_value_free() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("owner");
    let (service, _, _) = paired(&root).await;
    let request = RequestId::new();
    let password = uuid::Uuid::new_v4().to_string();
    let make = |id, username: String| protected::ProtectedOwnerRequest {
        id,
        operation: protected::ProtectedOwnerOperation::Write {
            target: CredentialWriteTarget::Create {
                metadata: LoginMetadata {
                    label: "Native collision refusal".into(),
                    bots: BotAccess::AllOwnerBots,
                    allowed_origins: vec!["https://example.test".into()],
                    password_policy: PasswordPolicy::EveryFill,
                },
            },
            values: protected::LoginValues {
                username,
                password: password.clone(),
                totp_seed: String::new(),
            },
        },
    };
    let refused = service
        .protected(service.inspect().generation, make(request, "admin".into()))
        .await
        .unwrap();
    assert_eq!(
        refused,
        CredentialWriteStatus::Rejected {
            request,
            reason: CredentialWriteRejection::PublicValueCollision
        }
    );
    assert_eq!(service.protected_status().await.unwrap(), Some(refused));
    assert!(service.login_items().await.unwrap().is_empty());
    let receipt = std::fs::read_to_string(root.join("protected-status.json")).unwrap();
    assert!(!receipt.contains("admin") && !receipt.contains(&password));
    assert!(matches!(
        service
            .protected(
                service.inspect().generation,
                make(RequestId::new(), uuid::Uuid::new_v4().to_string())
            )
            .await
            .unwrap(),
        CredentialWriteStatus::Committed { .. }
    ));
    service.sign_out().await.unwrap();
}

#[tokio::test]
#[ignore = "Requires the isolated paired manager fixture."]
async fn p05_chooser_rejects_acknowledged_same_generation_revocation() {
    use botstral_desktop::remote::VerifiedArtifact;
    let directory = tempfile::tempdir().unwrap();
    let (service, _, fixture) = paired(&directory.path().join("owner")).await;
    let scope = service.owner_scope().unwrap();
    let generation = scope.generation;
    let local = LocalManager::new().unwrap();
    local
        .sign_in(
            fixture["local"].as_str().unwrap().into(),
            Zeroizing::new(fixture["password"].as_str().unwrap().into()),
        )
        .await
        .unwrap();
    let device = local
        .devices()
        .await
        .unwrap()
        .into_iter()
        .find(|d| d.label == "Independent review")
        .unwrap();
    local.revoke_device(device.id).await.unwrap();
    assert!(service.restore().await.is_err());
    assert_eq!(service.inspect().issue.as_deref(), Some("revoked"));
    assert_eq!(service.inspect().generation, generation);
    let destination = directory.path().join("revoked.txt");
    let saved = service
        .save_export(
            scope,
            TaskId::new(),
            &destination,
            VerifiedArtifact {
                name: "revoked.txt".into(),
                bytes: b"must not save".to_vec(),
            },
        )
        .await;
    assert!(saved.is_err());
    service.shutdown().await;
    assert!(
        !destination.exists(),
        "Acknowledged revocation during chooser must prevent export without a generation change"
    );
}

#[path = "journal/attachments.rs"]
mod attachments;
#[path = "journal/saved_results.rs"]
mod saved_results;
#[tokio::test]
#[ignore = "Requires the isolated paired manager fixture with recorded requests and restart support."]
async fn p05_attachments_restart_and_export() {
    attachments::verify().await;
}

#[tokio::test]
#[ignore = "Requires the isolated paired manager fixture with restart support."]
async fn p05_saved_results_restart_and_export() {
    saved_results::verify().await;
}

#[tokio::test]
#[ignore = "Requires the isolated paired manager fixture."]
async fn p05_export_reauthenticates_after_chooser() {
    use botstral_desktop::remote::VerifiedArtifact;
    let directory = tempfile::tempdir().unwrap();
    let (service, _, fixture) = paired(&directory.path().join("owner")).await;
    let scope = service.owner_scope().unwrap();
    let local = LocalManager::new().unwrap();
    local
        .sign_in(
            fixture["local"].as_str().unwrap().into(),
            Zeroizing::new(fixture["password"].as_str().unwrap().into()),
        )
        .await
        .unwrap();
    let device = local
        .devices()
        .await
        .unwrap()
        .into_iter()
        .find(|d| !d.revoked && d.label == "Independent review")
        .unwrap();
    local.revoke_device(device.id).await.unwrap();
    let destination = directory.path().join("revoked-before-read.txt");
    let saved = service
        .save_export(
            scope,
            TaskId::new(),
            &destination,
            VerifiedArtifact {
                name: "revoked.txt".into(),
                bytes: b"must not save".to_vec(),
            },
        )
        .await;
    assert!(saved.is_err());
    assert_eq!(service.inspect().generation, scope.generation);
    assert_ne!(service.inspect().status, "online");
    assert!(service.restore().await.is_err());
    assert_eq!(service.inspect().issue.as_deref(), Some("revoked"));
    assert!(!destination.exists());
    service.shutdown().await;
    local.sign_out();
}

#[tokio::test]
#[ignore = "Requires two owned local managers from p05-manager-switch.py."]
async fn p05_export_manager_switch() {
    use botstral_desktop::remote::VerifiedArtifact;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("owner");
    let (first, _, _) = paired(&root).await;
    let scope = first.owner_scope().unwrap();
    first.sign_out().await.unwrap();
    let other_root = PathBuf::from(std::env::var_os("BOTSTRAL_P05_OTHER_MANAGER").unwrap());
    let (second, store, fixture) = paired_at(&root, &other_root).await;
    let current = second.owner_scope().unwrap();
    assert_eq!(current.generation, scope.generation);
    assert_ne!(current.manager, scope.manager);
    second.shutdown().await;
    let mut client = OwnerClient::open_with(&root, store.clone()).unwrap();
    let mut session = client.connect().await.unwrap();
    let id = TaskId::new();
    session
        .call(OwnerOperation::SubmitTask {
            input: SubmitTask {
                id,
                session_id: fixture["session"].as_str().unwrap().parse().unwrap(),
                conversation_id: None,
                settings: TaskSettings {
                    model: "P05 authority fixture".into(),
                    max_turns: 1,
                    max_tokens: 64,
                    deadline: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs() as i64
                        + 600,
                    max_cost_micros: 0,
                },
                work: WorkSpec::Unavailable,
            },
        })
        .await
        .unwrap();
    drop(session);
    drop(client);
    let second = RemoteService::new(root, store);
    second.restore().await.unwrap();
    second.source_task(id).await.unwrap();
    let destination = directory.path().join("wrong-manager.txt");
    let result = second
        .save_export(
            scope,
            id,
            &destination,
            VerifiedArtifact {
                name: "old-manager.txt".into(),
                bytes: b"previously verified chooser bytes".to_vec(),
            },
        )
        .await;
    assert_eq!(result, Err("stale".into()));
    assert!(!destination.exists());
    second.sign_out().await.unwrap();
    println!(
        "PASS P05 native export refuses old-manager chooser bytes after actual re-pairing at the same local generation, even with a readable source task on the new manager."
    );
}
