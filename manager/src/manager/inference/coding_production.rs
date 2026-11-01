use super::*;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Stdio},
};

fn private(path: &Path, value: &impl serde::Serialize) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}
struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
async fn task(app: &App, id: TaskId, expected: TaskState) -> TaskView {
    tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            let value: TaskView = serde_json::from_value(
                app.call("GET", &format!("/v1/tasks/{id}"), Value::Null, 200)
                    .await,
            )
            .unwrap();
            if value.state == expected {
                return value;
            }
            assert!(
                !value.state.terminal() && value.state != TaskState::AwaitingAttention,
                "unexpected task {value:?}"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("production coding task completes")
}
fn native_result(task: &TaskView) -> VibeResult {
    let captured = &task
        .process
        .as_ref()
        .unwrap()
        .result
        .as_ref()
        .unwrap()
        .stdout;
    assert!(captured.complete && !captured.truncated);
    let bytes = (0..captured.hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&captured.hex[i..i + 2], 16).unwrap())
        .collect::<Vec<_>>();
    serde_json::from_slice(&bytes).unwrap()
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "Requires Linux systemd, BOTSTRAL_VIBE_RUNTIME=Vibe 2.24.2 and BOTSTRAL_RUN_AGENT_BIN=production agent."]
async fn m04_coding_production_worker_pinned_vibe() {
    assert_eq!(std::env::consts::OS, "linux");
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let runtime = PathBuf::from(std::env::var_os("BOTSTRAL_VIBE_RUNTIME").unwrap())
        .canonicalize()
        .unwrap();
    let binary = PathBuf::from(std::env::var_os("BOTSTRAL_RUN_AGENT_BIN").unwrap())
        .canonicalize()
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    let local = private_local(&root);
    let fixture = Fixture::start().await;
    let app = App::start(local.directory().into(), &fixture).await;
    app.initialize().await;
    let identity_dir = root.path().join("manager-identity");
    fs::create_dir(&identity_dir).unwrap();
    fs::set_permissions(&identity_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let identity = botstral_link::tls::ensure_identity(&identity_dir, "manager-identity").unwrap();
    let listener = crate::worker_http::WorkerListener::bind(crate::WorkerTls {
        listen: "127.0.0.1:0".parse().unwrap(),
        certificate: identity.certificate,
        key: identity.key,
    })
    .await
    .unwrap();
    let bound = listener.local_addr().unwrap();
    app.manager
        .set_join_advertisement(
            botstral_link::ManagerEndpoint::parse(&bound.to_string()).unwrap(),
            identity.pin,
        )
        .await
        .unwrap();
    let (stop, stopped) = watch::channel(false);
    let server = tokio::spawn(listener.serve(app.manager.clone(), stopped));
    let grant = app
        .call(
            "POST",
            "/v1/worker-invitations",
            json!({"lifetime_seconds":600}),
            201,
        )
        .await;
    let state = root.path().join("agent");
    assert!(
        Command::new(&binary)
            .args(["join", "--state-dir"])
            .arg(&state)
            .arg(grant["code"].as_str().unwrap())
            .stdout(Stdio::null())
            .status()
            .unwrap()
            .success()
    );
    let workers = app.call("GET", "/v1/workers", Value::Null, 200).await;
    let session: ComputerSession = serde_json::from_value(
        app.call(
            "POST",
            "/v1/computer-sessions",
            json!({"bot_id":app.bot,"worker_id":workers[0]["id"]}),
            200,
        )
        .await,
    )
    .unwrap();
    let account = ProcessAccount {
        uid: unsafe { libc::geteuid() },
        gid: unsafe { libc::getegid() },
    };
    let resources = ProcessLimits {
        runtime_seconds: 120,
        output_bytes: 1024,
        processes: 32,
        memory_bytes: 268435456,
    };
    private(
        &state.join("process.json"),
        &json!({"profile":"linux_systemd_v1","account":account,"ceilings":resources}),
    );
    let manifest = root.path().join("vibe-runtime.json");
    assert!(
        Command::new("/usr/bin/python3")
            .arg(source.join("agent/vibe/seal.py"))
            .arg("--runtime")
            .arg(&runtime)
            .arg("--manifest")
            .arg(&manifest)
            .stdout(Stdio::null())
            .status()
            .unwrap()
            .success()
    );
    let digest = CertificateFingerprint::from_der(&fs::read(&manifest).unwrap());
    private(
        &state.join("vibe.json"),
        &json!({"account":account,"directory":runtime,"manifest":manifest,"runtime":digest,"ceilings":resources}),
    );
    let log = fs::File::create(root.path().join("worker.log")).unwrap();
    let mut worker = Child(
        Command::new(&binary)
            .args(["run", "--state-dir"])
            .arg(&state)
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .spawn()
            .unwrap(),
    );
    tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            assert!(worker.0.try_wait().unwrap().is_none(), "worker exited");
            let session = app
                .call(
                    "GET",
                    &format!("/v1/computer-sessions/{}", session.id),
                    Value::Null,
                    200,
                )
                .await;
            if session["status"] == "confirmed" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    let file = workspace.join("answer.py");
    fs::write(&file, b"def answer():\n    return 0\n").unwrap();
    *fixture.state.coding_path.lock().unwrap() = Some(file.to_string_lossy().into_owned());
    let spec = VibeSpec {
        policy: VibePolicy {
            connectors: None,
            workspace: CodingWorkspace {
                id: CodingWorkspaceId::new(),
                path: workspace.to_string_lossy().into_owned(),
            },
            runtime: digest,
            account,
            resources,
            configuration_revision: 1,
            model_id: "mistral-small-2506".into(),
            access: VibeAccess::EditFilesAsGuestAccount,
            outputs: vec![OutputDeclaration {
                slot: "answer".into(),
                name: "answer.py".into(),
                media_type: MediaType::TextPlain,
                max_bytes: 4096,
            }],
        },
        session: VibeSession::New,
        prompt: "Edit answer.py.".into(),
        limits: RunLimits {
            deadline: now() + 180,
            max_turns: 3,
            max_total_tokens: 200000,
            max_cost_microusd: 200000,
            max_output_tokens: 64,
            max_operations: 1,
            max_concurrent_operations: 1,
        },
    };
    let mut input = SubmitTask {
        id: TaskId::new(),
        session_id: session.id,
        conversation_id: None,
        settings: TaskSettings {
            model: "mistral-small-2506".into(),
            max_turns: 3,
            max_tokens: 200000,
            deadline: spec.limits.deadline,
            max_cost_micros: 200000,
        },
        work: WorkSpec::Vibe {
            spec: Box::new(spec),
        },
    };
    app.call("POST", "/v1/tasks", json!(input), 200).await;
    assert_eq!(fixture.chats(), 0);
    assert_eq!(fs::read(&file).unwrap(), b"def answer():\n    return 0\n");
    app.call(
        "PUT",
        &format!("/v1/tasks/{}/approval", input.id),
        json!({"expires_at":now()+120}),
        200,
    )
    .await;
    let finished = task(&app, input.id, TaskState::Completed).await;
    assert_eq!(fs::read(&file).unwrap(), b"def answer():\n    return 42\n");
    assert_eq!(fixture.chats(), 2);
    let native = native_result(&finished);
    assert_eq!(native.approval_denied, 0);
    assert_eq!(native.tool_errors, 0);
    app.call("POST", "/v1/tasks", json!(input), 200).await;
    assert_eq!(fixture.chats(), 2);
    let first = input.id;
    input.id = TaskId::new();
    if let WorkSpec::Vibe { spec } = &mut input.work {
        spec.session = VibeSession::Resume {
            task: first,
            native_session: native.native_session,
        };
        spec.prompt = "Summarize the previous edit.".into();
    }
    app.call("POST", "/v1/tasks", json!(input), 200).await;
    app.call(
        "PUT",
        &format!("/v1/tasks/{}/approval", input.id),
        json!({"expires_at":now()+120}),
        200,
    )
    .await;
    task(&app, input.id, TaskState::Completed).await;
    assert_eq!(fixture.chats(), 3);
    let completed_calls = app
        .call(
            "GET",
            &format!("/v1/tasks/{first}/coding-calls"),
            Value::Null,
            200,
        )
        .await;
    assert_eq!(completed_calls.as_array().unwrap().len(), 2);
    assert!(
        completed_calls
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["phase"] == "settled")
    );
    let completed = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let value = task(&app, first, TaskState::Completed).await;
            if value.outputs.iter().any(|output| {
                output.declaration.slot == "answer"
                    && matches!(output.state, ArtifactState::Available { .. })
            }) {
                break value;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("completed coding artifact becomes available");
    let denied_workspace = root.path().join("denied-workspace");
    fs::create_dir(&denied_workspace).unwrap();
    let outside = root.path().join("outside.py");
    fs::write(&outside, b"def answer():\n    return 0\n").unwrap();
    *fixture.state.coding_path.lock().unwrap() = Some(outside.to_string_lossy().into_owned());
    input.id = TaskId::new();
    if let WorkSpec::Vibe { spec } = &mut input.work {
        spec.policy.workspace = CodingWorkspace {
            id: CodingWorkspaceId::new(),
            path: denied_workspace.to_string_lossy().into_owned(),
        };
        spec.session = VibeSession::New;
        spec.prompt = "Attempt the proposed edit.".into();
    }
    app.call("POST", "/v1/tasks", json!(input), 200).await;
    app.call(
        "PUT",
        &format!("/v1/tasks/{}/approval", input.id),
        json!({"expires_at":now()+120}),
        200,
    )
    .await;
    let denied = task(&app, input.id, TaskState::Failed).await;
    let result = native_result(&denied);
    assert_eq!(result.exit_code, 0);
    assert!(result.approval_denied > 0 && result.tool_errors > 0);
    assert_eq!(
        fs::read(&outside).unwrap(),
        b"def answer():\n    return 0\n"
    );
    assert_eq!(result.calls, 1);
    assert_eq!(fixture.chats(), 4);
    let cancel_workspace = root.path().join("cancel-workspace");
    fs::create_dir(&cancel_workspace).unwrap();
    input.id = TaskId::new();
    if let WorkSpec::Vibe { spec } = &mut input.work {
        spec.policy.workspace = CodingWorkspace {
            id: CodingWorkspaceId::new(),
            path: cancel_workspace.to_string_lossy().into_owned(),
        };
    }
    let pending: TaskView =
        serde_json::from_value(app.call("POST", "/v1/tasks", json!(input), 200).await).unwrap();
    app.call(
        "PUT",
        &format!("/v1/tasks/{}/approval", input.id),
        json!({"expires_at":now()+120}),
        200,
    )
    .await;
    let native_binary = runtime.join("bin/vibe");
    let group = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let ready = state
                .join("processes")
                .join(pending.command.execution.to_string())
                .join("ready.json");
            if let Ok(bytes) = fs::read(ready) {
                let ready: Value = serde_json::from_slice(&bytes).unwrap();
                let group = Path::new("/sys/fs/cgroup").join(
                    ready["scope"]["cgroup"]
                        .as_str()
                        .unwrap()
                        .trim_start_matches('/'),
                );
                let pids = fs::read_to_string(group.join("cgroup.procs")).unwrap_or_default();
                if pids.split_whitespace().any(|pid| {
                    fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|arguments| {
                        arguments
                            .split(|byte| *byte == 0)
                            .any(|arg| arg == native_binary.as_os_str().as_encoded_bytes())
                    })
                }) {
                    break group;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("actual Vibe CLI observed before cancellation");
    app.call(
        "POST",
        &format!("/v1/tasks/{}/cancel", input.id),
        json!({}),
        200,
    )
    .await;
    task(&app, input.id, TaskState::Cancelled).await;
    match fs::read_to_string(group.join("cgroup.events")) {
        Ok(events) => assert!(events.lines().any(|line| line == "populated 0")),
        Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::NotFound),
    }
    let before_stall = fixture.chats();
    let stalled_workspace = root.path().join("stalled-workspace");
    fs::create_dir(&stalled_workspace).unwrap();
    input.id = TaskId::new();
    input.settings.deadline = now() + 20;
    if let WorkSpec::Vibe { spec } = &mut input.work {
        spec.policy.workspace = CodingWorkspace {
            id: CodingWorkspaceId::new(),
            path: stalled_workspace.to_string_lossy().into_owned(),
        };
        spec.limits.deadline = input.settings.deadline;
    }
    fixture.reply(Reply {
        gate: Some(Arc::new(Semaphore::new(0))),
        ..Reply::default()
    });
    app.call("POST", "/v1/tasks", json!(input), 200).await;
    app.call(
        "PUT",
        &format!("/v1/tasks/{}/approval", input.id),
        json!({"expires_at":input.settings.deadline}),
        200,
    )
    .await;
    fixture.reached(before_stall + 1).await;
    assert!(
        !fs::read_to_string(root.path().join("worker.log"))
            .unwrap()
            .contains(KEY)
    );
    stop.send_replace(true);
    server.await.unwrap().unwrap();
    let in_flight = app
        .call(
            "GET",
            &format!("/v1/tasks/{}/coding-calls", input.id),
            Value::Null,
            200,
        )
        .await;
    assert_eq!(in_flight[0]["phase"], "sent");
    tokio::time::timeout(Duration::from_secs(3), app.shutdown())
        .await
        .expect("coding HTTP jobs drain within manager shutdown bound");
    let db = rusqlite::Connection::open(local.directory().join("manager.sqlite3")).unwrap();
    let stored: String = db
        .query_row(
            "SELECT view FROM coding_calls WHERE task=?1",
            [input.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    let stored: CodingCallView = serde_json::from_str(&stored).unwrap();
    assert_eq!(stored.phase, CodingCallPhase::Unknown);
    assert_eq!(stored.accounting, InferenceAccounting::Held);
    drop(db);
    assert!(
        Command::new("kill")
            .args(["-TERM", &worker.0.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if let Some(status) = worker.0.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    let restarted = App::start(local.directory().into(), &fixture).await;
    let calls = restarted
        .call(
            "GET",
            &format!("/v1/tasks/{}/coding-calls", input.id),
            Value::Null,
            200,
        )
        .await;
    assert_eq!(calls.as_array().unwrap().len(), 1);
    assert_eq!(calls[0]["phase"], "unknown");
    assert_eq!(calls[0]["accounting"]["type"], "held");
    let restored = task(&restarted, first, TaskState::Completed).await;
    assert_eq!(restored.process, completed.process);
    assert_eq!(restored.outputs, completed.outputs);
    assert_eq!(
        restarted
            .call(
                "GET",
                &format!("/v1/tasks/{first}/coding-calls"),
                Value::Null,
                200
            )
            .await,
        completed_calls
    );
    let artifact = restored
        .outputs
        .iter()
        .find(|o| o.declaration.slot == "answer")
        .unwrap();
    let ArtifactState::Available { id, .. } = artifact.state else {
        panic!("completed coding artifact unavailable after restart");
    };
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!(
            "{}/v1/tasks/{first}/artifacts/{id}",
            restarted.address
        ))
        .bearer_auth(&restarted.token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        response.bytes().await.unwrap().as_ref(),
        b"def answer():\n    return 42\n"
    );
    assert_eq!(fixture.chats(), before_stall + 1);
    restarted.shutdown().await;
}
