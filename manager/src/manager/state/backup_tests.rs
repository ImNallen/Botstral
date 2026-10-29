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
    let backup = root.join("backup");
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
    recovered
        .unlock_vault(session(&grant), passphrase())
        .unwrap();
    assert_eq!(
        recovered.provider_secret().unwrap().expose_secret(),
        "synthetic restored provider credential"
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
    let (mut file, _, length) = recovered
        .download_artifact(session(&grant), task.id, artifact)
        .unwrap();
    let mut actual = Vec::new();
    file.read_to_end(&mut actual).unwrap();
    assert_eq!(actual, bytes);
    assert_eq!(length, bytes.len() as u64);
}
