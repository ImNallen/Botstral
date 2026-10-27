use super::*;

#[tokio::test]
async fn w01_manager_store_reservation_refuses_authority_at_the_finite_byte_limit() {
    let mut f = Fixture::new().await;
    let session = f.durable_session().await;
    let journal = JournalId::new();
    let (mut socket, generation) = connection(&f, journal, vec![], 0).await;
    let session = f.confirmed(session.id).await;
    let deadline = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        + 300;
    let spec = ComputerSpec {
        profile: ComputerProfile::LinuxPlaywrightV1,
        account: ProcessAccount {
            uid: 1000,
            gid: 1000,
        },
        runtime: ComputerRuntime {
            sha256: CertificateFingerprint::from_der(b"fixture"),
        },
        deadline,
        limits: ProcessLimits {
            runtime_seconds: 60,
            output_bytes: 1024,
            processes: 128,
            memory_bytes: 1073741824,
        },
        viewport: Viewport {
            width: 800,
            height: 600,
        },
        operations: vec![ComputerOperation::WriteFile {
            path: "/approved/file".into(),
            text: "fixture".into(),
        }],
        outputs: (0..2)
            .map(|n| OutputDeclaration {
                slot: format!("file{n}"),
                name: "file.txt".into(),
                media_type: MediaType::TextPlain,
                max_bytes: MAX_ARTIFACT_BYTES,
            })
            .collect(),
    };
    spec.validate().unwrap();
    for index in 0..17 {
        let input = SubmitTask {
            id: TaskId::new(),
            session_id: session.id,
            conversation_id: None,
            settings: TaskSettings {
                model: "unavailable".into(),
                max_turns: 1,
                max_tokens: 1,
                deadline,
                max_cost_micros: 0,
            },
            work: WorkSpec::Computer {
                spec: Box::new(spec.clone()),
            },
        };
        let (status, task) = f
            .owner("POST", "/v1/tasks", serde_json::to_value(input).unwrap())
            .await;
        assert_eq!(status, 200);
        let path = format!("/v1/tasks/{}", task["id"].as_str().unwrap());
        assert_eq!(
            f.owner(
                "PUT",
                &format!("{path}/approval"),
                json!({"expires_at":deadline-1})
            )
            .await
            .0,
            200
        );
        let d = delivery(&mut socket).await;
        wire(
            &mut socket,
            WorkerMessage::Claim {
                incarnation: journal,
                fence: d.fence,
                binding: d.authority.unwrap(),
            },
        )
        .await;
        if index == 16 {
            assert_eq!(
                read(&mut socket).await,
                ManagerMessage::ClaimDenied {
                    execution: d.body.execution
                }
            );
            assert_eq!(
                f.owner("GET", &path, json!({})).await.1["approval"]["consumed"],
                false
            );
            break;
        }
        assert!(matches!(
            read(&mut socket).await,
            ManagerMessage::Permit { .. }
        ));
        let scope = ProcessScope {
            journal,
            execution: d.body.execution,
            host: CertificateFingerprint::from_der(b"fixture"),
            boot: RequestId::new().to_string(),
            invocation: "b".repeat(32),
            unit: format!("botstral-{journal}-{}.service", d.body.execution),
            cgroup: format!("/fixture/botstral-{journal}-{}.service", d.body.execution),
        };
        wire(
            &mut socket,
            WorkerMessage::Reports {
                generation,
                incarnation: journal,
                high_water: index + 1,
                reports: vec![DurableReport {
                    sequence: index + 1,
                    command: d.body.id,
                    execution: d.body.execution,
                    event: ExecutionEvent::ProcessFinished {
                        reason: ProcessCompletionReason::Result,
                        success: true,
                        scope,
                    },
                }],
            },
        )
        .await;
        assert_eq!(
            read(&mut socket).await,
            ManagerMessage::ReportAck { through: index + 1 }
        );
    }
    let db = rusqlite::Connection::open(f.root.path().join("manager/manager.sqlite3")).unwrap();
    let reserved: i64 = db
        .query_row("SELECT sum(reserved) FROM artifacts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(reserved as u64, MAX_STORE_BYTES);
    if let Some(path) = std::env::var_os("BOTSTRAL_PROCESS_EVIDENCE") {
        fs::create_dir_all(&path).unwrap();
        fs::write(PathBuf::from(path).join("artifact-admission.json"),serde_json::to_vec_pretty(&json!({"passed":true,"reserved_bytes":reserved,"execution_count":16,"next_approval_consumed":false})).unwrap()).unwrap();
    }
    f.manager.stop().await;
}

async fn connection(
    f: &Fixture,
    journal: JournalId,
    entries: Vec<JournalEntry>,
    high: u64,
) -> (Socket, Generation) {
    let mut socket = transport::connect_socket(&f.worker).await.unwrap();
    let mut hello = Hello::current(false);
    hello.capabilities.push(Capability::ComputerV1);
    wire(&mut socket, WorkerMessage::Hello(hello)).await;
    let ManagerMessage::Welcome { generation, .. } = read(&mut socket).await else {
        panic!("welcome")
    };
    wire(&mut socket, WorkerMessage::SnapshotApplied { generation }).await;
    wire(
        &mut socket,
        WorkerMessage::RecoveryHello {
            generation,
            incarnation: journal,
            high_water: high,
        },
    )
    .await;
    assert!(matches!(
        read(&mut socket).await,
        ManagerMessage::InventoryRequest { offset: 0 }
    ));
    wire(
        &mut socket,
        WorkerMessage::Inventory {
            generation,
            incarnation: journal,
            offset: 0,
            total: entries.len() as u16,
            entries,
        },
    )
    .await;
    assert!(matches!(
        read(&mut socket).await,
        ManagerMessage::RecoveryReady { .. }
    ));
    (socket, generation)
}
async fn delivery(socket: &mut Socket) -> Delivery {
    loop {
        match read(socket).await {
            ManagerMessage::Delivery(d) if d.authority.is_some() => return d,
            ManagerMessage::Delivery(_) => {}
            m => panic!("delivery {m:?}"),
        }
    }
}
async fn progress(socket: &mut Socket) -> (ArtifactId, u64, bool) {
    loop {
        match read(socket).await {
            ManagerMessage::ArtifactProgress {
                id, offset, state, ..
            } => return (id, offset, state == ArtifactTransferState::Stored),
            ManagerMessage::Delivery(_) => {}
            m => panic!("progress {m:?}"),
        }
    }
}
fn chunk(offer: &ArtifactOffer, id: ArtifactId, offset: u64, bytes: &[u8]) -> WorkerMessage {
    use std::fmt::Write;
    let mut hex = String::new();
    for b in bytes {
        write!(&mut hex, "{b:02x}").unwrap();
    }
    WorkerMessage::ArtifactChunk {
        chunk: ArtifactChunk {
            journal: offer.journal,
            execution: offer.execution,
            slot: offer.slot.clone(),
            id,
            offset,
            hex,
        },
    }
}
async fn denied(socket: &mut Socket, message: WorkerMessage) {
    wire(socket, message).await;
    let actual = read(socket).await;
    assert!(
        matches!(actual, ManagerMessage::Error { .. }),
        "expected protocol rejection, received {actual:?}"
    );
}
async fn offset_committed(f: &Fixture, id: ArtifactId, expected: u64) {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let db = rusqlite::Connection::open(f.root.path().join("manager/manager.sqlite3")).unwrap();
        let offset: i64 = db
            .query_row(
                "SELECT offset FROM artifacts WHERE id=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        if offset as u64 == expected {
            return;
        }
        assert!(
            Instant::now() < until,
            "durable offset {offset}, expected {expected}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
#[tokio::test]
async fn w01_artifact_protocol_uses_durable_admitted_scope_and_idempotent_bytes() {
    let mut f = Fixture::new().await;
    let session = f.durable_session().await;
    let journal = JournalId::new();
    let (mut socket, _) = connection(&f, journal, vec![], 0).await;
    let session = f.confirmed(session.id).await;
    let deadline = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        + 300;
    let approval_expires = deadline - 297;
    let mut request = json!({"id":TaskId::new(),"session_id":session.id,"conversation_id":null,"settings":{"model":"unavailable","max_turns":1,"max_tokens":1,"deadline":deadline,"max_cost_micros":0},"work":{"type":"computer","spec":{"profile":"linux_playwright_v1","account":{"uid":1000,"gid":1000},"runtime":{"sha256":"a".repeat(64)},"deadline":deadline,"limits":{"runtime_seconds":60,"output_bytes":1024,"processes":128,"memory_bytes":1073741824},"viewport":{"width":800,"height":600},"operations":[{"type":"export_file","path":"/approved/file","output":"file"}],"outputs":[{"slot":"file","name":"file.txt","media_type":"text_plain","max_bytes":65536}]}}});
    for slot in ["checksum", "partial"] {
        request["work"]["spec"]["outputs"]
            .as_array_mut()
            .unwrap()
            .push(
                json!({"slot":slot,"name":"file.txt","media_type":"text_plain","max_bytes":65536}),
            );
    }
    let (status, task) = f.owner("POST", "/v1/tasks", request.clone()).await;
    assert_eq!(status, 200, "{task}");
    let task_path = format!("/v1/tasks/{}", task["id"].as_str().unwrap());
    assert_eq!(
        f.owner(
            "PUT",
            &format!("{task_path}/approval"),
            json!({"expires_at":approval_expires})
        )
        .await
        .0,
        200
    );
    let d = delivery(&mut socket).await;
    wire(
        &mut socket,
        WorkerMessage::Claim {
            incarnation: journal,
            fence: d.fence,
            binding: d.authority.unwrap(),
        },
    )
    .await;
    assert!(matches!(
        read(&mut socket).await,
        ManagerMessage::Permit { .. }
    ));
    denied(
        &mut socket,
        WorkerMessage::ArtifactOffer {
            offer: ArtifactOffer {
                journal,
                task: d.body.task,
                execution: d.body.execution,
                slot: "file".into(),
                output: SealedOutput::Unavailable {
                    reason: ArtifactUnavailable::Missing,
                },
            },
        },
    )
    .await;
    let (reconnected, generation) = connection(
        &f,
        journal,
        vec![JournalEntry {
            body: d.body.clone(),
            phase: JournalPhase::Admitted,
        }],
        0,
    )
    .await;
    socket = reconnected;
    let scope = ProcessScope {
        journal,
        execution: d.body.execution,
        host: CertificateFingerprint::from_der(b"fixture"),
        boot: RequestId::new().to_string(),
        invocation: "b".repeat(32),
        unit: format!("botstral-{journal}-{}.service", d.body.execution),
        cgroup: format!("/fixture/botstral-{journal}-{}.service", d.body.execution),
    };
    let reports = vec![
        DurableReport {
            sequence: 1,
            command: d.body.id,
            execution: d.body.execution,
            event: ExecutionEvent::ProcessResult {
                evidence: ProcessEvidence {
                    effect: ProcessEffect::Observed,
                    result: None,
                },
            },
        },
        DurableReport {
            sequence: 2,
            command: d.body.id,
            execution: d.body.execution,
            event: ExecutionEvent::ProcessFinished {
                reason: ProcessCompletionReason::Result,
                success: true,
                scope,
            },
        },
    ];
    wire(
        &mut socket,
        WorkerMessage::Reports {
            generation,
            incarnation: journal,
            high_water: 2,
            reports,
        },
    )
    .await;
    assert_eq!(
        read(&mut socket).await,
        ManagerMessage::ReportAck { through: 2 }
    );
    let completed = f.owner("GET", &task_path, json!({})).await.1;
    assert_eq!(completed["state"], "completed");
    assert_eq!(completed["outputs"][0]["state"]["type"], "pending");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    if now <= approval_expires {
        tokio::time::sleep(Duration::from_secs((approval_expires - now + 1) as u64)).await;
    }
    let bytes = vec![b'x'; ARTIFACT_CHUNK_BYTES + 17];
    let offer = ArtifactOffer {
        journal,
        task: d.body.task,
        execution: d.body.execution,
        slot: "file".into(),
        output: SealedOutput::Content {
            content: ArtifactContent {
                sha256: CertificateFingerprint::from_der(&bytes),
                length: bytes.len() as u64,
            },
        },
    };
    let entry = JournalEntry {
        body: d.body.clone(),
        phase: JournalPhase::Resolved,
    };
    for field in ["journal", "task", "execution"] {
        let mut value = serde_json::to_value(&offer).unwrap();
        value[field] = RequestId::new().to_string().into();
        denied(
            &mut socket,
            WorkerMessage::ArtifactOffer {
                offer: serde_json::from_value(value).unwrap(),
            },
        )
        .await;
        socket = connection(&f, journal, vec![entry.clone()], 2).await.0;
    }
    let mut wrong = offer.clone();
    wrong.slot = "extra".into();
    denied(&mut socket, WorkerMessage::ArtifactOffer { offer: wrong }).await;
    socket = connection(&f, journal, vec![entry.clone()], 2).await.0;
    wire(
        &mut socket,
        WorkerMessage::ArtifactOffer {
            offer: offer.clone(),
        },
    )
    .await;
    let (id, offset, available) = progress(&mut socket).await;
    assert_eq!((offset, available), (0, false));
    let mut wrong = offer.clone();
    wrong.output = SealedOutput::Content {
        content: ArtifactContent {
            sha256: CertificateFingerprint::from_der(b"wrong"),
            length: bytes.len() as u64,
        },
    };
    denied(&mut socket, WorkerMessage::ArtifactOffer { offer: wrong }).await;
    socket = connection(&f, journal, vec![entry.clone()], 2).await.0;
    denied(&mut socket, chunk(&offer, id, 1, b"gap")).await;
    socket = connection(&f, journal, vec![entry.clone()], 2).await.0;
    wire(
        &mut socket,
        chunk(&offer, id, 0, &bytes[..ARTIFACT_CHUNK_BYTES]),
    )
    .await;
    offset_committed(&f, id, ARTIFACT_CHUNK_BYTES as u64).await;
    socket.close(None).await.unwrap();
    let entry = JournalEntry {
        body: d.body.clone(),
        phase: JournalPhase::Resolved,
    };
    let (mut socket, _) = connection(&f, journal, vec![entry.clone()], 2).await;
    wire(
        &mut socket,
        WorkerMessage::ArtifactOffer {
            offer: offer.clone(),
        },
    )
    .await;
    assert_eq!(
        progress(&mut socket).await,
        (id, ARTIFACT_CHUNK_BYTES as u64, false)
    );
    denied(&mut socket, chunk(&offer, id, 0, b"mismatched duplicate")).await;
    socket = connection(&f, journal, vec![entry.clone()], 2).await.0;
    wire(
        &mut socket,
        chunk(&offer, id, 0, &bytes[..ARTIFACT_CHUNK_BYTES]),
    )
    .await;
    assert_eq!(
        progress(&mut socket).await,
        (id, ARTIFACT_CHUNK_BYTES as u64, false)
    );
    let partial = format!("{task_path}/artifacts/{id}");
    assert_eq!(f.owner("GET", &partial, json!({})).await.0, 404);
    wire(
        &mut socket,
        chunk(
            &offer,
            id,
            ARTIFACT_CHUNK_BYTES as u64,
            &bytes[ARTIFACT_CHUNK_BYTES..],
        ),
    )
    .await;
    offset_committed(&f, id, bytes.len() as u64).await;
    socket.close(None).await.unwrap();
    f.restart_manager(None).await;
    let (mut socket, _) = connection(&f, journal, vec![entry], 2).await;
    wire(
        &mut socket,
        WorkerMessage::ArtifactOffer {
            offer: offer.clone(),
        },
    )
    .await;
    assert_eq!(progress(&mut socket).await, (id, bytes.len() as u64, true));
    wire(
        &mut socket,
        chunk(
            &offer,
            id,
            ARTIFACT_CHUNK_BYTES as u64,
            &bytes[ARTIFACT_CHUNK_BYTES..],
        ),
    )
    .await;
    assert_eq!(progress(&mut socket).await, (id, bytes.len() as u64, true));
    wire(
        &mut socket,
        WorkerMessage::ArtifactFailed {
            offer: offer.clone(),
        },
    )
    .await;
    assert_eq!(progress(&mut socket).await, (id, bytes.len() as u64, true));
    let grant = f.invitation(&f.other, 600).await;
    let enrolled = worker_http(
        &f.other,
        "POST",
        "/worker/v1/enroll",
        json!({"invitation":grant["invitation"]}),
    )
    .await;
    assert_eq!(enrolled.0, 201);
    let mut unrelated = transport::connect_socket(&f.other).await.unwrap();
    let mut hello = Hello::current(false);
    hello.capabilities.push(Capability::ComputerV1);
    wire(&mut unrelated, WorkerMessage::Hello(hello)).await;
    let ManagerMessage::Welcome { generation, .. } = read(&mut unrelated).await else {
        panic!("welcome")
    };
    wire(
        &mut unrelated,
        WorkerMessage::SnapshotApplied { generation },
    )
    .await;
    let other_journal = JournalId::new();
    wire(
        &mut unrelated,
        WorkerMessage::RecoveryHello {
            generation,
            incarnation: other_journal,
            high_water: 0,
        },
    )
    .await;
    assert!(matches!(
        read(&mut unrelated).await,
        ManagerMessage::InventoryRequest { .. }
    ));
    wire(
        &mut unrelated,
        WorkerMessage::Inventory {
            generation,
            incarnation: other_journal,
            offset: 0,
            total: 0,
            entries: vec![],
        },
    )
    .await;
    assert!(matches!(
        read(&mut unrelated).await,
        ManagerMessage::RecoveryReady { .. }
    ));
    denied(
        &mut unrelated,
        WorkerMessage::ArtifactOffer {
            offer: offer.clone(),
        },
    )
    .await;

    let result = f.owner("GET", &task_path, json!({})).await.1;
    assert_eq!(result["outputs"][0]["state"]["type"], "available");
    let db = rusqlite::Connection::open(f.root.path().join("manager/manager.sqlite3")).unwrap();
    assert_eq!(
        db.query_row::<i64, _, _>("SELECT count(*) FROM artifacts", [], |r| r.get(0))
            .unwrap(),
        3
    );
    for slot in ["checksum", "partial", "file"] {
        let mut damaged = offer.clone();
        damaged.slot = slot.into();
        let (artifact, through) = if slot == "file" {
            (id, bytes.len() as u64)
        } else {
            wire(
                &mut socket,
                WorkerMessage::ArtifactOffer {
                    offer: damaged.clone(),
                },
            )
            .await;
            let (artifact, offset, stored) = progress(&mut socket).await;
            assert_eq!((offset, stored), (0, false));
            wire(
                &mut socket,
                chunk(&damaged, artifact, 0, &bytes[..ARTIFACT_CHUNK_BYTES]),
            )
            .await;
            assert_eq!(
                progress(&mut socket).await,
                (artifact, ARTIFACT_CHUNK_BYTES as u64, false)
            );
            (artifact, ARTIFACT_CHUNK_BYTES as u64)
        };
        let suffix = if slot == "file" { "blob" } else { "part" };
        let path = f
            .root
            .path()
            .join("manager/artifacts")
            .join(format!("{artifact}.{suffix}"));
        let mut file = OpenOptions::new().write(true).open(path).unwrap();
        if slot == "partial" {
            file.set_len(0).unwrap();
        } else {
            file.write_all(b"corrupt").unwrap();
        }
        file.sync_all().unwrap();
        if slot != "checksum" {
            socket.close(None).await.unwrap();
            f.restart_manager(None).await;
            socket = connection(
                &f,
                journal,
                vec![JournalEntry {
                    body: d.body.clone(),
                    phase: JournalPhase::Resolved,
                }],
                2,
            )
            .await
            .0;
        }
        let resume = through.min(ARTIFACT_CHUNK_BYTES as u64);
        wire(
            &mut socket,
            chunk(&damaged, artifact, resume, &bytes[resume as usize..]),
        )
        .await;
        let receipt = read(&mut socket).await;
        assert!(
            matches!(receipt,
            ManagerMessage::ArtifactProgress { id: received, state: ArtifactTransferState::Unavailable, .. } if received == artifact),
            "expected durable unavailable progress, received {receipt:?}"
        );
        let state = f.owner("GET", &task_path, json!({})).await.1;
        assert_eq!(state["state"], "completed");
        assert_eq!(
            state["outputs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|o| o["declaration"]["slot"] == slot)
                .unwrap()["state"]["type"],
            "unavailable"
        );
    }
    if let Some(path) = std::env::var_os("BOTSTRAL_PROCESS_EVIDENCE") {
        fs::create_dir_all(&path).unwrap();
        fs::write(PathBuf::from(path).join("artifact-protocol.json"),serde_json::to_vec_pretty(&json!({"passed":true,"task":result,"offset":bytes.len(),"lost_chunk_ack":true,"lost_final_receipt":true,"wrong_worker_refused":true,"early_unavailable_refused":true,"corrupt_chunks_receive_terminal_progress":true})).unwrap()).unwrap();
    }
    f.manager.stop().await;
}
