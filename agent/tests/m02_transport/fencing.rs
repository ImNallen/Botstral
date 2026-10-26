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

    assert!(matches!(
        report(&mut socket).await.message,
        WorkerMessage::RecoveryHello { .. }
    ));
    send_probe(&mut socket, ManagerMessage::InventoryRequest { offset: 0 }).await;
    assert!(matches!(
        report(&mut socket).await.message,
        WorkerMessage::Inventory { total: 0, .. }
    ));
    send_probe(&mut socket, ManagerMessage::RecoveryReady { generation }).await;
    let body = CommandBody {
        id: CommandId::new(),
        owner: OwnerId::new(),
        task: TaskId::new(),
        bot: a.bot_id,
        worker,
        session: a.id,
        operation: OperationId::new(),
        execution: ExecutionId::new(),
        work: WorkSpec::TestEffect {
            value: 1,
            observable: true,
        },
    };
    let binding = ExecutionBinding {
        owner: body.owner,
        bot: body.bot,
        task: body.task,
        operation: body.operation,
        arguments: body.argument_digest(),
        permission_revision: 1,
        approval: ApprovalId::new(),
        execution: body.execution,
        expires_at: 4000000000,
    };
    send_probe(
        &mut socket,
        ManagerMessage::Delivery(Delivery {
            body: body.clone(),
            fence: a.fence(generation),
            authority: Some(binding.clone()),
            cancel: false,
        }),
    )
    .await;
    let incarnation = loop {
        match report(&mut socket).await.message {
            WorkerMessage::Claim { incarnation, .. } => break incarnation,
            WorkerMessage::Reports { reports, .. } => {
                send_probe(
                    &mut socket,
                    ManagerMessage::ReportAck {
                        through: reports.last().unwrap().sequence,
                    },
                )
                .await
            }
            other => panic!("claim expected {other:?}"),
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
    loop {
        match report(&mut socket).await.message {
            WorkerMessage::ControlApplied { .. } => break,
            WorkerMessage::Reports { reports, .. } => {
                send_probe(
                    &mut socket,
                    ManagerMessage::ReportAck {
                        through: reports.last().unwrap().sequence,
                    },
                )
                .await
            }
            other => panic!("control expected {other:?}"),
        }
    }
    send_probe(
        &mut socket,
        ManagerMessage::Permit {
            incarnation,
            fence: a.fence(generation),
            binding,
        },
    )
    .await;
    let db = rusqlite::Connection::open(state.join("synthetic-provider.sqlite3")).unwrap();
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
        loop {
            match report(&mut socket).await.message {
                WorkerMessage::Acknowledgment {
                    outcome: Acknowledgment::Rejected { code: received },
                    ..
                } => {
                    assert_eq!(received, code);
                    break;
                }
                WorkerMessage::Reports { reports, .. } => {
                    send_probe(
                        &mut socket,
                        ManagerMessage::ReportAck {
                            through: reports.last().unwrap().sequence,
                        },
                    )
                    .await
                }
                other => panic!("refusal expected {other:?}"),
            }
        }
    }
    assert_eq!(
        db.query_row::<i64, _, _>("SELECT count(*) FROM effects", [], |r| r.get(0))
            .unwrap(),
        0,
        "stale permit never invoked provider"
    );

    let mut bot = owned.clone();
    bot.owner = InputOwner::Bot;
    bot.revision = bot.revision.advance().unwrap();
    send_probe(
        &mut socket,
        ManagerMessage::Control {
            generation,
            session: bot.clone(),
        },
    )
    .await;
    loop {
        match report(&mut socket).await.message {
            WorkerMessage::ControlApplied { .. } => break,
            WorkerMessage::Reports { reports, .. } => {
                send_probe(
                    &mut socket,
                    ManagerMessage::ReportAck {
                        through: reports.last().unwrap().sequence,
                    },
                )
                .await
            }
            other => panic!("control {other:?}"),
        }
    }
    let next = CommandBody {
        id: CommandId::new(),
        task: TaskId::new(),
        operation: OperationId::new(),
        execution: ExecutionId::new(),
        ..body
    };
    let authority = ExecutionBinding {
        owner: next.owner,
        bot: next.bot,
        task: next.task,
        operation: next.operation,
        arguments: next.argument_digest(),
        permission_revision: 1,
        approval: ApprovalId::new(),
        execution: next.execution,
        expires_at: 4000000000,
    };
    let delivery = Delivery {
        body: next.clone(),
        fence: bot.fence(generation),
        authority: Some(authority.clone()),
        cancel: false,
    };
    send_probe(&mut socket, ManagerMessage::Delivery(delivery.clone())).await;
    let mut completed_sequence = None;
    while completed_sequence.is_none() {
        match report(&mut socket).await.message {
            WorkerMessage::Claim {
                incarnation,
                fence,
                binding,
            } => {
                send_probe(
                    &mut socket,
                    ManagerMessage::Permit {
                        incarnation,
                        fence,
                        binding,
                    },
                )
                .await
            }
            WorkerMessage::Reports { reports, .. } => {
                completed_sequence = reports
                    .iter()
                    .find(|r| {
                        r.execution == next.execution
                            && matches!(r.event, ExecutionEvent::Completed { .. })
                    })
                    .map(|r| r.sequence);
                send_probe(
                    &mut socket,
                    ManagerMessage::ReportAck {
                        through: reports.last().unwrap().sequence,
                    },
                )
                .await;
            }
            other => panic!("execution {other:?}"),
        }
    }
    assert_eq!(
        db.query_row::<i64, _, _>("SELECT sum(invocations) FROM effects", [], |r| r.get(0))
            .unwrap(),
        1
    );
    send_probe(&mut socket, ManagerMessage::Delivery(delivery.clone())).await;
    let duplicate = report(&mut socket).await;
    assert!(
        matches!(duplicate.message,WorkerMessage::Reports{reports,..} if reports.iter().any(|r|Some(r.sequence)==completed_sequence && matches!(r.event,ExecutionEvent::Completed{..})))
    );
    let timeout_body = CommandBody {
        id: CommandId::new(),
        task: TaskId::new(),
        operation: OperationId::new(),
        execution: ExecutionId::new(),
        ..next.clone()
    };
    let timeout_binding = ExecutionBinding {
        owner: timeout_body.owner,
        bot: timeout_body.bot,
        task: timeout_body.task,
        operation: timeout_body.operation,
        arguments: timeout_body.argument_digest(),
        permission_revision: 1,
        approval: ApprovalId::new(),
        execution: timeout_body.execution,
        expires_at: 4000000000,
    };
    send_probe(
        &mut socket,
        ManagerMessage::Delivery(Delivery {
            body: timeout_body.clone(),
            fence: bot.fence(generation),
            authority: Some(timeout_binding.clone()),
            cancel: false,
        }),
    )
    .await;
    let mut timed_out = false;
    while !timed_out {
        match report(&mut socket).await.message {
            WorkerMessage::Claim { .. } => {}
            WorkerMessage::Reports { reports, .. } => {
                timed_out=reports.iter().any(|r|r.execution==timeout_body.execution && matches!(&r.event,ExecutionEvent::Uncertain{reason} if reason=="permit_timeout"));
                send_probe(
                    &mut socket,
                    ManagerMessage::ReportAck {
                        through: reports.last().unwrap().sequence,
                    },
                )
                .await;
            }
            other => panic!("permit timeout {other:?}"),
        }
    }
    send_probe(
        &mut socket,
        ManagerMessage::Permit {
            incarnation,
            fence: bot.fence(generation),
            binding: timeout_binding,
        },
    )
    .await;
    let mut changed = delivery;
    changed.body.work = WorkSpec::TestEffect {
        value: 2,
        observable: true,
    };
    send_probe(&mut socket, ManagerMessage::Delivery(changed)).await;
    let closed = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .unwrap();
    assert!(
        closed.is_none()
            || closed.is_some_and(|r| r.is_err() || matches!(r, Ok(Message::Close(_))))
    );
    assert_eq!(
        db.query_row::<i64, _, _>("SELECT sum(invocations) FROM effects", [], |r| r.get(0))
            .unwrap(),
        1,
        "changed identity never replaced execution"
    );
    daemon.stop().await;
    f.manager.stop().await;
}
