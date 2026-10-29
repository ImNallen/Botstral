use super::*;
use crate::worker_http::VerifiedIdentity;
use std::collections::BTreeMap;

const TOOL: &str = include_str!("../../../tests/fixtures/m04/tool-call.json");

struct SyntheticWorker {
    gateway: WorkerGateway,
    lease: ConnectionLease,
    journal: JournalId,
    session: ComputerSession,
    outbound: mpsc::Receiver<Envelope<ManagerMessage>>,
}
impl SyntheticWorker {
    async fn start(app: &App) -> Self {
        let peer = || VerifiedIdentity::for_test(b"M04 recorded worker identity");
        let invitation = app
            .call(
                "POST",
                "/v1/worker-invitations",
                json!({"fingerprint":peer().fingerprint(),"lifetime_seconds":600}),
                201,
            )
            .await;
        let gateway = app.manager.worker_gateway();
        let worker = gateway
            .enroll(
                peer(),
                SecretString::from(invitation["invitation"].as_str().unwrap()),
            )
            .await
            .unwrap();
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
        Self::reconnect(app, session).await
    }
    async fn reconnect(app: &App, session: ComputerSession) -> Self {
        let peer = || VerifiedIdentity::for_test(b"M04 recorded worker identity");
        let gateway = app.manager.worker_gateway();
        let (out, outbound) = mpsc::channel(32);
        let (stop, _) = watch::channel(false);
        let mut hello = Hello::current(false);
        hello
            .capabilities
            .extend([Capability::ComputerV1, Capability::ProcessSystemdV1]);
        let lease = gateway
            .connect(peer(), hello, out, stop, RequestId::new())
            .await
            .unwrap();
        let mut result = Self {
            gateway,
            lease,
            journal: JournalId::new(),
            session,
            outbound,
        };
        result
            .report(WorkerMessage::SnapshotApplied {
                generation: lease.generation,
            })
            .await;
        result
            .report(WorkerMessage::RecoveryHello {
                generation: lease.generation,
                incarnation: result.journal,
                high_water: 0,
            })
            .await;
        result
            .report(WorkerMessage::Inventory {
                generation: lease.generation,
                incarnation: result.journal,
                offset: 0,
                total: 0,
                entries: vec![],
            })
            .await;
        result.session = app
            .manager
            .computer_session(app.session(), result.session.id)
            .await
            .unwrap();
        result.drain();
        result
    }
    async fn report(&self, message: WorkerMessage) {
        self.gateway
            .receive(self.lease, Envelope::new(message))
            .await
            .unwrap();
    }
    fn drain(&mut self) -> Vec<ManagerMessage> {
        let mut result = Vec::new();
        while let Ok(message) = self.outbound.try_recv() {
            result.push(message.message);
        }
        result
    }
    async fn approved(&mut self, app: &App, task: TaskId) -> TaskView {
        let task: TaskView = serde_json::from_value(
            app.call(
                "PUT",
                &format!("/v1/tasks/{task}/approval"),
                json!({"expires_at":now()+60}),
                200,
            )
            .await,
        )
        .unwrap();
        self.report(WorkerMessage::Claim {
            incarnation: self.journal,
            fence: self.session.fence(self.lease.generation),
            binding: task.approval.binding.clone().unwrap(),
        })
        .await;
        let permit = self
            .drain()
            .into_iter()
            .find(|m| matches!(m, ManagerMessage::Permit { .. }))
            .expect("exact child approval produces a permit");
        self.gateway
            .admit_delivery(self.lease, permit)
            .await
            .unwrap();
        task
    }
    async fn finished(&self, task: &TaskView) {
        let scope = ProcessScope {
            journal: self.journal,
            execution: task.command.execution,
            host: CertificateFingerprint::from_der(b"recorded host"),
            boot: RequestId::new().to_string(),
            invocation: "a".repeat(32),
            unit: format!(
                "botstral-{}-{}.service",
                self.journal, task.command.execution
            ),
            cgroup: format!(
                "/recorded/botstral-{}-{}.service",
                self.journal, task.command.execution
            ),
        };
        self.report(WorkerMessage::Reports {
            generation: self.lease.generation,
            incarnation: self.journal,
            high_water: 2,
            reports: vec![
                DurableReport {
                    sequence: 1,
                    command: task.command.id,
                    execution: task.command.execution,
                    event: ExecutionEvent::ProcessResult {
                        evidence: ProcessEvidence {
                            effect: ProcessEffect::Observed,
                            result: Some(ProcessResult {
                                exit: ProcessExit::Exited { code: 0 },
                                stdout: CapturedOutput {
                                    hex: "646f6e65".into(),
                                    truncated: false,
                                    complete: true,
                                },
                                stderr: CapturedOutput {
                                    hex: String::new(),
                                    truncated: false,
                                    complete: true,
                                },
                            }),
                        },
                    },
                },
                DurableReport {
                    sequence: 2,
                    command: task.command.id,
                    execution: task.command.execution,
                    event: ExecutionEvent::ProcessFinished {
                        scope,
                        reason: ProcessCompletionReason::Result,
                        success: true,
                    },
                },
            ],
        })
        .await;
    }
    async fn artifact(&mut self, task: &TaskView, slot: &str, bytes: &[u8]) -> ArtifactId {
        self.drain();
        self.report(WorkerMessage::ArtifactOffer {
            offer: ArtifactOffer {
                journal: self.journal,
                task: task.id,
                execution: task.command.execution,
                slot: slot.into(),
                output: SealedOutput::Content {
                    content: ArtifactContent {
                        sha256: CertificateFingerprint::from_der(bytes),
                        length: bytes.len() as u64,
                    },
                },
            },
        })
        .await;
        let id = self
            .drain()
            .into_iter()
            .find_map(|m| match m {
                ManagerMessage::ArtifactProgress { id, .. } => Some(id),
                _ => None,
            })
            .unwrap();
        self.report(WorkerMessage::ArtifactChunk {
            chunk: ArtifactChunk {
                journal: self.journal,
                execution: task.command.execution,
                slot: slot.into(),
                id,
                offset: 0,
                hex: bytes.iter().map(|b| format!("{b:02x}")).collect(),
            },
        })
        .await;
        id
    }
}
fn request(app: &App, session: ComputerSessionId) -> SubmitRun {
    SubmitRun { images: Vec::new(),id:RunId::new(),bot:app.bot,session: Some(session),configuration_revision:1,prompt:"Read the example page, write and read /srv/shared/m04-note.txt, and explain the result.".into(),limits:RunLimits {deadline:now()+90,max_turns:4,max_total_tokens:200_000,max_cost_microusd:200_000,max_output_tokens:64,max_operations:12,max_concurrent_operations:1},execution_policy:RunExecutionPolicy { connectors: None, vision: None, vibe: None,
        computer:Some(ComputerToolPolicy {profile:ComputerProfile::LinuxPlaywrightV1,account:ProcessAccount {uid:1000,gid:1000},runtime:ComputerRuntime {sha256:CertificateFingerprint::from_der(b"fixture runtime")},limits:ProcessLimits {runtime_seconds:30,output_bytes:1024,processes:64,memory_bytes:512*1024*1024},viewport:Viewport {width:800,height:600},outputs:[("page","page.txt"),("file","shared.txt")].into_iter().map(|(slot,name)|OutputDeclaration {slot:slot.into(),name:name.into(),media_type:MediaType::TextPlain,max_bytes:8192}).collect()}),
        process:Some(ProcessToolPolicy {profile:ProcessProfile::LinuxSystemdV1,account:ProcessAccount {uid:1000,gid:1000},environment:BTreeMap::new(),limits:ProcessLimits {runtime_seconds:30,output_bytes:1024,processes:8,memory_bytes:64*1024*1024}}),
    }}
}
async fn unlock_run_history(app: &App, id: RunId) {
    let refused = app
        .call("GET", &format!("/v1/runs/{id}"), Value::Null, 409)
        .await;
    assert_eq!(refused["error"], "vault_locked");
    app.call(
        "POST",
        "/v1/vault/unlock",
        json!({"passphrase": PASSPHRASE}),
        204,
    )
    .await;
}
async fn view(app: &App, id: RunId) -> RunView {
    serde_json::from_value(
        app.call("GET", &format!("/v1/runs/{id}"), Value::Null, 200)
            .await,
    )
    .unwrap()
}
async fn phase(app: &App, id: RunId, predicate: impl Fn(&RunPhase) -> bool) -> RunView {
    let mut last_phase = None;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let view = view(app, id).await;
            last_phase = Some(view.summary.phase.clone());
            if predicate(&view.summary.phase) {
                return view;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!("run did not reach expected durable phase; last phase {last_phase:?}")
    })
}

#[tokio::test]
async fn m04_run_recorded_browser_file_approval_result_artifact_and_followup() {
    let root = tempfile::tempdir().unwrap();
    let local = private_local(&root);
    let fixture = Fixture::start().await;
    let app = App::start(local.directory().into(), &fixture).await;
    app.initialize().await;
    let mut worker = SyntheticWorker::start(&app).await;
    let input = request(&app, worker.session.id);
    fixture.reply(Reply {
        body: TOOL.into(),
        ..Reply::default()
    });
    app.call("POST", "/v1/runs", json!(input), 200).await;
    let waiting = phase(&app, input.id, |p| {
        matches!(p, RunPhase::WaitingForTool { .. })
    })
    .await;
    assert_eq!(waiting.summary.accounting.settled_cost_microusd, 28);
    let intent = waiting.turns[0].intent.as_ref().unwrap();
    let deliveries = worker.drain();
    assert!(
        deliveries
            .iter()
            .any(|m| matches!(m,ManagerMessage::Delivery(d) if d.authority.is_none()))
    );
    assert!(
        !deliveries
            .iter()
            .any(|m| matches!(m, ManagerMessage::Permit { .. }))
    );
    let task = app
        .call(
            "GET",
            &format!("/v1/tasks/{}", intent.task),
            Value::Null,
            200,
        )
        .await;
    assert_eq!(task["approval"]["consumed"], false);
    assert!(task["approval"]["binding"].is_null());
    assert_eq!(
        task["command"]["work"]["spec"]["operations"][2]["path"],
        "/srv/shared/m04-note.txt"
    );
    let approved = worker.approved(&app, intent.task).await;
    fixture.reply(Reply::default());
    worker.finished(&approved).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        fixture.chats(),
        1,
        "terminal task still waits for declared outputs"
    );
    worker
        .artifact(&approved, "page", b"Recorded browser page")
        .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(fixture.chats(), 1);
    let artifact = worker
        .artifact(&approved, "file", b"durable shared guest file")
        .await;
    let completed = phase(&app, input.id, |p| matches!(p, RunPhase::Completed)).await;
    assert_eq!(completed.summary.accounting.admitted_turns, 2);
    assert_eq!(completed.summary.accounting.settled_tokens, 40);
    assert_eq!(completed.summary.accounting.settled_cost_microusd, 56);
    let db = rusqlite::Connection::open(local.directory().join("manager.sqlite3")).unwrap();
    let prepared: String = db
        .query_row(
            "SELECT provider_request FROM run_turns WHERE id=?1",
            [waiting.turns[0].id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&prepared).unwrap(),
        fixture.state.requests.lock().unwrap()[0]
    );
    assert!(
        db.execute("UPDATE run_turns SET provider_request='{}'", [])
            .is_err()
    );
    drop(db);
    let requests = fixture.state.requests.lock().unwrap().clone();
    assert_eq!(
        requests[1]["messages"][2]["tool_calls"][0]["id"],
        "tool00001"
    );
    assert_eq!(requests[1]["messages"][3]["role"], "tool");
    assert_eq!(requests[1]["messages"][3]["tool_call_id"], "tool00001");
    assert!(
        requests[1]["messages"][3]["content"]
            .as_str()
            .unwrap()
            .contains("durable shared guest file")
    );
    assert!(!requests[1].to_string().contains(KEY));
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!(
            "{}/v1/tasks/{}/artifacts/{artifact}",
            app.address, intent.task
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
    assert_eq!(
        app.call("POST", "/v1/runs", json!(input), 200).await,
        json!(completed)
    );
    let snapshot = app.call("GET", "/v1/events/0/64", Value::Null, 200).await;
    assert_eq!(snapshot["state"]["runs"], json!([completed.summary]));
    assert!(snapshot["state"]["runs"][0].get("turns").is_none());
    drop(worker);
    app.shutdown().await;
    let app = App::start(local.directory().into(), &fixture).await;
    unlock_run_history(&app, input.id).await;
    assert_eq!(view(&app, input.id).await, completed);
    assert_eq!(fixture.chats(), 2);
    app.shutdown().await;
    no_plaintext(local.directory());
}

#[tokio::test]
async fn m04_run_truncated_final_is_stopped_and_persisted() {
    let root = tempfile::tempdir().unwrap();
    let local = private_local(&root);
    let fixture = Fixture::start().await;
    let app = App::start(local.directory().into(), &fixture).await;
    app.initialize().await;
    let worker = SyntheticWorker::start(&app).await;
    let input = request(&app, worker.session.id);
    let mut response: Value = serde_json::from_str(COMPLETION).unwrap();
    response["choices"][0]["finish_reason"] = json!("length");
    fixture.reply(Reply {
        body: response.to_string(),
        ..Reply::default()
    });
    app.call("POST", "/v1/runs", json!(input), 200).await;
    let stopped = phase(&app, input.id, RunPhase::terminal).await;
    let saved = serde_json::to_value(&stopped).unwrap();
    assert_eq!(saved["summary"]["phase"]["type"], "stopped");
    assert_eq!(saved["summary"]["phase"]["reason"], "output_limit");
    assert_eq!(saved["turns"][0]["finish_reason"], "length");
    assert_eq!(
        saved["turns"][0]["response"]["content"],
        response["choices"][0]["message"]["content"]
    );
    assert_eq!(saved["turns"][0]["accounting"]["type"], "settled");
    assert_eq!(stopped.summary.accounting.settled_tokens, 20);
    assert_eq!(stopped.summary.accounting.settled_cost_microusd, 28);
    assert_eq!(stopped.summary.accounting.held_tokens, 0);
    assert!(stopped.turns[0].intent.is_none());
    assert_eq!(fixture.chats(), 1);
    drop(worker);
    app.shutdown().await;
    let app = App::start(local.directory().into(), &fixture).await;
    unlock_run_history(&app, input.id).await;
    assert_eq!(view(&app, input.id).await, stopped);
    assert_eq!(fixture.chats(), 1);
    app.shutdown().await;
}

#[tokio::test]
async fn m04_run_unsupported_proposals_settle_valid_usage_without_children() {
    let root = tempfile::tempdir().unwrap();
    let local = private_local(&root);
    let fixture = Fixture::start().await;
    let app = App::start(local.directory().into(), &fixture).await;
    app.initialize().await;
    let worker = SyntheticWorker::start(&app).await;
    let base: Value = serde_json::from_str(TOOL).unwrap();
    for kind in [
        "multiple",
        "unknown",
        "malformed",
        "authority",
        "oversized",
        "wrong_finish",
        "missing_usage",
        "invalid_usage",
        "reflected",
        "encoded_secret",
    ] {
        let mut reply = base.clone();
        let function = &mut reply["choices"][0]["message"]["tool_calls"][0]["function"];
        match kind {
            "unknown" => function["name"] = "not_a_tool".into(),
            "malformed" => function["arguments"] = "{".into(),
            "authority" => {
                function["arguments"] =
                    r#"{"operations":[],"outputs":[],"account":{"uid":0,"gid":0}}"#.into()
            }
            "oversized" => function["arguments"] = "x".repeat(2049).into(),
            "multiple" => {
                let call = reply["choices"][0]["message"]["tool_calls"][0].clone();
                reply["choices"][0]["message"]["tool_calls"]
                    .as_array_mut()
                    .unwrap()
                    .push(call);
            }
            "wrong_finish" => reply["choices"][0]["finish_reason"] = "stop".into(),
            "missing_usage" => {
                reply.as_object_mut().unwrap().remove("usage");
            }
            "invalid_usage" => reply["usage"]["total_tokens"] = 999.into(),
            "reflected" | "encoded_secret" => {
                reply["choices"][0]["message"]["content"] = KEY.into()
            }
            _ => unreachable!(),
        }
        let body = if kind == "encoded_secret" {
            reply
                .to_string()
                .replace(KEY, &KEY.replace("secret", "\\u0073ecret"))
        } else {
            reply.to_string()
        };
        fixture.reply(Reply {
            body,
            ..Reply::default()
        });
        let input = request(&app, worker.session.id);
        app.call("POST", "/v1/runs", json!(input), 200).await;
        let settled = phase(&app, input.id, RunPhase::terminal).await;
        assert!(settled.turns[0].intent.is_none(), "{kind}");
        if matches!(
            kind,
            "missing_usage" | "invalid_usage" | "reflected" | "encoded_secret"
        ) {
            assert_eq!(settled.summary.accounting.held_tokens, 32832, "{kind}");
            assert_eq!(settled.summary.accounting.settled_tokens, 0);
        } else {
            assert_eq!(
                settled.summary.phase,
                RunPhase::Stopped {
                    reason: RunStopReason::UnsupportedProposal
                },
                "{kind}"
            );
            assert_eq!(
                settled.summary.accounting.settled_cost_microusd, 28,
                "{kind}"
            );
            assert_eq!(settled.summary.accounting.held_tokens, 0);
        }
    }
    assert!(
        app.call("GET", "/v1/events/0/64", Value::Null, 200).await["state"]["tasks"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    drop(worker);
    app.shutdown().await;
    no_plaintext(local.directory());
}

#[tokio::test]
async fn m04_run_late_usage_cancellation_and_crash() {
    let root = tempfile::tempdir().unwrap();
    let local = private_local(&root);
    let fixture = Fixture::start().await;
    let app = App::start(local.directory().into(), &fixture).await;
    app.initialize().await;
    let worker = SyntheticWorker::start(&app).await;
    let gate = Arc::new(Semaphore::new(0));
    fixture.reply(Reply {
        body: TOOL.into(),
        gate: Some(gate.clone()),
        ..Reply::default()
    });
    let input = request(&app, worker.session.id);
    app.call("POST", "/v1/runs", json!(input), 200).await;
    fixture.reached(1).await;
    tokio::time::timeout(
        Duration::from_secs(1),
        app.call("GET", "/v1/owner", Value::Null, 200),
    )
    .await
    .unwrap();
    app.call(
        "POST",
        &format!("/v1/runs/{}/cancel", input.id),
        json!({}),
        200,
    )
    .await;
    gate.add_permits(1);
    let settled = phase(&app, input.id, RunPhase::terminal).await;
    assert_eq!(settled.summary.phase, RunPhase::Cancelled);
    assert_eq!(settled.summary.accounting.settled_cost_microusd, 28);
    assert!(settled.turns[0].intent.is_none());
    fixture.reply(Reply {
        body: TOOL.into(),
        gate: Some(Arc::new(Semaphore::new(0))),
        ..Reply::default()
    });
    let input = request(&app, worker.session.id);
    app.call("POST", "/v1/runs", json!(input), 200).await;
    fixture.reached(2).await;
    let sending = view(&app, input.id).await;
    drop(worker);
    app.crash().await;
    let app = App::start(local.directory().into(), &fixture).await;
    unlock_run_history(&app, input.id).await;
    let recovered = view(&app, input.id).await;
    assert_eq!(
        recovered.summary.phase,
        RunPhase::AwaitingAttention {
            reason: RunAttentionReason::Restart
        }
    );
    assert_eq!(recovered.turns[0].attempt, sending.turns[0].attempt);
    assert_eq!(recovered.summary.accounting.held_tokens, 32832);
    app.call("POST", "/v1/runs", json!(input), 200).await;
    assert_eq!(fixture.chats(), 2);
    app.shutdown().await;
    no_plaintext(local.directory());
}

#[tokio::test]
async fn m04_run_function_capability_and_owner_authentication() {
    let root = tempfile::tempdir().unwrap();
    let local = private_local(&root);
    let fixture = Fixture::start().await;
    let mut app = App::start(local.directory().into(), &fixture).await;
    app.initialize().await;
    let worker = SyntheticWorker::start(&app).await;
    let input = request(&app, worker.session.id);
    let token = std::mem::take(&mut app.token);
    for (method, path, body) in [
        ("POST", "/v1/runs".into(), json!(input)),
        ("GET", format!("/v1/runs/{}", input.id), Value::Null),
        ("POST", format!("/v1/runs/{}/cancel", input.id), json!({})),
    ] {
        app.call(method, &path, body, 401).await;
    }
    app.token = token;
    let mut model: Value = serde_json::from_str(MODEL).unwrap();
    model["capabilities"]["function_calling"] = false.into();
    *fixture.state.model.lock().unwrap() = model.to_string();
    app.verify(1, 200).await;
    app.call("POST", "/v1/runs", json!(input), 409).await;
    let single = app.request(1);
    app.submit(&single, 200).await;
    assert_eq!(app.settled(single.id).await["phase"]["type"], "completed");
    drop(worker);
    app.shutdown().await;
}

#[path = "run_production.rs"]
mod production;

#[tokio::test]
async fn m04_run_capacity_backpressure_and_bounded_shutdown() {
    let root = tempfile::tempdir().unwrap();
    let local = private_local(&root);
    let fixture = Fixture::start().await;
    let app = App::start(local.directory().into(), &fixture).await;
    app.initialize().await;
    let worker = SyntheticWorker::start(&app).await;
    let capacity = app
        .manager
        .provider
        .calls
        .clone()
        .acquire_many_owned(MAX_INFERENCE_CALLS as u32)
        .await
        .unwrap();
    let clock = now();
    app.manager
        .queue
        .call_wait(move |state| {
            state.test_time = Some(clock);
            Ok(())
        })
        .await
        .unwrap();
    let mut expired = request(&app, worker.session.id);
    expired.limits.deadline = clock + 1;
    app.call("POST", "/v1/runs", json!(expired), 200).await;
    app.manager
        .queue
        .call_wait(move |state| {
            state.test_time = Some(clock + 1);
            Ok(())
        })
        .await
        .unwrap();
    let expired = phase(&app, expired.id, RunPhase::terminal).await;
    assert_eq!(
        expired.summary.phase,
        RunPhase::Stopped {
            reason: RunStopReason::Deadline
        }
    );
    assert!(expired.turns.is_empty());
    assert_eq!(fixture.chats(), 0);
    app.manager
        .queue
        .call_wait(|state| {
            state.test_time = None;
            Ok(())
        })
        .await
        .unwrap();
    drop(capacity);
    let gate = Arc::new(Semaphore::new(0));
    fixture.reply(Reply {
        gate: Some(gate.clone()),
        ..Reply::default()
    });
    let input = request(&app, worker.session.id);
    app.call("POST", "/v1/runs", json!(input), 200).await;
    fixture.reached(1).await;
    let (entered_tx, entered_rx) = oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let queue = app.manager.queue.clone();
    let blocker = tokio::spawn(async move {
        queue
            .call(false, move |_| {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok(())
            })
            .await
    });
    entered_rx.await.unwrap();
    while app.manager.queue.sender.try_send(Box::new(|_| {})).is_ok() {}
    assert_eq!(app.manager.queue.sender.capacity(), 0);
    gate.add_permits(1);
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(app.manager.queue.sender.capacity(), 0);
    assert_eq!(
        app.manager.owner(app.session()).await,
        Err(OperationError::Busy)
    );
    release_tx.send(()).unwrap();
    blocker.await.unwrap().unwrap();
    app.manager.queue.call_wait(|_| Ok(())).await.unwrap();
    let completed = phase(&app, input.id, RunPhase::terminal).await;
    assert_eq!(completed.summary.phase, RunPhase::Completed);
    assert_eq!(completed.summary.accounting.settled_cost_microusd, 28);
    fixture.reply(Reply {
        gate: Some(Arc::new(Semaphore::new(0))),
        ..Reply::default()
    });
    let input = request(&app, worker.session.id);
    app.call("POST", "/v1/runs", json!(input), 200).await;
    fixture.reached(2).await;
    drop(worker);
    app.shutdown().await;
    let app = App::start(local.directory().into(), &fixture).await;
    unlock_run_history(&app, input.id).await;
    let stopped = view(&app, input.id).await;
    assert_eq!(
        stopped.summary.phase,
        RunPhase::AwaitingAttention {
            reason: RunAttentionReason::Provider
        }
    );
    assert_eq!(
        stopped.turns[0].phase,
        RunTurnPhase::AwaitingAttention {
            reason: InferenceAttentionReason::Shutdown
        }
    );
    assert_eq!(stopped.summary.accounting.held_tokens, 32832);
    assert_eq!(fixture.chats(), 2);
    app.shutdown().await;
    no_plaintext(local.directory());
}

#[tokio::test]
async fn m04_run_deadlines_reconcile_while_provider_slots_are_full() {
    let root = tempfile::tempdir().unwrap();
    let local = private_local(&root);
    let fixture = Fixture::start().await;
    let app = App::start(local.directory().into(), &fixture).await;
    app.initialize().await;
    let worker = SyntheticWorker::start(&app).await;
    fixture.reply(Reply {
        body: TOOL.into(),
        ..Reply::default()
    });
    let mut expiring = request(&app, worker.session.id);
    expiring.limits.deadline = now() + 3;
    app.call("POST", "/v1/runs", json!(expiring), 200).await;
    let waiting = phase(&app, expiring.id, |p| {
        matches!(p, RunPhase::WaitingForTool { .. })
    })
    .await;
    let intent = waiting.turns[0].intent.as_ref().unwrap();
    let gate = Arc::new(Semaphore::new(0));
    fixture.reply(Reply {
        gate: Some(gate.clone()),
        ..Reply::default()
    });
    let first = request(&app, worker.session.id);
    let second = request(&app, worker.session.id);
    for input in [&first, &second] {
        app.call("POST", "/v1/runs", json!(input), 200).await;
    }
    fixture.reached(3).await;
    phase(&app, expiring.id, |p| *p == RunPhase::Cancelling).await;
    assert_eq!(
        app.call(
            "GET",
            &format!("/v1/tasks/{}", intent.task),
            Value::Null,
            200
        )
        .await["state"],
        "cancelling"
    );
    assert!(matches!(
        view(&app, first.id).await.summary.phase,
        RunPhase::Calling { .. }
    ));
    assert!(matches!(
        view(&app, second.id).await.summary.phase,
        RunPhase::Calling { .. }
    ));
    worker
        .report(WorkerMessage::Reports {
            generation: worker.lease.generation,
            incarnation: worker.journal,
            high_water: 1,
            reports: vec![DurableReport {
                sequence: 1,
                command: intent.command,
                execution: intent.execution,
                event: ExecutionEvent::Stopped {
                    applied: 0,
                    evidence: StopEvidence::NotLaunched,
                },
            }],
        })
        .await;
    let stopped = phase(&app, expiring.id, RunPhase::terminal).await;
    assert_eq!(
        stopped.summary.phase,
        RunPhase::Stopped {
            reason: RunStopReason::Deadline
        }
    );
    for input in [&first, &second] {
        app.call(
            "POST",
            &format!("/v1/runs/{}/cancel", input.id),
            json!({}),
            200,
        )
        .await;
    }
    gate.add_permits(2);
    for input in [&first, &second] {
        assert_eq!(
            phase(&app, input.id, RunPhase::terminal)
                .await
                .summary
                .phase,
            RunPhase::Cancelled
        );
    }
    drop(worker);
    app.shutdown().await;
}

mod desktop_fixture;
pub(super) mod production_fixture;

#[tokio::test]
async fn p03_manager_loop_executes_without_owner_clients_and_recovers_inbox() {
    let root = tempfile::tempdir().unwrap();
    let local = private_local(&root);
    let fixture = Fixture::start().await;
    let app = App::start(local.directory().into(), &fixture).await;
    app.initialize().await;
    let worker = SyntheticWorker::start(&app).await;
    let request = request(&app, worker.session.id);
    let routine = RoutineId::new();
    let bot = app.bot;
    let session = app.session();
    let session_revision = worker.session.revision;
    app.manager
        .queue
        .call(false, move |s| {
            s.routine_command(
                session,
                RoutineCommand {
                    id: RequestId::new(),
                    action: RoutineAction::SetBudget {
                        budget: RoutineBudget {
                            max_runs: 4,
                            max_tokens: 2_000_000,
                            max_cost_microusd: 2_000_000,
                        },
                    },
                },
            )?;
            Ok(())
        })
        .await
        .unwrap();
    let session = app.session();
    app.manager
        .queue
        .call(false, move |s| {
            s.routine_command(
                session,
                RoutineCommand {
                    id: RequestId::new(),
                    action: RoutineAction::Create {
                        id: routine,
                        definition: Box::new(RoutineDefinition {
                            bot,
                            name: "Clients closed".into(),
                            trigger: RoutineTrigger::Once { at: now() + 1 },
                            catchup_seconds: 60,
                            wakeups: None,
                            template: RunTemplate {
                                session: request.session.unwrap(),
                                session_revision,
                                configuration_revision: 1,
                                permission_revision: 1,
                                prompt: "Complete one scheduled turn.".into(),
                                execution_policy: request.execution_policy,
                                duration_seconds: 60,
                                max_turns: 4,
                                max_total_tokens: 200_000,
                                max_cost_microusd: 200_000,
                                max_output_tokens: 64,
                                max_operations: 4,
                            },
                        }),
                    },
                },
            )?;
            Ok(())
        })
        .await
        .unwrap();
    app.manager.sign_out(app.session()).await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let inbox = app.manager.queue.call(false, |s| s.inbox()).await.unwrap();
            if let Some(entry) = inbox.into_iter().find(|e| e.routine == routine) {
                break entry;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("manager-only schedule completed");
    assert!(matches!(
        result.status,
        OccurrenceStatus::Finished {
            phase: RunPhase::Completed
        }
    ));
    assert_eq!(fixture.chats(), 1);
    drop(worker);
    app.shutdown().await;
    let app = App::start(local.directory().into(), &fixture).await;
    let inbox = app.manager.queue.call(false, |s| s.inbox()).await.unwrap();
    assert_eq!(inbox[0].id, result.id);
    assert_eq!(inbox[0].run, result.run);
    app.shutdown().await;
}

#[path = "memory_tests.rs"]
mod memory_tests;
