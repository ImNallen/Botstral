use super::*;

#[tokio::test]
async fn enrollment_tls_negotiation_and_lifecycle_use_actual_binaries() {
    let mut f = Fixture::new().await;
    let grant = f.invitation(&f.worker, 600).await;
    assert_eq!(
        worker_http(
            &f.worker,
            "POST",
            "/worker/v1/enroll",
            json!({"invitation":grant["invitation"]}),
        )
        .await
        .0,
        201
    );
    assert_eq!(
        worker_http(
            &f.other,
            "POST",
            "/worker/v1/enroll",
            json!({"invitation":grant["invitation"]})
        )
        .await
        .1["error"],
        "invitation_used"
    );
    let expired = f.invitation(&f.other, 1).await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(
        worker_http(
            &f.other,
            "POST",
            "/worker/v1/enroll",
            json!({"invitation":expired["invitation"]})
        )
        .await
        .1["error"],
        "invitation_expired"
    );
    let manager_state = f.root.path().join("manager");
    let mut invite = Command::new(manager_binary())
        .args([
            "invite",
            "worker",
            "--state-dir",
            manager_state.to_str().unwrap(),
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    invite
        .stdin
        .take()
        .unwrap()
        .write_all(b"synthetic transport password\n")
        .unwrap();
    let invited = invite.wait_with_output().unwrap();
    assert!(
        invited.status.success(),
        "invite worker: {}",
        String::from_utf8_lossy(&invited.stderr)
    );
    let code = String::from_utf8(invited.stdout).unwrap();
    let code = code.lines().next().unwrap();
    assert!(JoinCode::parse(code).is_ok());
    let agent_state = f.root.path().join("agent");
    let output = Command::new(agent_binary())
        .args(["join", "--state-dir", agent_state.to_str().unwrap(), code])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "actual enrollment: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let joined = super::public_fingerprint(&agent_state.join("worker-identity.pem"));
    assert!(
        f.owner("GET", "/v1/workers", json!({}))
            .await
            .1
            .as_array()
            .unwrap()
            .iter()
            .any(|worker| worker["fingerprint"] == joined.as_str() && worker["revoked"] == false),
        "joined worker is enrolled"
    );
    let daemon_state = f.root.path().join("daemon");
    fs::create_dir(&daemon_state).unwrap();
    fs::set_permissions(&daemon_state, fs::Permissions::from_mode(0o700)).unwrap();
    private(
        &daemon_state.join("config.json"),
        &serde_json::to_vec(&f.worker).unwrap(),
    );
    for config in [&f.wrong, &f.expired] {
        assert!(transport::connect_socket(config).await.is_err());
    }
    let state = f.root.path().join("wrong-pin");
    fs::create_dir(&state).unwrap();
    fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
    private(
        &state.join("config.json"),
        &serde_json::to_vec(&f.wrong).unwrap(),
    );
    let result = Command::new(agent_binary())
        .args(["check", "--state-dir", state.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        !result.status.success(),
        "actual daemon rejects a manager pin mismatch"
    );
    assert_eq!(
        String::from_utf8_lossy(&result.stderr),
        "worker operation failed\n"
    );
    for path in [
        "/v1/owner",
        "/v1/sessions",
        "/v1/bots",
        "/v1/status",
        "/v1/vault/unlock",
        "/v1/vault/lock",
        "/v1/provider-keys/mistral",
        "/v1/worker-invitations",
        "/v1/devices",
        "/v1/approvals",
        "/v1/security",
    ] {
        for method in ["GET", "POST", "PUT", "DELETE"] {
            assert_eq!(
                worker_http(&f.worker, method, path, json!({})).await.0,
                404,
                "worker route {method} {path}"
            );
        }
    }
    assert_eq!(
        worker_http(
            &f.worker,
            "POST",
            "/worker/v1/enroll",
            json!({"invitation":f.token})
        )
        .await
        .1["error"],
        "invalid_invitation"
    );
    assert_eq!(
        local(
            f.listen,
            "GET",
            "/v1/owner",
            Some(grant["invitation"].as_str().unwrap()),
            json!({})
        )
        .await
        .0,
        401
    );
    let mut capped = Vec::new();
    for _ in 0..16 {
        capped.push(transport::connect_socket(&f.worker).await.unwrap());
    }
    assert!(
        transport::connect_socket(&f.worker).await.is_err(),
        "upgraded sockets retain all 16 permits"
    );
    drop(capped);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut silent = transport::connect_socket(&f.worker).await.unwrap();
    assert_eq!(
        read(&mut silent).await,
        ManagerMessage::Error {
            code: ErrorCode::WrongPhase
        }
    );
    for message in [
        Message::Binary(vec![1, 2, 3].into()),
        Message::Text("{".into()),
        Message::Text("x".repeat(MAX_MESSAGE + 1).into()),
    ] {
        let mut socket = transport::connect_socket(&f.worker).await.unwrap();
        let _ = socket.send(message).await;
        let response = tokio::time::timeout(Duration::from_secs(6), socket.next())
            .await
            .unwrap();
        if let Some(Ok(Message::Text(text))) = response {
            assert_eq!(
                decode::<ManagerMessage>(text.as_bytes()).unwrap().message,
                ManagerMessage::Error {
                    code: ErrorCode::InvalidMessage
                }
            );
        }
    }
    let mut unenrolled = transport::connect_socket(&f.other).await.unwrap();
    wire(&mut unenrolled, WorkerMessage::Hello(Hello::current(false))).await;
    assert_eq!(
        read(&mut unenrolled).await,
        ManagerMessage::Error {
            code: ErrorCode::Unauthorized
        }
    );
    for (hello, expected) in [
        (
            Hello {
                versions: vec![99],
                ..Hello::current(false)
            },
            ErrorCode::IncompatibleVersion,
        ),
        (
            Hello {
                capabilities: vec![],
                ..Hello::current(false)
            },
            ErrorCode::MissingCapability,
        ),
        (
            Hello {
                required: vec![Capability::TestInput],
                ..Hello::current(false)
            },
            ErrorCode::MissingCapability,
        ),
        (
            Hello {
                channel: ReleaseChannel::Experimental,
                ..Hello::current(false)
            },
            ErrorCode::IncompatibleChannel,
        ),
    ] {
        if cfg!(feature = "nonproduction-test-worker")
            && hello.required.contains(&Capability::TestInput)
        {
            continue;
        }
        let mut socket = transport::connect_socket(&f.worker).await.unwrap();
        wire(&mut socket, WorkerMessage::Hello(hello)).await;
        assert_eq!(
            read(&mut socket).await,
            ManagerMessage::Error { code: expected }
        );
    }
    let mut early = transport::connect_socket(&f.worker).await.unwrap();
    wire(
        &mut early,
        WorkerMessage::Heartbeat {
            generation: Generation::INITIAL,
        },
    )
    .await;
    assert_eq!(
        read(&mut early).await,
        ManagerMessage::Error {
            code: ErrorCode::WrongPhase
        }
    );
    for method in [
        "approve",
        "enroll_owner_device",
        "get_credential",
        "change_security_policy",
    ] {
        let mut socket = transport::connect_socket(&f.worker).await.unwrap();
        let mut value =
            serde_json::to_value(Envelope::new(WorkerMessage::Hello(Hello::current(false))))
                .unwrap();
        value["message"] = json!({"type":method});
        socket
            .send(Message::Text(value.to_string().into()))
            .await
            .unwrap();
        assert_eq!(
            read(&mut socket).await,
            ManagerMessage::Error {
                code: ErrorCode::InvalidMessage
            }
        );
    }
    let fingerprint = super::public_fingerprint(&f.worker.client_cert);
    let workers = f.owner("GET", "/v1/workers", json!({})).await.1;
    assert_eq!(
        workers
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["fingerprint"] == fingerprint.as_str())
            .unwrap()["generation"],
        0
    );
    let race = Config {
        client_cert: f.root.path().join("race/worker-identity.pem"),
        client_key: f.root.path().join("race/worker-identity.key"),
        ..f.worker.clone()
    };
    let invitation = f.invitation(&race, 600).await;
    let body = json!({"invitation": invitation["invitation"]});
    let (one, two) = tokio::join!(
        worker_http(&race, "POST", "/worker/v1/enroll", body.clone()),
        worker_http(&race, "POST", "/worker/v1/enroll", body)
    );
    assert_eq!(one.0, 201, "{one:?}");
    assert_eq!(two.0, 201, "{two:?}");
    assert_eq!(one.1["worker_id"], two.1["worker_id"]);
    let recovery = Config {
        client_cert: f.root.path().join("recovery/worker-identity.pem"),
        client_key: f.root.path().join("recovery/worker-identity.key"),
        ..f.worker.clone()
    };
    let invitation = f.invitation(&recovery, 600).await;
    let body = serde_json::to_vec(&json!({"invitation":invitation["invitation"]})).unwrap();
    let mut lost = transport::connect_tls(&recovery).await.unwrap();
    use tokio::io::AsyncWriteExt;
    lost.write_all(format!("POST /worker/v1/enroll HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await.unwrap();
    lost.write_all(&body).await.unwrap();
    lost.flush().await.unwrap();
    let fingerprint = super::public_fingerprint(&recovery.client_cert);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if f.owner("GET", "/v1/workers", json!({}))
                .await
                .1
                .as_array()
                .unwrap()
                .iter()
                .any(|w| w["fingerprint"] == fingerprint.as_str())
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    drop(lost);
    let mut recovered = transport::connect_socket(&recovery).await.unwrap();
    wire(&mut recovered, WorkerMessage::Hello(Hello::current(false))).await;
    let ManagerMessage::Welcome { generation, .. } = read(&mut recovered).await else {
        panic!("certificate recovers lost enrollment response");
    };
    wire(
        &mut recovered,
        WorkerMessage::SnapshotApplied { generation },
    )
    .await;
    drop(recovered);
    let mut daemon = launch(
        agent_binary(),
        &["run", "--state-dir", daemon_state.to_str().unwrap()],
        f.root.path(),
        "agent",
    );
    let first = f.wait_phase("ready", 1).await;
    daemon.stop().await;
    f.wait_phase("disconnected", 1).await;
    let mut daemon = launch(
        agent_binary(),
        &["run", "--state-dir", daemon_state.to_str().unwrap()],
        f.root.path(),
        "agent-reconnect",
    );
    let next = f
        .wait_phase("ready", first["generation"].as_u64().unwrap() + 1)
        .await;
    assert_eq!(next["id"], first["id"]);
    let worker: WorkerId = serde_json::from_value(next["id"].clone()).unwrap();
    let generation = Generation::try_from(next["generation"].as_u64().unwrap()).unwrap();
    let session = f.create_session(worker, "Production refusal").await;
    let session = f.confirmed(session.id).await;
    let (status, task)=f.owner("POST","/v1/tasks",json!({"id":TaskId::new(),"session_id":session.id,"conversation_id":null,"settings":{"model":"unavailable","max_turns":1,"max_tokens":1,"deadline":4000000000i64,"max_cost_micros":0},"work":{"type":"unavailable"}})).await;
    assert_eq!(status, 200);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let result = f
                .owner(
                    "GET",
                    &format!("/v1/tasks/{}", task["id"].as_str().unwrap()),
                    json!({}),
                )
                .await
                .1;
            if result["state"] == "failed" {
                assert_eq!(result["outcome"]["code"], "unsupported_execution");
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let approved = botstral_link::Command {
        id: CommandId::new(),
        task_id: TaskId::new(),
        fence: session.fence(generation),
        action: CommandAction::ExecuteApproved {
            approval_id: ApprovalId::new(),
            operation_id: OperationId::new(),
            argument_digest: CertificateFingerprint::from_der(b"arguments"),
            expires_at: 4000000000,
        },
    };
    assert_eq!(
        f.owner(
            "POST",
            "/v1/worker-commands",
            serde_json::to_value(approved).unwrap()
        )
        .await
        .1["error"],
        "unsupported_execution"
    );
    let taken = f.takeover(&session, InputOwner::Owner).await;
    let input = CommandId::new();
    assert_eq!(
        f.owner(
            "POST",
            "/v1/worker-input",
            json!({"id":input,"fence":taken.fence(generation),"value":1})
        )
        .await
        .0,
        200
    );
    f.receipt(worker, input, |v| {
        v["acknowledgment"] == json!({"type":"rejected","code":"unsupported_execution"})
    })
    .await;
    assert_eq!(
        f.owner("DELETE", &format!("/v1/workers/{worker}"), json!({}))
            .await
            .0,
        204
    );
    f.wait_phase("disconnected", generation.get()).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if fs::read_to_string(f.root.path().join("agent-reconnect.log"))
                .unwrap()
                .contains("worker disconnected; retrying")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("revocation closes the live daemon socket");
    let mut revoked = transport::connect_socket(&f.worker).await.unwrap();
    wire(&mut revoked, WorkerMessage::Hello(Hello::current(false))).await;
    assert_eq!(
        read(&mut revoked).await,
        ManagerMessage::Error {
            code: ErrorCode::Unauthorized
        }
    );
    f.manager.stop().await;
    daemon.stop().await;
    for name in ["manager", "manager.log", "agent.log", "agent-reconnect.log"] {
        let path = f.root.path().join(name);
        let files: Vec<PathBuf> = if path.is_dir() {
            fs::read_dir(path)
                .unwrap()
                .map(|e| e.unwrap().path())
                .collect()
        } else {
            vec![path]
        };
        for path in files {
            let bytes = fs::read(&path).unwrap();
            for secret in [
                grant["invitation"].as_str().unwrap(),
                "PRIVATE KEY",
                "synthetic transport password",
                "synthetic transport passphrase",
            ] {
                if secret == "PRIVATE KEY"
                    && path.file_name().is_some_and(|name| {
                        name == "manager-identity.key" || name == "manager-identity.pem"
                    })
                {
                    continue;
                }
                assert!(
                    !bytes.windows(secret.len()).any(|w| w == secret.as_bytes()),
                    "secret absent in {}",
                    path.display()
                );
            }
        }
    }
    assert_eq!(
        fs::metadata(agent_state.join("config.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[tokio::test]
async fn invite_device_binary_prints_a_join_code() {
    let f = Fixture::new().await;
    let manager_state = f.root.path().join("manager");
    let mut invite = Command::new(manager_binary())
        .args([
            "invite",
            "device",
            "--label",
            "X",
            "--state-dir",
            manager_state.to_str().unwrap(),
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    invite
        .stdin
        .take()
        .unwrap()
        .write_all(b"synthetic transport password\n")
        .unwrap();
    let invited = invite.wait_with_output().unwrap();
    assert!(
        invited.status.success(),
        "invite device: {}",
        String::from_utf8_lossy(&invited.stderr)
    );
    let stdout = String::from_utf8(invited.stdout).unwrap();
    let code = stdout.lines().next().unwrap();
    assert!(code.starts_with("bj1/device/"), "device join code: {code}");
    assert!(JoinCode::parse(code).is_ok());
}
