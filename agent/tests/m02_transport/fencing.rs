use super::*;

#[tokio::test]
async fn actual_daemon_rejects_stale_frames_and_queued_receipts() {
    use std::sync::Arc;
    let mut f = Fixture::new().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
        Arc::new(tls::roots(&f.worker.manager_ca).unwrap()),
        provider.clone(),
    )
    .build()
    .unwrap();
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_client_cert_verifier(verifier)
        .with_single_cert(
            tls::certificates(&f.root.path().join("server.pem")).unwrap(),
            tls::private_key(&f.root.path().join("server.key")).unwrap(),
        )
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let state = f.root.path().join("probe-agent");
    fs::create_dir(&state).unwrap();
    fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
    let mut client = f.worker.clone();
    client.manager = format!("wss://localhost:{}", listener.local_addr().unwrap().port());
    private(
        &state.join("config.json"),
        &serde_json::to_vec(&client).unwrap(),
    );
    let mut daemon = launch(
        Path::new(env!("CARGO_BIN_EXE_botstral-test-agent")),
        &["run", "--state-dir", state.to_str().unwrap()],
        f.root.path(),
        "probe-agent",
    );
    let stream = tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .unwrap()
        .unwrap()
        .0;
    let stream = acceptor.accept(stream).await.unwrap();
    let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
    async fn send_probe(
        socket: &mut WebSocketStream<tokio_rustls::server::TlsStream<TcpStream>>,
        message: ManagerMessage,
    ) {
        socket
            .send(Message::Text(
                serde_json::to_string(&Envelope::new(message))
                    .unwrap()
                    .into(),
            ))
            .await
            .unwrap();
    }
    async fn report(
        socket: &mut WebSocketStream<tokio_rustls::server::TlsStream<TcpStream>>,
    ) -> Envelope<WorkerMessage> {
        loop {
            let Message::Text(text) = tokio::time::timeout(Duration::from_secs(5), socket.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap()
            else {
                panic!("text report")
            };
            let envelope = decode::<WorkerMessage>(text.as_bytes()).unwrap();
            if !matches!(envelope.message, WorkerMessage::Heartbeat { .. }) {
                return envelope;
            }
        }
    }
    assert!(matches!(
        report(&mut socket).await.message,
        WorkerMessage::Hello(_)
    ));
    let generation = Generation::INITIAL.advance().unwrap();
    let worker = WorkerId::new();
    let a = ComputerSession {
        id: ComputerSessionId::new(),
        bot_id: BotId::new(),
        worker_id: worker,
        owner: InputOwner::Bot,
        revision: Revision::INITIAL,
        status: ControlStatus::Pending,
    };
    send_probe(
        &mut socket,
        ManagerMessage::Welcome {
            worker_id: worker,
            generation,
            sessions: vec![a.clone()],
        },
    )
    .await;
    assert!(matches!(
        report(&mut socket).await.message,
        WorkerMessage::SnapshotApplied { .. }
    ));
    let start = botstral_link::Command {
        id: CommandId::new(),
        task_id: TaskId::new(),
        fence: a.fence(generation),
        action: CommandAction::Start {
            work: WorkSpec::TestInput { steps: 10 },
        },
    };
    send_probe(&mut socket, ManagerMessage::Command(start.clone())).await;
    let queued = loop {
        let report = report(&mut socket).await;
        if matches!(report.message, WorkerMessage::BotInput { .. }) {
            break report;
        }
    };
    let mut owned = a.clone();
    owned.owner = InputOwner::Owner;
    owned.revision = owned.revision.advance().unwrap();
    send_probe(
        &mut socket,
        ManagerMessage::Control {
            generation,
            session: owned.clone(),
        },
    )
    .await;
    assert!(matches!(
        report(&mut socket).await.message,
        WorkerMessage::ControlApplied { .. }
    ));
    socket
        .send(Message::Text(
            serde_json::to_string(&Envelope {
                version: VERSION,
                request_id: queued.request_id,
                message: ManagerMessage::Receipt {
                    command_id: start.id,
                    outcome: Acknowledgment::Accepted,
                },
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let status = botstral_link::Command {
        id: CommandId::new(),
        task_id: start.task_id,
        fence: owned.fence(generation),
        action: CommandAction::Status,
    };
    send_probe(&mut socket, ManagerMessage::Command(status)).await;
    assert!(matches!(
        report(&mut socket).await.message,
        WorkerMessage::Acknowledgment { .. }
    ));
    assert!(matches!(
        report(&mut socket).await.message,
        WorkerMessage::Event {
            event: TaskEvent::Status {
                status: TaskStatus::Suspended,
                applied: 0
            },
            ..
        }
    ));
    for (fence, code) in [
        (a.fence(generation), ErrorCode::StaleRevision),
        (owned.fence(Generation::INITIAL), ErrorCode::StaleGeneration),
    ] {
        send_probe(
            &mut socket,
            ManagerMessage::Input {
                command_id: CommandId::new(),
                fence,
                value: 1,
            },
        )
        .await;
        assert!(
            matches!(report(&mut socket).await.message, WorkerMessage::Acknowledgment { outcome: Acknowledgment::Rejected { code: received }, .. } if received == code)
        );
    }
    daemon.stop().await;
    f.manager.stop().await;
}
