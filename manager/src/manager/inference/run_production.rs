use super::*;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Child, Command, Stdio},
};

struct WorkerProcess(Child);
impl Drop for WorkerProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn private(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}
async fn wait_process(child: &mut Child, operation: &str) {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "{operation} exited with {status}");
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("fixture process completes");
}

#[tokio::test]
#[ignore = "Requires Linux x86_64, a systemd user manager, and BOTSTRAL_RUN_RUNTIME prepared by agent/runtime/setup.sh."]
async fn m04_run_production_worker_browser_file_process_and_artifact() {
    assert_eq!(std::env::consts::OS, "linux");
    assert_eq!(std::env::consts::ARCH, "x86_64");
    assert_ne!(
        unsafe { libc::geteuid() },
        0,
        "run as the guest account, not root"
    );
    let runtime = PathBuf::from(
        std::env::var_os("BOTSTRAL_RUN_RUNTIME")
            .expect("set the packaged browser runtime directory"),
    )
    .canonicalize()
    .unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let binary = std::env::var_os("BOTSTRAL_RUN_AGENT_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| source.join("target/release/botstral-agent"))
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
    let mut enroll = WorkerProcess(
        Command::new(&binary)
            .args(["join", "--state-dir"])
            .arg(&state)
            .arg(grant["code"].as_str().unwrap())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_process(&mut enroll.0, "worker enrollment").await;
    let workers = app.call("GET", "/v1/workers", Value::Null, 200).await;
    let worker: WorkerId = serde_json::from_value(workers[0]["id"].clone()).unwrap();
    let session: ComputerSession = serde_json::from_value(
        app.call(
            "POST",
            "/v1/computer-sessions",
            json!({"bot_id":app.bot,"worker_id":worker}),
            200,
        )
        .await,
    )
    .unwrap();
    let mut input = request(&app, session.id);
    let account = ProcessAccount {
        uid: unsafe { libc::geteuid() },
        gid: unsafe { libc::getegid() },
    };
    let computer = input.execution_policy.computer.as_mut().unwrap();
    computer.account = account;
    computer.runtime.sha256 =
        CertificateFingerprint::from_der(&fs::read(runtime.join("runtime.json")).unwrap());
    computer.limits.processes = 128;
    computer.limits.memory_bytes = 1024 * 1024 * 1024;
    computer.limits.runtime_seconds = 60;
    private(&state.join("computer.json"),&serde_json::to_vec(&json!({"account":account,"runtime":computer.runtime,"directory":runtime,"ceilings":computer.limits})).unwrap());
    let process = input.execution_policy.process.as_mut().unwrap();
    process.account = account;
    private(
        &state.join("process.json"),
        &serde_json::to_vec(
            &json!({"profile":process.profile,"account":account,"ceilings":process.limits}),
        )
        .unwrap(),
    );
    let log = fs::File::create(root.path().join("worker.log")).unwrap();
    let mut worker_process = WorkerProcess(
        Command::new(&binary)
            .args(["run", "--state-dir"])
            .arg(&state)
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .spawn()
            .unwrap(),
    );
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            assert!(
                worker_process.0.try_wait().unwrap().is_none(),
                "worker exited during probe"
            );
            let workers = app.call("GET", "/v1/workers", Value::Null, 200).await;
            if workers[0]["phase"] == "ready" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("production worker becomes ready after probes");
    let shared = root.path().join("shared");
    fs::create_dir(&shared).unwrap();
    let shared_file = shared.join("m04-note.txt");
    let mut tool: Value = serde_json::from_str(TOOL).unwrap();
    let args = &mut tool["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"];
    let mut operations: Value = serde_json::from_str(args.as_str().unwrap()).unwrap();
    operations["operations"][0]["url"] = format!("{}/page", fixture.endpoint).into();
    for index in [2, 3] {
        operations["operations"][index]["path"] = shared_file.to_str().unwrap().into();
    }
    *args = operations.to_string().into();
    fixture.reply(Reply {
        body: tool.to_string(),
        ..Reply::default()
    });
    input.limits.deadline = now() + 180;
    app.call("POST", "/v1/runs", json!(input), 200).await;
    let first = phase(&app, input.id, |p| {
        matches!(p, RunPhase::WaitingForTool { .. })
    })
    .await;
    let first_task = first.turns[0].intent.as_ref().unwrap().task;
    assert!(
        !shared_file.exists(),
        "no file effect before exact approval"
    );
    tool["choices"][0]["message"]["tool_calls"] = json!([{"id":"tool00002","type":"function","function":{"name":"process","arguments":json!({"executable":"/usr/bin/cat","args":[shared_file],"cwd":shared}).to_string()}}]);
    fixture.reply(Reply {
        body: tool.to_string(),
        ..Reply::default()
    });
    app.call(
        "PUT",
        &format!("/v1/tasks/{first_task}/approval"),
        json!({"expires_at":now()+120}),
        200,
    )
    .await;
    let second = tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            let view = view(&app, input.id).await;
            if view.turns.len() == 2
                && matches!(view.summary.phase, RunPhase::WaitingForTool { .. })
            {
                break view;
            }
            assert!(!view.summary.phase.terminal(), "{view:?}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        fs::read(&shared_file).unwrap(),
        b"durable shared guest file"
    );
    let second_task = second.turns[1].intent.as_ref().unwrap().task;
    fixture.reply(Reply::default());
    app.call(
        "PUT",
        &format!("/v1/tasks/{second_task}/approval"),
        json!({"expires_at":now()+60}),
        200,
    )
    .await;
    let final_view = tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            let view = view(&app, input.id).await;
            if view.summary.phase.terminal() {
                break view;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(final_view.summary.phase, RunPhase::Completed);
    assert_eq!(final_view.summary.accounting.admitted_turns, 3);
    assert!(
        final_view.turns[1]
            .intent
            .as_ref()
            .unwrap()
            .result
            .as_ref()
            .unwrap()
            .contains("durable shared guest file")
    );
    let task: TaskView = serde_json::from_value(
        app.call("GET", &format!("/v1/tasks/{first_task}"), Value::Null, 200)
            .await,
    )
    .unwrap();
    let output = task
        .outputs
        .iter()
        .find(|o| o.declaration.slot == "file")
        .unwrap();
    let ArtifactState::Available { id, .. } = output.state else {
        panic!("durable artifact missing")
    };
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!(
            "{}/v1/tasks/{first_task}/artifacts/{id}",
            app.address
        ))
        .bearer_auth(&app.token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.bytes().await.unwrap().as_ref(),
        b"durable shared guest file"
    );
    stop.send_replace(true);
    drop(worker_process);
    server.await.unwrap().unwrap();
    app.shutdown().await;
    no_plaintext(local.directory());
}
