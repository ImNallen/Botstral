use super::*;
use std::sync::Arc;

type ProbeSocket = WebSocketStream<tokio_rustls::server::TlsStream<TcpStream>>;

struct Probe {
    fixture: Fixture,
    daemon: Process,
    socket: ProbeSocket,
    sessions: Vec<ComputerSession>,
    generation: Generation,
    incarnation: JournalId,
    state: PathBuf,
}

impl Probe {
    async fn new() -> Self {
        let fixture = Fixture::new().await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
            Arc::new(tls::roots(&fixture.worker.manager_ca).unwrap()),
            provider.clone(),
        )
        .build()
        .unwrap();
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_client_cert_verifier(verifier)
            .with_single_cert(
                tls::certificates(&fixture.root.path().join("server.pem")).unwrap(),
                tls::private_key(&fixture.root.path().join("server.key")).unwrap(),
            )
            .unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let state = fixture.root.path().join("permit-agent");
        fs::create_dir(&state).unwrap();
        fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
        let mut client = fixture.worker.clone();
        client.manager = format!("wss://localhost:{}", listener.local_addr().unwrap().port());
        private(
            &state.join("config.json"),
            &serde_json::to_vec(&client).unwrap(),
        );
        let daemon = launch(
            Path::new(env!("CARGO_BIN_EXE_botstral-test-agent")),
            &["run", "--state-dir", state.to_str().unwrap()],
            fixture.root.path(),
            "permit-agent",
        );
        let stream = tokio::time::timeout(Duration::from_secs(5), listener.accept())
            .await
            .unwrap()
            .unwrap()
            .0;
        let stream = acceptor.accept(stream).await.unwrap();
        let socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        let worker = WorkerId::new();
        let sessions = (0..2)
            .map(|_| ComputerSession {
                id: ComputerSessionId::new(),
                bot_id: BotId::new(),
                worker_id: worker,
                owner: InputOwner::Bot,
                revision: Revision::INITIAL,
                status: ControlStatus::Confirmed,
            })
            .collect();
        let mut probe = Self {
            fixture,
            daemon,
            socket,
            sessions,
            generation: Generation::INITIAL,
            incarnation: JournalId::new(),
            state,
        };
        assert!(matches!(probe.read().await, WorkerMessage::Hello(_)));
        probe
            .send(ManagerMessage::Welcome {
                worker_id: worker,
                generation: probe.generation,
                sessions: probe.sessions.clone(),
            })
            .await;
        assert!(matches!(
            probe.read().await,
            WorkerMessage::SnapshotApplied { .. }
        ));
        let WorkerMessage::RecoveryHello { incarnation, .. } = probe.read().await else {
            panic!("journal hello")
        };
        probe.incarnation = incarnation;
        probe
            .send(ManagerMessage::InventoryRequest { offset: 0 })
            .await;
        assert!(matches!(
            probe.read().await,
            WorkerMessage::Inventory { total: 0, .. }
        ));
        probe
            .send(ManagerMessage::RecoveryReady {
                generation: probe.generation,
            })
            .await;
        probe
    }
    async fn send(&mut self, message: ManagerMessage) {
        self.socket
            .send(Message::Text(
                serde_json::to_string(&Envelope::new(message))
                    .unwrap()
                    .into(),
            ))
            .await
            .unwrap();
    }
    async fn read(&mut self) -> WorkerMessage {
        loop {
            let Message::Text(text) =
                tokio::time::timeout(Duration::from_secs(8), self.socket.next())
                    .await
                    .expect("bounded worker response")
                    .unwrap()
                    .unwrap()
            else {
                panic!("text response")
            };
            let message = decode::<WorkerMessage>(text.as_bytes()).unwrap().message;
            if !matches!(message, WorkerMessage::Heartbeat { .. }) {
                return message;
            }
        }
    }
    async fn acknowledge(&mut self, reports: &[DurableReport]) {
        self.send(ManagerMessage::ReportAck {
            through: reports.last().unwrap().sequence,
        })
        .await;
    }
    fn delivery(&self, session: usize, expires_at: i64) -> Delivery {
        let session = &self.sessions[session];
        let body = CommandBody {
            id: CommandId::new(),
            owner: OwnerId::new(),
            task: TaskId::new(),
            bot: session.bot_id,
            worker: session.worker_id,
            session: session.id,
            operation: OperationId::new(),
            execution: ExecutionId::new(),
            work: WorkSpec::TestEffect {
                value: 3,
                observable: true,
            },
        };
        let authority = ExecutionBinding {
            owner: body.owner,
            bot: body.bot,
            task: body.task,
            operation: body.operation,
            arguments: body.argument_digest(),
            permission_revision: 1,
            approval: ApprovalId::new(),
            execution: body.execution,
            expires_at,
        };
        Delivery {
            body,
            fence: session.fence(self.generation),
            authority: Some(authority),
            cancel: false,
        }
    }
    async fn claim(&mut self, expected: &Delivery) {
        loop {
            match self.read().await {
                WorkerMessage::Claim { binding, .. } => {
                    assert_eq!(Some(binding), expected.authority);
                    return;
                }
                WorkerMessage::Reports { reports, .. } => self.acknowledge(&reports).await,
                other => panic!("claim expected, got {other:?}"),
            }
        }
    }
    async fn permit(&mut self, delivery: &Delivery) {
        self.send(ManagerMessage::Permit {
            incarnation: self.incarnation,
            fence: delivery.fence,
            binding: delivery.authority.clone().unwrap(),
        })
        .await;
    }
    async fn completed(&mut self, execution: ExecutionId) {
        loop {
            match self.read().await {
                WorkerMessage::Reports { reports, .. } => {
                    let completed = reports.iter().any(|report| {
                        report.execution == execution
                            && matches!(report.event, ExecutionEvent::Completed { applied: 3 })
                    });
                    assert!(
                        !reports.iter().any(|report| report.execution == execution
                            && matches!(report.event, ExecutionEvent::Uncertain { .. })),
                        "valid execution became uncertain"
                    );
                    self.acknowledge(&reports).await;
                    if completed {
                        return;
                    }
                }
                other => panic!("completion expected, got {other:?}"),
            }
        }
    }
    fn invocations(&self) -> i64 {
        rusqlite::Connection::open(self.state.join("synthetic-provider.sqlite3"))
            .unwrap()
            .query_row(
                "SELECT coalesce(sum(invocations), 0) FROM effects",
                [],
                |row| row.get(0),
            )
            .unwrap()
    }
    async fn stop(mut self) {
        self.daemon.stop().await;
        self.fixture.manager.stop().await;
    }
}

#[tokio::test]
async fn invalid_permit_preserves_another_sessions_pending_claim() {
    let mut probe = Probe::new().await;
    let first = probe.delivery(0, epoch() + 60);
    let second = probe.delivery(1, epoch() + 600);
    probe.send(ManagerMessage::Delivery(first.clone())).await;
    probe.claim(&first).await;
    probe.send(ManagerMessage::Delivery(second.clone())).await;
    probe.claim(&second).await;
    let mut expired = first;
    expired.authority.as_mut().unwrap().expires_at = epoch() - 1;
    probe.permit(&expired).await;
    probe.permit(&second).await;
    probe.completed(second.body.execution).await;
    assert_eq!(probe.invocations(), 1);
    evidence(
        "permit-isolation",
        json!({"unrelated_execution_completed":true,"invocations":1}),
    );
    probe.stop().await;
}

#[tokio::test]
async fn renewed_approval_survives_denial_of_older_pending_claim() {
    let mut probe = Probe::new().await;
    let original = probe.delivery(0, epoch() + 60);
    probe.send(ManagerMessage::Delivery(original.clone())).await;
    probe.claim(&original).await;
    let mut renewed = original.clone();
    renewed.authority.as_mut().unwrap().expires_at += 60;
    probe.send(ManagerMessage::Delivery(renewed.clone())).await;
    probe
        .send(ManagerMessage::ClaimDenied {
            execution: original.body.execution,
        })
        .await;
    probe.claim(&renewed).await;
    probe.permit(&renewed).await;
    probe.completed(renewed.body.execution).await;
    assert_eq!(probe.invocations(), 1);
    evidence(
        "approval-renewal",
        json!({"renewed_claim_completed":true,"invocations":1}),
    );
    probe.stop().await;
}

#[tokio::test]
async fn actual_worker_report_pages_respect_count_and_byte_limits() {
    let mut probe = Probe::new().await;
    for _ in 0..24 {
        let mut delivery = probe.delivery(0, epoch() + 600);
        delivery.authority = None;
        probe.send(ManagerMessage::Delivery(delivery)).await;
    }
    let mut sequences = Vec::new();
    let mut sizes = Vec::new();
    let mut encoded_bytes = Vec::new();
    while sequences.len() < 48 {
        let WorkerMessage::Reports { reports, .. } = probe.read().await else {
            panic!("report page")
        };
        let bytes = serde_json::to_vec(&reports).unwrap().len();
        assert!(reports.len() <= 16);
        assert!(bytes <= 48 * 1024);
        sizes.push(reports.len());
        encoded_bytes.push(bytes);
        sequences.extend(reports.iter().map(|report| report.sequence));
        probe.acknowledge(&reports).await;
    }
    assert_eq!(sequences, (1..=48).collect::<Vec<_>>());
    assert!(sizes.len() >= 3);
    assert_eq!(probe.invocations(), 0);
    evidence(
        "observed-report-pages",
        json!({"observed_page_entries":sizes,"observed_page_bytes":encoded_bytes,"report_sequences":sequences,"invocations":0}),
    );
    probe.stop().await;
}
