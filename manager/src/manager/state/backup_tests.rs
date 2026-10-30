use super::*;
use age::secrecy::ExposeSecret;
use botstral_link::*;
use std::{io::Read, os::unix::fs::PermissionsExt, process::Command};

fn login() -> LoginPassword {
    LoginPassword::from_secret("synthetic recovery login password".into()).unwrap()
}
fn passphrase() -> VaultPassphrase {
    VaultPassphrase::from_secret("synthetic recovery vault passphrase".into()).unwrap()
}
fn session(grant: &SessionGrant) -> SessionToken {
    SessionToken::new(grant.token.0.expose_secret().to_owned()).unwrap()
}
fn maintenance(args: &[&std::ffi::OsStr]) -> std::process::Output {
    Command::new("python3")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/packaging/backup.py"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn offline_restore_reopens_owner_history_markdown_credentials_and_artifacts() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().canonicalize().unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    recovery_fixture(root, false);
}

#[test]
#[ignore = "Seeds real state for native release acceptance."]
fn release_seed_fixture() {
    let root = PathBuf::from(std::env::var_os("BOTSTRAL_RELEASE_FIXTURE").unwrap());
    recovery_fixture(root, true);
}

fn recovery_fixture(root: PathBuf, export: bool) {
    let source = root.join("source");
    let local = crate::LocalState::open(&source).unwrap();
    let (configured, _) = watch::channel(false);
    let mut state = State::open(source.clone(), configured).unwrap();
    let proof =
        BootstrapToken::new(fs::read_to_string(source.join("bootstrap.token")).unwrap()).unwrap();
    state
        .create_owner(
            proof,
            OwnerSetup {
                name: OwnerName::new("Recovery owner".into()).unwrap(),
                login_password: login(),
                vault_passphrase: passphrase(),
                recovery: RecoveryAcknowledgment::OfflinePassphraseCopy,
            },
        )
        .unwrap();
    let grant = state.sign_in(login()).unwrap();
    state.unlock_vault(session(&grant), passphrase()).unwrap();
    state
        .replace_provider_key(
            session(&grant),
            ProviderSecret::from_secret("synthetic restored provider credential".into()).unwrap(),
        )
        .unwrap();
    let bot = BotId::new();
    state
        .create_bot(
            session(&grant),
            NewBot {
                id: bot,
                name: BotName::new("Recovery bot".into()).unwrap(),
            },
        )
        .unwrap();
    let conversation = Conversation {
        id: ConversationId::new(),
        bot,
    };
    state
        .conversation(session(&grant), conversation.clone())
        .unwrap();
    let message = ConversationMessage {
        id: MessageId::new(),
        conversation: conversation.id,
        text: "Owner history survives a machine replacement.".into(),
    };
    state
        .append_message(session(&grant), message.clone())
        .unwrap();
    let document = Document {
        bot,
        kind: DocumentKind::Memory,
        revision: 3,
        sources: vec![SourceReference::Message { id: message.id }],
        markdown: "Remember the owner's recovery preference.".into(),
    };
    let mut metadata = document.clone();
    metadata.markdown.clear();
    let memory_path = source.join(format!("memory-{bot}.md"));
    fs::write(
        &memory_path,
        format!(
            "<!-- botstral-document {} -->\n{}",
            serde_json::to_string(&metadata).unwrap(),
            document.markdown
        ),
    )
    .unwrap();
    fs::set_permissions(&memory_path, fs::Permissions::from_mode(0o600)).unwrap();
    let entries = state.memory_context(bot).unwrap();
    let peer = crate::worker_http::VerifiedIdentity::for_test(b"recovery worker");
    let invitation = state
        .invite_worker(session(&grant), peer.fingerprint().clone(), 600)
        .unwrap();
    let worker = state.enroll_worker(peer, invitation.secret).unwrap();
    let computer = state
        .open_computer_session(session(&grant), bot, worker)
        .unwrap();
    let mut task = state
        .submit_task(
            session(&grant),
            SubmitTask {
                id: TaskId::new(),
                session_id: computer.id,
                conversation_id: Some(conversation.id),
                settings: TaskSettings {
                    model: "unavailable".into(),
                    max_turns: 1,
                    max_tokens: 16,
                    deadline: state.now() + 600,
                    max_cost_micros: 0,
                },
                work: WorkSpec::Unavailable,
            },
        )
        .unwrap();
    let artifact = ArtifactId::new();
    let bytes = b"Retained durable artifact bytes.";
    let content = ArtifactContent {
        length: bytes.len() as u64,
        sha256: CertificateFingerprint::from_der(bytes),
    };
    let declaration = OutputDeclaration {
        slot: "recovery".into(),
        name: "recovered.txt".into(),
        media_type: MediaType::TextPlain,
        max_bytes: 1024,
    };
    task.state = TaskState::Completed;
    task.outputs.push(OutputView {
        declaration: declaration.clone(),
        state: ArtifactState::Available {
            id: artifact,
            content: content.clone(),
        },
    });
    tasks::save(&state.connection, &task).unwrap();
    let offer = ArtifactOffer {
        journal: JournalId::new(),
        task: task.id,
        execution: ExecutionId::new(),
        slot: declaration.slot.clone(),
        output: SealedOutput::Content { content },
    };
    state
        .connection
        .execute(
            "INSERT INTO artifacts VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?7,'available')",
            params![
                artifact.to_string(),
                task.id.to_string(),
                worker.to_string(),
                offer.journal.to_string(),
                offer.execution.to_string(),
                declaration.slot,
                bytes.len() as i64,
                serde_json::to_string(&offer).unwrap()
            ],
        )
        .unwrap();
    fs::create_dir(source.join("artifacts")).unwrap();
    fs::set_permissions(source.join("artifacts"), fs::Permissions::from_mode(0o700)).unwrap();
    let blob = source.join("artifacts").join(format!("{artifact}.blob"));
    fs::write(&blob, bytes).unwrap();
    fs::set_permissions(blob, fs::Permissions::from_mode(0o600)).unwrap();
    for _ in 0..34 {
        let mut newer = task.clone();
        newer.id = TaskId::new();
        newer.request.id = newer.id;
        newer.command.task = newer.id;
        newer.command.id = CommandId::new();
        newer.outputs.clear();
        tasks::insert_task(&state.connection, &newer).unwrap();
    }
    let EventReplay::Snapshot { state: recent, .. } = state.resume(session(&grant), 0, 64).unwrap()
    else {
        panic!("Expected snapshot")
    };
    assert!(!recent.tasks.iter().any(|t| t.id == task.id));
    let query = ArtifactQuery {
        bot: None,
        name: "RECOVER".into(),
        after: None,
        limit: ARTIFACT_PAGE_SIZE as u16,
    };
    let page = state.artifact_page(session(&grant), query.clone()).unwrap();
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].task, task.id);
    assert_eq!(state.task(session(&grant), task.id).unwrap().id, task.id);
    let backup = root.join("backup");
    let login_request = RequestId::new();
    let CredentialWriteStatus::Committed { item, .. } = state
        .write_login(
            session(&grant),
            login_request,
            CredentialWriteTarget::Create {
                metadata: LoginMetadata {
                    label: "Recovery login".into(),
                    bots: BotAccess::Selected { bots: vec![bot] },
                    allowed_origins: vec!["https://example.test".into()],
                    password_policy: PasswordPolicy::EveryFill,
                },
            },
            protected::LoginValues {
                username: "synthetic recovery private username".into(),
                password: "synthetic recovery private password".into(),
                totp_seed: String::new(),
            },
        )
        .unwrap()
    else {
        panic!("committed login required")
    };
    state
        .update_login(session(&grant), item.id, item.revision, None)
        .unwrap();
    if export {
        let mut uncertain = task.clone();
        uncertain.id = TaskId::new();
        uncertain.request.id = uncertain.id;
        uncertain.command.task = uncertain.id;
        uncertain.command.id = CommandId::new();
        uncertain.outputs.clear();
        uncertain.state = TaskState::AwaitingAttention;
        tasks::insert_task(&state.connection, &uncertain).unwrap();
        drop(state);
        drop(local);
        return;
    }
    let command = [
        "create".as_ref(),
        "--state-dir".as_ref(),
        source.as_os_str(),
        "--destination".as_ref(),
        backup.as_os_str(),
    ];
    assert!(
        !maintenance(&command).status.success(),
        "running manager lock must refuse backup"
    );
    drop(state);
    drop(local);
    for entry in fs::read_dir(&source).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        }
    }
    let output = maintenance(&command);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let restored = root.join("restored");
    let output = maintenance(&[
        "restore".as_ref(),
        "--backup".as_ref(),
        backup.as_os_str(),
        "--destination".as_ref(),
        restored.as_os_str(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _local = crate::LocalState::open(&restored).unwrap();
    let (configured, _) = watch::channel(false);
    let mut recovered = State::open(restored, configured).unwrap();
    assert_eq!(
        recovered.owner(session(&grant)),
        Err(OperationError::Unauthorized)
    );
    let grant = recovered.sign_in(login()).unwrap();
    assert_eq!(
        recovered.connection_status(session(&grant)).unwrap().vault,
        VaultStatus::Locked
    );
    assert!(recovered.provider_secret().is_err());
    assert!(recovered.known_login_secrets().is_err());
    recovered
        .unlock_vault(session(&grant), passphrase())
        .unwrap();
    assert_eq!(
        recovered.provider_secret().unwrap().expose_secret(),
        "synthetic restored provider credential"
    );
    assert_eq!(
        recovered.load_login(item.id).unwrap().state,
        CredentialState::Removed
    );
    assert!(matches!(
        recovered
            .credential_write_status(session(&grant), login_request, false)
            .unwrap(),
        CredentialWriteStatus::Committed { .. }
    ));
    assert!(
        recovered
            .reject_known_secrets(&"synthetic recovery private password")
            .is_err()
    );
    let EventReplay::Snapshot {
        state: snapshot, ..
    } = recovered.resume(session(&grant), 0, 64).unwrap()
    else {
        panic!("owner snapshot required")
    };
    assert_eq!(snapshot.messages, vec![message]);
    assert_eq!(recovered.memory_context(bot).unwrap(), entries);
    assert!(
        recovered
            .directory
            .join(format!("legacy-memory-{bot}.md"))
            .exists()
    );
    let page = recovered.artifact_page(session(&grant), query).unwrap();
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].id, artifact);
    assert_eq!(
        recovered
            .task(session(&grant), page.entries[0].task)
            .unwrap()
            .id,
        task.id
    );
    let (mut file, _, length) = recovered
        .download_artifact(session(&grant), task.id, artifact)
        .unwrap();
    let mut actual = Vec::new();
    file.read_to_end(&mut actual).unwrap();
    assert_eq!(actual, bytes);
    assert_eq!(length, bytes.len() as u64);

    let other_bot = BotId::new();
    recovered
        .create_bot(
            session(&grant),
            NewBot {
                id: other_bot,
                name: BotName::new("Other saved results".into()).unwrap(),
            },
        )
        .unwrap();
    for i in 1..MAX_ARTIFACT_RECORDS {
        let mut saved = task.clone();
        saved.id = TaskId::new();
        saved.request.id = saved.id;
        saved.command.task = saved.id;
        saved.command.id = CommandId::new();
        saved.command.bot = if i % 2 == 0 { bot } else { other_bot };
        let id = ArtifactId::new();
        let output = &mut saved.outputs[0];
        output.declaration.name = if i == 1 {
            "literal %_*.txt".into()
        } else {
            "Duplicate.txt".into()
        };
        output.state = if i == 2 {
            ArtifactState::Unavailable {
                reason: ArtifactUnavailable::Missing,
            }
        } else if i == 3 {
            ArtifactState::Transferring {
                id,
                content: ArtifactContent {
                    length: 1,
                    sha256: CertificateFingerprint::from_der(b"x"),
                },
            }
        } else {
            ArtifactState::Available {
                id,
                content: ArtifactContent {
                    length: 1,
                    sha256: CertificateFingerprint::from_der(b"x"),
                },
            }
        };
        let slot = output.declaration.slot.clone();
        tasks::insert_task(&recovered.connection, &saved).unwrap();
        recovered
            .connection
            .execute(
                "INSERT INTO artifacts VALUES(?1,?2,?3,?4,?5,?6,1,?7,1,'available')",
                params![
                    id.to_string(),
                    saved.id.to_string(),
                    worker.to_string(),
                    JournalId::new().to_string(),
                    ExecutionId::new().to_string(),
                    slot,
                    serde_json::to_string(&ArtifactOffer {
                        task: saved.id,
                        slot: slot.clone(),
                        ..offer.clone()
                    })
                    .unwrap()
                ],
            )
            .unwrap();
    }
    let all = ArtifactQuery {
        bot: None,
        name: String::new(),
        after: None,
        limit: ARTIFACT_PAGE_SIZE as u16,
    };
    let mut query = all.clone();
    let mut entries = Vec::new();
    loop {
        let page = recovered
            .artifact_page(session(&grant), query.clone())
            .unwrap();
        assert!(page.entries.len() <= ARTIFACT_PAGE_SIZE);
        let envelope = OwnerReply::Artifacts(page.clone());
        assert!(serde_json::to_vec(&envelope).unwrap().len() < ARTIFACT_PAGE_SERIALIZED_BOUND);
        entries.extend(page.entries);
        let Some(next) = page.next else { break };
        assert!(next.len() <= MAX_ARTIFACT_CURSOR_BYTES);
        query.after = Some(next);
    }
    assert_eq!(entries.len(), MAX_ARTIFACT_RECORDS);
    assert!(entries.windows(2).all(|w| (
        w[0].output.declaration.name.to_ascii_lowercase(),
        w[0].id.0
    ) < (
        w[1].output.declaration.name.to_ascii_lowercase(),
        w[1].id.0
    )));
    assert_eq!(entries.iter().filter(|e| e.bot == other_bot).count(), 256);
    let literal = recovered
        .artifact_page(
            session(&grant),
            ArtifactQuery {
                name: "%_*".into(),
                ..all.clone()
            },
        )
        .unwrap();
    assert_eq!(literal.entries.len(), 1);
    let filtered = recovered
        .artifact_page(
            session(&grant),
            ArtifactQuery {
                bot: Some(other_bot),
                ..all.clone()
            },
        )
        .unwrap();
    assert!(filtered.entries.iter().all(|e| e.bot == other_bot));
    for invalid in [
        ArtifactQuery {
            limit: 0,
            ..all.clone()
        },
        ArtifactQuery {
            limit: 33,
            ..all.clone()
        },
        ArtifactQuery {
            name: "a".repeat(81),
            ..all.clone()
        },
        ArtifactQuery {
            name: "é".into(),
            ..all.clone()
        },
        ArtifactQuery {
            after: Some("x".repeat(1025)),
            ..all.clone()
        },
        ArtifactQuery {
            after: Some("{}".into()),
            ..all.clone()
        },
        ArtifactQuery {
            name: "changed".into(),
            after: query.after.clone(),
            ..all.clone()
        },
        ArtifactQuery {
            bot: Some(other_bot),
            after: query.after.clone(),
            ..all.clone()
        },
    ] {
        assert_eq!(
            recovered.artifact_page(session(&grant), invalid),
            Err(OperationError::InvalidInput)
        );
    }
    let unauthorized = SessionToken::new("x".repeat(43)).unwrap();
    assert_eq!(
        recovered.artifact_page(unauthorized, ArtifactQuery { limit: 0, ..all }),
        Err(OperationError::Unauthorized)
    );

    recovered
        .routine_command(
            session(&grant),
            RoutineCommand {
                id: RequestId::new(),
                action: RoutineAction::DeleteBot { bot: other_bot },
            },
        )
        .unwrap();
    let deleted = recovered
        .artifact_page(
            session(&grant),
            ArtifactQuery {
                bot: Some(other_bot),
                name: String::new(),
                after: None,
                limit: 32,
            },
        )
        .unwrap();
    assert_eq!(deleted.entries.len(), 32);
    assert!(deleted.entries.iter().all(|e| e.bot == other_bot));
    assert_eq!(
        recovered
            .task(session(&grant), deleted.entries[0].task)
            .unwrap()
            .command
            .bot,
        other_bot
    );
    let entry = &entries[0];
    let mut changed = recovered.load_task(entry.task).unwrap();
    changed.outputs[0].state = ArtifactState::Pending;
    tasks::save(&recovered.connection, &changed).unwrap();
    let pending = recovered
        .artifact_page(
            session(&grant),
            ArtifactQuery {
                name: "duplicate".into(),
                after: None,
                ..query.clone()
            },
        )
        .unwrap();
    assert!(!pending.entries.iter().any(|e| e.id == entry.id));
    changed.outputs[0].state = ArtifactState::Available {
        id: ArtifactId::new(),
        content: ArtifactContent {
            length: 1,
            sha256: CertificateFingerprint::from_der(b"x"),
        },
    };
    tasks::save(&recovered.connection, &changed).unwrap();
    let mismatch = recovered
        .artifact_page(
            session(&grant),
            ArtifactQuery {
                name: String::new(),
                after: None,
                ..query.clone()
            },
        )
        .unwrap();
    assert!(!mismatch.entries.iter().any(|e| e.id == entry.id));
    changed.command.owner = OwnerId::new();
    tasks::save(&recovered.connection, &changed).unwrap();
    assert_eq!(
        recovered.task(session(&grant), changed.id),
        Err(OperationError::Unauthorized)
    );
    let blob = recovered
        .directory
        .join("artifacts")
        .join(format!("{artifact}.blob"));
    for case in ["missing", "corrupt", "symlink"] {
        task.outputs[0].state = ArtifactState::Available {
            id: artifact,
            content: ArtifactContent {
                length: bytes.len() as u64,
                sha256: CertificateFingerprint::from_der(bytes),
            },
        };
        tasks::save(&recovered.connection, &task).unwrap();
        let _ = fs::remove_file(&blob);
        if case == "corrupt" {
            fs::write(&blob, b"wrong bytes").unwrap();
        }
        if case == "symlink" {
            std::os::unix::fs::symlink(root.join("source"), &blob).unwrap();
        }
        assert!(
            recovered
                .download_artifact(session(&grant), task.id, artifact)
                .is_err(),
            "{case}"
        );
        let page = recovered
            .artifact_page(
                session(&grant),
                ArtifactQuery {
                    name: "recovered".into(),
                    after: None,
                    ..query.clone()
                },
            )
            .unwrap();
        assert!(matches!(
            page.entries[0].output.state,
            ArtifactState::Unavailable {
                reason: ArtifactUnavailable::Corrupt
            }
        ));
    }
}

#[test]
#[ignore = "Reopens state after native package replacement and proves credential use."]
fn release_verify_fixture() {
    let root = PathBuf::from(std::env::var_os("BOTSTRAL_RELEASE_FIXTURE").unwrap());
    let _local = crate::LocalState::open(&root.join("source")).unwrap();
    let (configured, _) = watch::channel(false);
    let mut state = State::open(root.join("source"), configured).unwrap();
    let grant = state.sign_in(login()).unwrap();
    assert_eq!(
        state.connection_status(session(&grant)).unwrap().vault,
        VaultStatus::Locked
    );
    assert!(state.provider_secret().is_err());
    state.unlock_vault(session(&grant), passphrase()).unwrap();
    assert_eq!(
        state.provider_secret().unwrap().expose_secret(),
        "synthetic restored provider credential"
    );
    assert!(
        state
            .reject_known_secrets(&"synthetic recovery private password")
            .is_err()
    );
    println!("Retained encrypted credentials decrypt only after explicit unlock.");
}
