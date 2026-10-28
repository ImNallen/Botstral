use super::*;

#[cfg(feature = "nonproduction-test-worker")]
mod permits;
#[cfg(feature = "nonproduction-test-worker")]
mod regrants;
mod snapshots;

#[tokio::test]
async fn conversation_append_order_survives_restart_and_retries() {
    let mut f = Fixture::new().await;
    let bot = BotId::new();
    assert_eq!(
        f.owner(
            "PUT",
            &format!("/v1/bots/{bot}"),
            json!({"name":"Ordering"})
        )
        .await
        .0,
        200
    );
    let conversation = ConversationId::new();
    assert_eq!(
        f.owner(
            "POST",
            "/v1/conversations",
            json!({"id":conversation,"bot":bot})
        )
        .await
        .0,
        200
    );
    let first = json!({"id":"ffffffff-ffff-4fff-bfff-ffffffffffff","conversation":conversation,"text":"first"});
    let second = json!({"id":"00000000-0000-4000-8000-000000000001","conversation":conversation,"text":"second"});
    for message in [&first, &second, &first] {
        assert_eq!(
            f.owner("POST", "/v1/messages", message.clone()).await,
            (200, message.clone())
        );
    }
    for restart in [false, true] {
        if restart {
            f.restart_manager(None).await;
        }
        let (status, snapshot) = f.owner("GET", "/v1/events/0/64", json!({})).await;
        assert_eq!(status, 200);
        assert_eq!(snapshot["state"]["messages"], json!([first, second]));
    }
    evidence(
        "conversation-order",
        json!({"snapshot_order":["first","second"],"retry_preserved_position":true,"restart_preserved_order":true}),
    );
    f.manager.stop().await;
}

#[tokio::test]
async fn durable_task_survives_manager_restart() {
    let mut f = Fixture::new().await;
    let grant = f.invitation(&f.worker, 600).await;
    let enrolled = worker_http(
        &f.worker,
        "POST",
        "/worker/v1/enroll",
        json!({"invitation":grant["invitation"]}),
    )
    .await;
    assert_eq!(enrolled.0, 201);
    let worker: WorkerId = serde_json::from_value(enrolled.1["worker_id"].clone()).unwrap();
    let session = f.create_session(worker, "Recovery").await;
    let id = TaskId::new();
    let request = json!({"id":id,"session_id":session.id,"conversation_id":null,"settings":{"model":"unavailable","max_turns":10,"max_tokens":1000,"deadline":4000000000i64,"max_cost_micros":1000},"work":{"type":"unavailable"}});
    let (status, original) = f.owner("POST", "/v1/tasks", request.clone()).await;
    assert_eq!(status, 200, "durable submission: {original}");
    assert_eq!(original["id"], json!(id));
    assert_eq!(f.owner("POST", "/v1/tasks", request).await.1, original);
    f.restart_manager(None).await;
    let recovered = f.owner("GET", &format!("/v1/tasks/{id}"), json!({})).await;
    assert_eq!(recovered, (200, original));
    f.manager.stop().await;
}

impl Fixture {
    #[cfg(feature = "nonproduction-test-worker")]
    pub(super) fn agent_state(&self) -> PathBuf {
        self.root.path().join("durable-agent")
    }
    #[cfg(feature = "nonproduction-test-worker")]
    pub(super) fn start_agent(&self, fault: Option<&str>) -> Process {
        let state = self.agent_state();
        if !state.exists() {
            fs::create_dir(&state).unwrap();
            fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
            private(
                &state.join("config.json"),
                &serde_json::to_vec(&self.worker).unwrap(),
            );
        }
        launch_fault(
            Path::new(env!("CARGO_BIN_EXE_botstral-test-agent")),
            &["run", "--state-dir", state.to_str().unwrap()],
            self.root.path(),
            fault,
        )
    }
    pub(super) async fn restart_manager(&mut self, fault: Option<&str>) {
        let _ = self.manager.0.kill();
        let _ = self.manager.0.wait();
        let p = self.root.path();
        let state = p.join("manager");
        let _ = fs::remove_file(state.join("listeners.json"));
        let port = self.worker.manager.rsplit(':').next().unwrap();
        self.manager = launch_fault(
            &manager_binary(),
            &[
                "--state-dir",
                state.to_str().unwrap(),
                "--listen",
                &self.listen.to_string(),
                "--setup-listen",
                "127.0.0.1:0",
                "--worker-listen",
                &format!("127.0.0.1:{port}"),
                "--worker-tls-cert",
                p.join("server.pem").to_str().unwrap(),
                "--worker-tls-key",
                p.join("server.key").to_str().unwrap(),
                "--worker-client-ca",
                p.join("ca.pem").to_str().unwrap(),
            ],
            p,
            fault,
        );
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                assert!(
                    self.manager.0.try_wait().unwrap().is_none(),
                    "manager restarted"
                );
                if state.join("listeners.json").exists() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            self.owner("GET", "/v1/owner", json!({})).await.0,
            401,
            "old login revoked on startup"
        );
        let (status, session) = local(
            self.listen,
            "POST",
            "/v1/sessions",
            None,
            json!({"password":"synthetic transport password"}),
        )
        .await;
        assert_eq!(status, 201);
        self.token = session["token"].as_str().unwrap().into();
    }
    pub(super) async fn durable_session(&self) -> ComputerSession {
        let grant = self.invitation(&self.worker, 600).await;
        let (status, enrolled) = worker_http(
            &self.worker,
            "POST",
            "/worker/v1/enroll",
            json!({"invitation":grant["invitation"]}),
        )
        .await;
        assert_eq!(status, 201);
        self.create_session(
            serde_json::from_value(enrolled["worker_id"].clone()).unwrap(),
            "Durable",
        )
        .await
    }
    #[cfg(feature = "nonproduction-test-worker")]
    pub(super) async fn submit(&self, session: &ComputerSession, work: WorkSpec) -> Value {
        let request = json!({"id":TaskId::new(),"session_id":session.id,"conversation_id":null,"settings":{"model":"unavailable","max_turns":10,"max_tokens":1000,"deadline":4000000000i64,"max_cost_micros":1000},"work":work});
        let (status, task) = self.owner("POST", "/v1/tasks", request).await;
        assert_eq!(status, 200, "task submitted {task}");
        task
    }
    #[cfg(feature = "nonproduction-test-worker")]
    pub(super) async fn approve(&self, task: &Value) -> Value {
        let (status, task) = self
            .owner(
                "PUT",
                &format!("/v1/tasks/{}/approval", task["id"].as_str().unwrap()),
                json!({"expires_at":epoch()+600}),
            )
            .await;
        assert_eq!(status, 200, "approval {task}");
        task
    }
    #[cfg(feature = "nonproduction-test-worker")]
    pub(super) async fn task_state(&self, task: &Value, state: &str) -> Value {
        let result = tokio::time::timeout(Duration::from_secs(12), async {
            loop {
                let (status, v) = self
                    .owner(
                        "GET",
                        &format!("/v1/tasks/{}", task["id"].as_str().unwrap()),
                        json!({}),
                    )
                    .await;
                assert_eq!(status, 200);
                if v["state"] == state {
                    break v;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await;
        match result {
            Ok(value) => value,
            Err(_) => {
                let last = self
                    .owner(
                        "GET",
                        &format!("/v1/tasks/{}", task["id"].as_str().unwrap()),
                        json!({}),
                    )
                    .await;
                let logs = fs::read_dir(self.root.path())
                    .unwrap()
                    .filter_map(Result::ok)
                    .filter(|e| e.path().extension().is_some_and(|v| v == "log"))
                    .map(|e| fs::read_to_string(e.path()).unwrap())
                    .collect::<Vec<_>>();
                panic!("task did not reach {state}: {last:?}, logs {logs:?}");
            }
        }
    }
    #[cfg(feature = "nonproduction-test-worker")]
    pub(super) fn effects(&self) -> (i64, i64) {
        let path = self.agent_state().join("synthetic-provider.sqlite3");
        if !path.exists() {
            return (0, 0);
        }
        let db = rusqlite::Connection::open(path).unwrap();
        db.query_row(
            "SELECT count(*),coalesce(sum(invocations),0) FROM effects",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    }
}
#[cfg(feature = "nonproduction-test-worker")]
fn epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
fn launch_fault(binary: &Path, args: &[&str], root: &Path, fault: Option<&str>) -> Process {
    let log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(root.join(format!("process-{}.log", RequestId::new())))
        .unwrap();
    let mut command = Command::new(binary);
    command
        .args(args)
        .stdout(log.try_clone().unwrap())
        .stderr(log);
    if let Some(fault) = fault {
        command.env("BOTSTRAL_FAULT", fault);
    } else {
        command.env_remove("BOTSTRAL_FAULT");
    }
    Process(command.spawn().unwrap())
}
#[cfg(feature = "nonproduction-test-worker")]
pub(super) async fn reached(process: &mut Process, state: &Path, point: &str) {
    tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            if let Some(status) = process.0.try_wait().unwrap() {
                assert!(!status.success());
                assert!(
                    state.join(format!("fault-{point}.reached")).exists(),
                    "fault marker {point}, logs {:?}",
                    fs::read_dir(state.parent().unwrap())
                        .unwrap()
                        .filter_map(Result::ok)
                        .filter(|e| e.path().extension().is_some_and(|v| v == "log"))
                        .map(|e| fs::read_to_string(e.path()).unwrap())
                        .collect::<Vec<_>>()
                );
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("fault reached");
}
fn evidence(name: &str, value: Value) {
    if let Some(path) = std::env::var_os("M03_EVIDENCE_DIR") {
        let path = PathBuf::from(path);
        fs::create_dir_all(&path).unwrap();
        fs::write(
            path.join(format!("{name}.json")),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
    }
}

#[cfg(feature = "nonproduction-test-worker")]
#[tokio::test]
async fn independent_effect_ledger_recovers_every_worker_crash_boundary() {
    for (point, observable, expected, count) in [
        ("acceptance_committed", true, "completed", 1),
        ("effect_committed", true, "completed", 1),
        ("effect_committed", false, "awaiting_attention", 1),
        ("outcome_committed", true, "completed", 1),
        ("permit_admitted", true, "awaiting_attention", 0),
    ] {
        let mut f = Fixture::new().await;
        let session = f.durable_session().await;
        let task = f
            .submit(
                &session,
                WorkSpec::TestEffect {
                    value: 7,
                    observable,
                },
            )
            .await;
        let approved = f.approve(&task).await;
        let mut daemon = f.start_agent(Some(point));
        reached(&mut daemon, &f.agent_state(), point).await;
        let before = f.effects();
        let mut daemon = f.start_agent(None);
        let result = f.task_state(&task, expected).await;
        assert_eq!(result["command"], approved["command"]);
        assert_eq!(f.effects(), (count, count));
        f.restart_manager(None).await;
        let recovered = f.task_state(&task, expected).await;
        assert_eq!(recovered["command"], result["command"]);
        assert_eq!(f.effects(), (count, count));
        evidence(
            &format!("worker-{point}-{observable}"),
            json!({"point":point,"observable":observable,"before":before,"after":f.effects(),"task":recovered}),
        );
        daemon.stop().await;
        f.manager.stop().await;
    }
}

#[cfg(feature = "nonproduction-test-worker")]
#[tokio::test]
async fn offline_cancellation_preserves_effects_until_stopped_confirmation() {
    let mut f = Fixture::new().await;
    let session = f.durable_session().await;
    let task = f
        .submit(
            &session,
            WorkSpec::TestEffect {
                value: 9,
                observable: true,
            },
        )
        .await;
    f.approve(&task).await;
    let mut daemon = f.start_agent(Some("effect_committed"));
    reached(&mut daemon, &f.agent_state(), "effect_committed").await;
    let path = format!("/v1/tasks/{}/cancel", task["id"].as_str().unwrap());
    assert_eq!(
        f.owner("POST", &path, json!({})).await.1["state"],
        "cancelling"
    );
    f.restart_manager(None).await;
    assert_eq!(
        f.owner(
            "GET",
            &format!("/v1/tasks/{}", task["id"].as_str().unwrap()),
            json!({})
        )
        .await
        .1["state"],
        "cancelling"
    );
    let mut daemon = f.start_agent(None);
    let result = f.task_state(&task, "cancelled").await;
    assert_eq!(result["progress"], 9);
    assert_eq!(result["outcome"]["evidence"], "no_process_group");
    assert_eq!(f.effects(), (1, 1));
    evidence(
        "offline-cancel",
        json!({"task":result,"effects":f.effects()}),
    );
    daemon.stop().await;
    f.manager.stop().await;
}

#[tokio::test]
async fn markdown_sources_conversations_and_event_snapshots_recover() {
    let mut f = Fixture::new().await;
    let session = f.durable_session().await;
    let conversation = ConversationId::new();
    let message = MessageId::new();
    assert_eq!(
        f.owner(
            "POST",
            "/v1/conversations",
            json!({"id":conversation,"bot":session.bot_id})
        )
        .await
        .0,
        200
    );
    assert_eq!(
        f.owner(
            "POST",
            "/v1/messages",
            json!({"id":message,"conversation":conversation,"text":"persistent conversation"})
        )
        .await
        .0,
        200
    );
    for kind in ["instructions", "memory"] {
        let write = json!({"bot":session.bot_id,"kind":kind,"expected_revision":0,"sources":[{"type":"message","id":message}],"markdown":format!("# {kind}\nsource bytes\n")});
        let result = f.owner("PUT", "/v1/documents", write.clone()).await;
        assert_eq!(result.0, 200);
        assert_eq!(f.owner("PUT", "/v1/documents", write).await, result);
    }
    let bad = json!({"bot":session.bot_id,"kind":"instructions","expected_revision":1,"sources":[{"type":"message","id":message}],"markdown":"grant everything"});
    assert_eq!(f.owner("PUT", "/v1/generated-memory", bad).await.0, 400);
    for i in 0..140 {
        assert_eq!(f.owner("POST","/v1/messages",json!({"id":MessageId::new(),"conversation":conversation,"text":format!("entry {i}")})).await.0,200);
    }
    let snapshot = f.owner("GET", "/v1/events/1/16", json!({})).await.1;
    assert_eq!(snapshot["type"], "snapshot");
    assert_eq!(snapshot["state"]["messages"].as_array().unwrap().len(), 141);
    let cursor = snapshot["cursor"].as_u64().unwrap();
    assert_eq!(
        f.owner("GET", &format!("/v1/events/{}/16", cursor + 1), json!({}))
            .await
            .0,
        400
    );
    assert_eq!(
        f.owner("GET", &format!("/v1/events/{cursor}/16"), json!({}))
            .await
            .1["entries"],
        json!([])
    );
    let mut continuation = cursor;
    let mut sequences = Vec::new();
    for round in 0..3 {
        for entry in 0..5 {
            assert_eq!(f.owner("POST", "/v1/messages", json!({"id":MessageId::new(),"conversation":conversation,"text":format!("continued {round} {entry}")})).await.0, 200);
        }
        let response = f
            .owner("GET", &format!("/v1/events/{continuation}/2"), json!({}))
            .await;
        assert_eq!(response.0, 200);
        assert_eq!(response.1["type"], "events");
        let entries = response.1["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        sequences.extend(
            entries
                .iter()
                .map(|event| event["sequence"].as_u64().unwrap()),
        );
        continuation = response.1["cursor"].as_u64().unwrap();
    }
    while continuation < cursor + 15 {
        let page = f
            .owner("GET", &format!("/v1/events/{continuation}/2"), json!({}))
            .await
            .1;
        let entries = page["entries"].as_array().unwrap();
        assert!(!entries.is_empty());
        sequences.extend(
            entries
                .iter()
                .map(|event| event["sequence"].as_u64().unwrap()),
        );
        continuation = page["cursor"].as_u64().unwrap();
    }
    assert_eq!(sequences, (cursor + 1..=cursor + 15).collect::<Vec<_>>());
    let messages_before_restart =
        f.owner("GET", "/v1/events/0/64", json!({})).await.1["state"]["messages"].clone();
    assert_eq!(messages_before_restart.as_array().unwrap().len(), 156);
    let state = f.root.path().join("manager");
    let source = fs::read(state.join(format!("memory-{}.md", session.bot_id))).unwrap();
    f.manager.stop().await;
    fs::write(state.join("memory-index.sqlite3"), b"damaged derived index").unwrap();
    f.restart_manager(None).await;
    assert_eq!(
        f.owner("GET", "/v1/events/0/64", json!({})).await.1["state"]["messages"],
        messages_before_restart
    );
    let found = f
        .owner(
            "POST",
            &format!("/v1/documents/search/{}", session.bot_id),
            json!({"query":"source bytes"}),
        )
        .await;
    assert_eq!(found.0, 200);
    assert_eq!(found.1.as_array().unwrap().len(), 2);
    assert_eq!(
        fs::read(state.join(format!("memory-{}.md", session.bot_id))).unwrap(),
        source
    );
    fs::remove_file(state.join("memory-index.sqlite3")).unwrap();
    assert_eq!(
        f.owner("POST", "/v1/documents/rebuild", json!({}))
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        2
    );
    fs::remove_file(state.join(format!("memory-{}.md", session.bot_id))).unwrap();
    assert_eq!(
        f.owner(
            "POST",
            &format!("/v1/documents/search/{}", session.bot_id),
            json!({"query":"source bytes"})
        )
        .await
        .1
        .as_array()
        .unwrap()
        .len(),
        1
    );
    evidence(
        "source-replay",
        json!({"snapshot_cursor":cursor,"messages_before_continuation":141,"messages_after_restart":156,"continuation_sequences":sequences,"source_preserved":true,"index_rebuilt":true}),
    );
    f.manager.stop().await;
}

#[tokio::test]
async fn derived_index_data_corruption_recovers_without_touching_sources() {
    let mut f = Fixture::new().await;
    let session = f.durable_session().await;
    let write = json!({"bot":session.bot_id,"kind":"memory","expected_revision":0,"sources":[{"type":"external","reference":"https://example.test/source"}],"markdown":"authoritative source bytes"});
    assert_eq!(f.owner("PUT", "/v1/documents", write).await.0, 200);
    let directory = f.root.path().join("manager");
    let source_path = directory.join(format!("memory-{}.md", session.bot_id));
    let source = fs::read(&source_path).unwrap();
    f.manager.stop().await;
    let index_path = directory.join("memory-index.sqlite3");
    let index = rusqlite::Connection::open(&index_path).unwrap();
    let root: u32 = index
        .query_row(
            "SELECT rootpage FROM sqlite_schema WHERE name='documents'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let page_size: u32 = index
        .pragma_query_value(None, "page_size", |row| row.get(0))
        .unwrap();
    drop(index);
    let mut bytes = fs::read(&index_path).unwrap();
    bytes[((root - 1) * page_size) as usize] = 0xff;
    fs::write(&index_path, bytes).unwrap();
    let corrupt = rusqlite::Connection::open(&index_path).unwrap();
    corrupt.execute_batch("CREATE TABLE IF NOT EXISTS documents(name TEXT PRIMARY KEY, body TEXT NOT NULL) STRICT;").unwrap();
    let error = corrupt.execute("DELETE FROM documents", []).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("database disk image is malformed")
    );
    println!("Schema creation succeeded; cache rewrite failed with {error}.");
    drop(corrupt);
    f.restart_manager(None).await;
    let found = f
        .owner(
            "POST",
            &format!("/v1/documents/search/{}", session.bot_id),
            json!({"query":"authoritative source bytes"}),
        )
        .await;
    assert_eq!(found.0, 200);
    assert_eq!(found.1.as_array().unwrap().len(), 1);
    assert_eq!(fs::read(&source_path).unwrap(), source);
    let malformed = b"malformed source without document metadata";
    fs::write(&source_path, malformed).unwrap();
    assert_eq!(
        f.owner("POST", "/v1/documents/rebuild", json!({})).await.0,
        400
    );
    assert_eq!(fs::read(&source_path).unwrap(), malformed);
    fs::write(&source_path, &source).unwrap();
    assert_eq!(
        f.owner("POST", "/v1/documents/rebuild", json!({})).await.0,
        200
    );
    evidence(
        "index-data-corruption",
        json!({"schema_survived":true,"damaged_data_page_rebuilt":true,"source_preserved":true,"malformed_source_rejected":true}),
    );
    f.manager.stop().await;
}

#[cfg(feature = "nonproduction-test-worker")]
#[tokio::test]
async fn delayed_claim_rejects_revocation_policy_and_expiry() {
    let mut f = Fixture::new().await;
    let session = f.durable_session().await;
    let mut daemon = f.start_agent(None);
    f.wait_phase("ready", 1).await;
    private(&f.agent_state().join("pause-before-claim"), b"pause");
    for mode in ["revocation", "permission", "expiry", "arguments"] {
        let task = f
            .submit(
                &session,
                WorkSpec::TestEffect {
                    value: 4,
                    observable: true,
                },
            )
            .await;
        f.task_state(&task, "awaiting_approval").await;
        let approval = if mode == "expiry" {
            let v = f
                .owner(
                    "PUT",
                    &format!("/v1/tasks/{}/approval", task["id"].as_str().unwrap()),
                    json!({"expires_at":epoch()+1}),
                )
                .await;
            assert_eq!(v.0, 200);
            v.1
        } else {
            f.approve(&task).await
        };
        match mode {
            "revocation" => {
                assert_eq!(
                    f.owner(
                        "DELETE",
                        &format!("/v1/tasks/{}/approval", task["id"].as_str().unwrap()),
                        json!({})
                    )
                    .await
                    .0,
                    200
                );
            }
            "permission" => {
                assert_eq!(
                    f.owner(
                        "PUT",
                        &format!("/v1/bots/{}/permission", session.bot_id),
                        json!({"expected_revision":1,"allow":true})
                    )
                    .await
                    .0,
                    200
                );
            }
            "expiry" => {
                while epoch()
                    < approval["approval"]["binding"]["expires_at"]
                        .as_i64()
                        .unwrap()
                {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            }
            "arguments" => {
                let mut request = task["request"].clone();
                request["work"]["value"] = 5.into();
                assert_eq!(f.owner("POST", "/v1/tasks", request).await.0, 409);
                assert_eq!(
                    f.owner(
                        "DELETE",
                        &format!("/v1/tasks/{}/approval", task["id"].as_str().unwrap()),
                        json!({})
                    )
                    .await
                    .0,
                    200
                );
            }
            _ => unreachable!(),
        }
        fs::remove_file(f.agent_state().join("pause-before-claim")).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(f.effects(), (0, 0), "invalid authority {mode}");
        let (status, task) = f
            .owner(
                "POST",
                &format!("/v1/tasks/{}/cancel", task["id"].as_str().unwrap()),
                json!({}),
            )
            .await;
        assert_eq!(status, 200);
        f.task_state(&task, "cancelled").await;
        private(&f.agent_state().join("pause-before-claim"), b"pause");
    }
    let task = f
        .submit(
            &session,
            WorkSpec::TestEffect {
                value: 1,
                observable: true,
            },
        )
        .await;
    assert_eq!(
        local(
            f.listen,
            "PUT",
            &format!("/v1/tasks/{}/approval", task["id"].as_str().unwrap()),
            None,
            json!({"expires_at":epoch()+600})
        )
        .await
        .0,
        401
    );
    evidence(
        "delayed-authority",
        json!({"effects":f.effects(),"invalidations":["revocation","permission","expiry","changed_arguments"]}),
    );
    daemon.stop().await;
    f.manager.stop().await;
}

#[cfg(feature = "nonproduction-test-worker")]
#[tokio::test]
async fn every_approval_binding_field_and_single_use_identity_are_checked() {
    let mut f = Fixture::new().await;
    let session = f.durable_session().await;
    let task = f
        .submit(
            &session,
            WorkSpec::TestEffect {
                value: 1,
                observable: true,
            },
        )
        .await;
    let approved = f.approve(&task).await;
    let other = f
        .submit(
            &session,
            WorkSpec::TestEffect {
                value: 2,
                observable: true,
            },
        )
        .await;
    let mut socket = transport::connect_socket(&f.worker).await.unwrap();
    wire(&mut socket, WorkerMessage::Hello(Hello::current(true))).await;
    let ManagerMessage::Welcome {
        generation,
        sessions,
        ..
    } = read(&mut socket).await
    else {
        panic!("welcome")
    };
    wire(&mut socket, WorkerMessage::SnapshotApplied { generation }).await;
    let incarnation = JournalId::new();
    wire(
        &mut socket,
        WorkerMessage::RecoveryHello {
            generation,
            incarnation,
            high_water: 0,
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
            incarnation,
            offset: 0,
            total: 0,
            entries: vec![],
        },
    )
    .await;
    let mut fence = None;
    for _ in 0..3 {
        match read(&mut socket).await {
            ManagerMessage::Delivery(d)
                if d.body.task.to_string() == task["id"].as_str().unwrap() =>
            {
                fence = Some(d.fence)
            }
            ManagerMessage::Delivery(_) | ManagerMessage::RecoveryReady { .. } => {}
            m => panic!("recovery {m:?}"),
        }
    }
    let fence = fence.unwrap();
    assert_eq!(sessions[0].fence(generation), fence);
    let original = approved["approval"]["binding"].clone();
    for field in [
        "owner",
        "bot",
        "task",
        "operation",
        "arguments",
        "permission_revision",
        "approval",
        "execution",
        "expires_at",
    ] {
        let mut binding = original.clone();
        binding[field] = match field {
            "task" => other["id"].clone(),
            "arguments" => json!("a".repeat(64)),
            "permission_revision" => json!(99),
            "expires_at" => json!(epoch() - 1),
            _ => json!(RequestId::new().to_string()),
        };
        wire(
            &mut socket,
            WorkerMessage::Claim {
                incarnation,
                fence,
                binding: serde_json::from_value(binding).unwrap(),
            },
        )
        .await;
        assert!(
            matches!(read(&mut socket).await, ManagerMessage::ClaimDenied { .. }),
            "field {field}"
        );
    }
    let binding: ExecutionBinding = serde_json::from_value(original).unwrap();
    wire(
        &mut socket,
        WorkerMessage::Claim {
            incarnation,
            fence,
            binding: binding.clone(),
        },
    )
    .await;
    assert!(matches!(
        read(&mut socket).await,
        ManagerMessage::Permit { .. }
    ));
    wire(
        &mut socket,
        WorkerMessage::Claim {
            incarnation,
            fence,
            binding,
        },
    )
    .await;
    assert!(matches!(
        read(&mut socket).await,
        ManagerMessage::ClaimDenied { .. }
    ));
    evidence(
        "binding-fields",
        json!({"fields":9,"single_use":true,"effects":f.effects()}),
    );
    socket.close(None).await.unwrap();
    f.manager.stop().await;
}

#[cfg(feature = "nonproduction-test-worker")]
#[tokio::test]
async fn published_markdown_recovers_each_file_index_event_crash_window() {
    for point in [
        "document_before_rename",
        "document_after_rename",
        "document_after_index",
    ] {
        let mut f = Fixture::new().await;
        let session = f.durable_session().await;
        let write = json!({"bot":session.bot_id,"kind":"memory","expected_revision":0,"sources":[{"type":"external","reference":"https://example.test/source"}],"markdown":"# durable source\nexact bytes\n"});
        f.restart_manager(Some(point)).await;
        let listen = f.listen;
        let token = f.token.clone();
        let body = write.clone();
        let response =
            tokio::spawn(
                async move { local(listen, "PUT", "/v1/documents", Some(&token), body).await },
            );
        let directory = f.root.path().join("manager");
        reached(&mut f.manager, &directory, point).await;
        assert!(response.await.is_err());
        let published = directory
            .join(format!("memory-{}.md", session.bot_id))
            .exists();
        assert_eq!(published, point != "document_before_rename");
        f.restart_manager(None).await;
        let replay = f.owner("GET", "/v1/events/0/16", json!({})).await.1;
        assert_eq!(
            replay["state"]["documents"].as_array().unwrap().len(),
            usize::from(published)
        );
        let written = f.owner("PUT", "/v1/documents", write.clone()).await;
        assert_eq!(written.0, 200);
        assert_eq!(written.1["revision"], 1);
        let before = f.owner("GET", "/v1/events/0/16", json!({})).await.1["cursor"].clone();
        assert_eq!(f.owner("PUT", "/v1/documents", write).await, written);
        assert_eq!(
            f.owner("GET", "/v1/events/0/16", json!({})).await.1["cursor"],
            before,
            "publication event deduplicated"
        );
        evidence(
            point,
            json!({"published_before_restart":published,"revision":1,"publication_cursor":before}),
        );
        f.manager.stop().await;
    }
}

#[cfg(feature = "nonproduction-test-worker")]
#[tokio::test]
async fn manager_claim_and_outbox_crashes_preserve_original_intent() {
    let mut f = Fixture::new().await;
    let session = f.durable_session().await;
    let request = json!({"id":TaskId::new(),"session_id":session.id,"conversation_id":null,"settings":{"model":"unavailable","max_turns":10,"max_tokens":1000,"deadline":4000000000i64,"max_cost_micros":1000},"work":{"type":"test_effect","value":2,"observable":true}});
    f.restart_manager(Some("task_committed")).await;
    let listen = f.listen;
    let token = f.token.clone();
    let body = request.clone();
    let response =
        tokio::spawn(async move { local(listen, "POST", "/v1/tasks", Some(&token), body).await });
    reached(
        &mut f.manager,
        &f.root.path().join("manager"),
        "task_committed",
    )
    .await;
    assert!(response.await.is_err());
    f.restart_manager(None).await;
    let result = f.owner("POST", "/v1/tasks", request).await;
    assert_eq!(result.0, 200);
    let task = result.1;
    f.approve(&task).await;
    f.restart_manager(Some("claim_committed")).await;
    let mut daemon = f.start_agent(None);
    reached(
        &mut f.manager,
        &f.root.path().join("manager"),
        "claim_committed",
    )
    .await;
    assert_eq!(f.effects(), (0, 0));
    f.restart_manager(None).await;
    let attention = f.task_state(&task, "awaiting_attention").await;
    assert_eq!(attention["command"], task["command"]);
    assert_eq!(f.effects(), (0, 0));
    evidence(
        "manager-outbox-claim",
        json!({"task":attention,"effects":f.effects()}),
    );
    daemon.stop().await;
    f.manager.stop().await;
}

#[cfg(feature = "nonproduction-test-worker")]
#[tokio::test]
async fn journal_replacement_after_unacknowledged_acceptance_never_restarts_work() {
    let mut f = Fixture::new().await;
    let session = f.durable_session().await;
    let task = f
        .submit(
            &session,
            WorkSpec::TestEffect {
                value: 3,
                observable: true,
            },
        )
        .await;
    f.approve(&task).await;
    let mut daemon = f.start_agent(Some("acceptance_committed"));
    reached(&mut daemon, &f.agent_state(), "acceptance_committed").await;
    let db = rusqlite::Connection::open(f.root.path().join("manager/manager.sqlite3")).unwrap();
    assert_eq!(
        db.query_row::<i64, _, _>("SELECT received FROM worker_journals", [], |r| r.get(0))
            .unwrap(),
        0
    );
    drop(db);
    for name in ["agent.sqlite3", "agent.sqlite3-wal", "agent.sqlite3-shm"] {
        let _ = fs::remove_file(f.agent_state().join(name));
    }
    let mut daemon = f.start_agent(None);
    let attention = f.task_state(&task, "awaiting_attention").await;
    assert_eq!(f.effects(), (0, 0));
    assert_eq!(attention["outcome"]["reason"], "journal_replaced");
    let cancelled = f
        .owner(
            "POST",
            &format!("/v1/tasks/{}/cancel", task["id"].as_str().unwrap()),
            json!({}),
        )
        .await;
    assert_eq!(cancelled.0, 200);
    let stopped = f.task_state(&task, "cancelled").await;
    assert!(
        stopped["effect_evidence"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["type"] == "uncertain" && e["reason"] == "journal_replaced")
    );
    evidence(
        "journal-replacement",
        json!({"task":attention,"manager_received_acceptance":false,"effects":f.effects()}),
    );
    daemon.stop().await;
    f.manager.stop().await;
}

#[cfg(feature = "nonproduction-test-worker")]
#[tokio::test]
async fn recovery_pages_and_lost_report_receipts_are_bounded_and_idempotent() {
    let mut f = Fixture::new().await;
    let session = f.durable_session().await;
    let mut tasks = Vec::new();
    for _ in 0..24 {
        tasks.push(
            f.submit(
                &session,
                WorkSpec::TestEffect {
                    value: 1,
                    observable: true,
                },
            )
            .await,
        );
    }
    f.restart_manager(Some("reports_committed")).await;
    let mut daemon = f.start_agent(None);
    reached(
        &mut f.manager,
        &f.root.path().join("manager"),
        "reports_committed",
    )
    .await;
    daemon.stop().await;
    f.restart_manager(None).await;
    let mut daemon = f.start_agent(None);
    for task in &tasks {
        f.task_state(task, "awaiting_approval").await;
    }
    daemon.stop().await;
    let db = rusqlite::Connection::open(f.agent_state().join("agent.sqlite3")).unwrap();
    let incarnation: String = db
        .query_row("SELECT incarnation FROM metadata", [], |r| r.get(0))
        .unwrap();
    let high: i64 = db
        .query_row("SELECT head FROM metadata", [], |r| r.get(0))
        .unwrap();
    drop(db);
    assert_eq!(high, 48);
    let manager =
        rusqlite::Connection::open(f.root.path().join("manager/manager.sqlite3")).unwrap();
    let receipts: i64 = manager
        .query_row("SELECT count(*) FROM report_receipts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(receipts, 48);
    let mut socket = transport::connect_socket(&f.worker).await.unwrap();
    wire(&mut socket, WorkerMessage::Hello(Hello::current(true))).await;
    let ManagerMessage::Welcome { generation, .. } = read(&mut socket).await else {
        panic!("welcome")
    };
    wire(&mut socket, WorkerMessage::SnapshotApplied { generation }).await;
    let incarnation: JournalId = incarnation.parse().unwrap();
    wire(
        &mut socket,
        WorkerMessage::RecoveryHello {
            generation,
            incarnation,
            high_water: high as u64,
        },
    )
    .await;
    for offset in [0, 16] {
        assert!(
            matches!(read(&mut socket).await,ManagerMessage::InventoryRequest{offset:n} if n==offset)
        );
        let entries = tasks
            .iter()
            .skip(offset as usize)
            .take(16)
            .map(|task| JournalEntry {
                body: serde_json::from_value(task["command"].clone()).unwrap(),
                phase: JournalPhase::Prepared,
            })
            .collect::<Vec<_>>();
        wire(
            &mut socket,
            WorkerMessage::Inventory {
                generation,
                incarnation,
                offset,
                total: 24,
                entries,
            },
        )
        .await;
    }
    assert!(matches!(
        read(&mut socket).await,
        ManagerMessage::RecoveryReady { .. }
    ));
    let before: i64 = manager
        .query_row("SELECT sequence FROM event_head", [], |r| r.get(0))
        .unwrap();
    let report: DurableReport = serde_json::from_str(
        &manager
            .query_row::<String, _, _>(
                "SELECT body FROM report_receipts WHERE sequence=1",
                [],
                |r| r.get(0),
            )
            .unwrap(),
    )
    .unwrap();
    wire(
        &mut socket,
        WorkerMessage::Reports {
            generation,
            incarnation,
            high_water: high as u64,
            reports: vec![report],
        },
    )
    .await;
    loop {
        if matches!(
            read(&mut socket).await,
            ManagerMessage::ReportAck { through: 48 }
        ) {
            break;
        }
    }
    assert_eq!(
        manager
            .query_row::<i64, _, _>("SELECT sequence FROM event_head", [], |r| r.get(0))
            .unwrap(),
        before
    );
    evidence(
        "report-pagination",
        json!({"tasks":24,"reports":48,"configured_page_limit":REPORT_PAGE,"receipt_replay_created_events":0,"effects":f.effects()}),
    );
    socket.close(None).await.unwrap();
    f.manager.stop().await;
}
