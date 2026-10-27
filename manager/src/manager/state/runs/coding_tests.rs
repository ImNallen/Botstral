use super::*;
use crate::manager::{
    coding::{CodingAdmission, CodingRequest},
    provider::{EnvelopeOutcome, ModelRequest, Provider},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

fn coding(f: &mut Fixture, deadline: Option<i64>) -> TaskView {
    f.state
        .worker_connections
        .get_mut(&f.lease.worker)
        .unwrap()
        .capabilities
        .push(Capability::VibeV1);
    let config = f
        .state
        .load_inference_configuration(f.input.bot, 1)
        .unwrap();
    f.input.execution_policy.vibe = Some(VibeToolPolicy {
        policy: VibePolicy {
            workspace: CodingWorkspace {
                id: CodingWorkspaceId::new(),
                path: "/srv/coding/fixture".into(),
            },
            runtime: CertificateFingerprint::from_der(b"pinned Vibe"),
            account: ProcessAccount {
                uid: 1000,
                gid: 1000,
            },
            resources: ProcessLimits {
                runtime_seconds: 60,
                output_bytes: 1024,
                processes: 32,
                memory_bytes: 268435456,
            },
            configuration_revision: 1,
            model_id: config.policy.model_id,
            access: VibeAccess::EditFilesAsGuestAccount,
            outputs: vec![],
        },
        limits: RunLimits {
            deadline: deadline.unwrap_or(f.input.limits.deadline),
            max_operations: 1,
            max_turns: 3,
            ..f.input.limits.clone()
        },
    });
    f.state.submit_run(token(), f.input.clone()).unwrap();
    let permit = claim(&mut f.state);
    f.state
        .finish_run_call(
            f.input.id,
            permit.turn.id,
            permit.turn.attempt,
            response(Some(RunToolCall {
                id: "coding-call".into(),
                proposal: ToolProposal::Vibe(VibeProposal {
                    prompt: "Edit answer.py.".into(),
                    session: VibeSession::New,
                }),
            })),
        )
        .unwrap();
    let run = f.state.load_run(f.input.id).unwrap();
    let task = f
        .state
        .load_task(run.turns[0].intent.as_ref().unwrap().task)
        .unwrap();
    f.approve_and_claim(task.id);
    task
}
#[test]
fn coding_child_uses_the_earlier_policy_or_parent_deadline() {
    for (policy_deadline, expected_deadline) in [(1450, 1450), (1500, 1500), (1600, 1500)] {
        let mut f = fixture();
        let task = coding(&mut f, Some(policy_deadline));
        assert_eq!(task.state, TaskState::Queued);
        assert_eq!(task.request.settings.deadline, expected_deadline);
        let WorkSpec::Vibe { spec } = &task.command.work else {
            panic!("expected coding child");
        };
        assert_eq!(spec.limits.deadline, expected_deadline);
        let run = f.state.load_run(f.input.id).unwrap();
        assert!(
            matches!(run.summary.phase, RunPhase::WaitingForTool { task: id, .. } if id == task.id)
        );
        assert_eq!(
            f.state.load_task(task.id).unwrap().state,
            TaskState::Running
        );
    }
}

fn request(model: &str, tools: bool) -> CodingRequest {
    serde_json::from_value(json!({"model":model,"messages":[{"role":"user","content":"Recorded coding request"}],"stream":true,
        "stream_options":{"include_usage":true},"tools":if tools { vec![json!({"type":"function","function":{"name":"edit","parameters":{"type":"object"}}})] } else { vec![] }})).unwrap()
}
fn admit(
    f: &mut Fixture,
    task: &TaskView,
    id: InferenceAttemptId,
    helper: bool,
) -> Result<CodingAdmission, OperationError> {
    f.state.admit_coding(
        VerifiedIdentity::for_test(b"M04 synthetic worker identity"),
        task.id,
        task.command.execution,
        id,
        request(
            &f.input
                .execution_policy
                .vibe
                .as_ref()
                .unwrap()
                .policy
                .model_id,
            !helper,
        ),
    )
}
fn reply(model: &str) -> Value {
    json!({"id":"recorded-coding-response","model":model,"created":1,"object":"chat.completion",
        "choices":[{"index":0,"message":{"role":"assistant","content":"Edited."},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":12,"completion_tokens":8,"total_tokens":20}})
}
#[tokio::test]
async fn coding_helpers_retry_and_parent_limits_use_recorded_http_and_prior_durable_holds() {
    let mut f = fixture();
    let task = coding(&mut f, None);
    let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
    let captured = requests.clone();
    let body = reply(
        &f.input
            .execution_policy
            .vibe
            .as_ref()
            .unwrap()
            .policy
            .model_id,
    );
    let router = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(
            move |headers: axum::http::HeaderMap, axum::Json(value): axum::Json<Value>| {
                let captured = captured.clone();
                let body = body.clone();
                async move {
                    assert_eq!(
                        headers["authorization"],
                        "Bearer synthetic-state-inference-secret"
                    );
                    assert_eq!(value["stream"], false);
                    assert_eq!(value["service_tier"], "standard_only");
                    assert_eq!(value["max_tokens"], 64);
                    assert_eq!(value["n"], 1);
                    captured.lock().unwrap().push(value);
                    axum::Json(body)
                }
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let provider = Provider::fixture(format!("http://{}", listener.local_addr().unwrap()));
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    for index in 0..3 {
        let id = InferenceAttemptId::new();
        let CodingAdmission::Call(permit) = admit(&mut f, &task, id, index == 1).unwrap() else {
            panic!()
        };
        let durable = f.state.coding_calls(token(), task.id).unwrap();
        assert_eq!(durable.last().unwrap().phase, CodingCallPhase::Reserved);
        assert!(durable.last().unwrap().reservation.total_tokens >= 32768 + 64);
        assert!(admit(&mut f, &task, InferenceAttemptId::new(), true).is_err());
        assert_eq!(requests.lock().unwrap().len(), index);
        assert!(f.state.send_coding(id).unwrap());
        let outcome = provider
            .envelope(
                ModelRequest {
                    policy: &permit.configuration.policy,
                    deadline: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs() as i64
                        + 10,
                    max_output_tokens: permit.spec.limits.max_output_tokens,
                    max_total_tokens: permit.view.reservation.total_tokens,
                    prepared: &permit.prepared,
                },
                &permit.secret,
                None,
            )
            .await;
        let response = f.state.finish_coding(id, outcome).unwrap();
        assert!(matches!(
            admit(&mut f, &task, id, index == 1).unwrap(),
            CodingAdmission::Cached(_)
        ));
        assert!(
            !response
                .to_string()
                .contains("synthetic-state-inference-secret")
        );
        assert!(crate::manager::coding::sse(response).ends_with("data: [DONE]\n\n"));
    }
    assert!(matches!(
        admit(&mut f, &task, InferenceAttemptId::new(), false),
        Err(OperationError::InferenceBudget)
    ));
    assert_eq!(requests.lock().unwrap().len(), 3);
    let parent = f.state.load_run(f.input.id).unwrap();
    assert_eq!(parent.summary.accounting.admitted_coding_calls, 3);
    assert_eq!(parent.summary.accounting.settled_tokens, 80);
    assert_eq!(parent.summary.accounting.held_tokens, 0);
    server.abort();
}
#[test]
fn coding_uncertainty_recovery_cancellation_and_identity_never_restore_authority() {
    for cause in [
        "unknown",
        "restart",
        "cancel",
        "permission",
        "worker",
        "changed_request",
    ] {
        let mut f = fixture();
        let task = coding(&mut f, None);
        let id = InferenceAttemptId::new();
        assert!(matches!(
            f.state.admit_coding(
                VerifiedIdentity::for_test(b"different worker"),
                task.id,
                task.command.execution,
                id,
                request(
                    &f.input
                        .execution_policy
                        .vibe
                        .as_ref()
                        .unwrap()
                        .policy
                        .model_id,
                    false
                )
            ),
            Err(OperationError::Unauthorized)
        ));
        assert!(admit(&mut f, &task, id, false).is_ok());
        assert!(f.state.send_coding(id).unwrap());
        match cause {
            "unknown" => {
                assert!(
                    f.state
                        .finish_coding(
                            id,
                            EnvelopeOutcome::Unknown {
                                reason: InferenceAttentionReason::InvalidUsage,
                                request_id: None
                            }
                        )
                        .is_err()
                );
            }
            "restart" => f.state.recover_coding().unwrap(),
            "cancel" => {
                f.state.cancel_task(token(), task.id).unwrap();
            }
            "permission" => {
                let revision = f.state.permission(f.input.bot).unwrap().revision;
                f.state
                    .set_permission(token(), f.input.bot, revision, false)
                    .unwrap();
            }
            "worker" => {
                f.state.worker_connections.remove(&f.lease.worker);
            }
            "changed_request" => {
                assert!(admit(&mut f, &task, id, true).is_err());
            }
            _ => unreachable!(),
        }
        assert!(
            admit(&mut f, &task, InferenceAttemptId::new(), true).is_err(),
            "{cause}"
        );
        assert_eq!(
            f.state.coding_calls(token(), task.id).unwrap()[0].accounting,
            InferenceAccounting::Held
        );
        assert!(
            f.state
                .load_run(f.input.id)
                .unwrap()
                .summary
                .accounting
                .held_tokens
                > 0
        );
    }
}
#[test]
fn coding_validation_rejects_model_switches_tools_urls_and_unknown_options() {
    let mut f = fixture();
    let task = coding(&mut f, None);
    let WorkSpec::Vibe { spec } = task.command.work else {
        panic!()
    };
    let mut value = serde_json::to_value(request(&spec.policy.model_id, true)).unwrap();
    value["model"] = json!("other-model");
    assert!(
        serde_json::from_value::<CodingRequest>(value.clone())
            .unwrap()
            .prepare(&spec)
            .is_err()
    );
    value["model"] = json!(spec.policy.model_id);
    value["tools"][0]["function"]["name"] = json!("bash");
    assert!(
        serde_json::from_value::<CodingRequest>(value.clone())
            .unwrap()
            .prepare(&spec)
            .is_err()
    );
    value["tools"] = json!([]);
    value["messages"][0]["content"] =
        json!([{"type":"image_url","image_url":{"url":"https://unapproved.example/image"}}]);
    assert!(
        serde_json::from_value::<CodingRequest>(value.clone())
            .unwrap()
            .prepare(&spec)
            .is_err()
    );
    value["response_format"] = json!({"type":"json_object"});
    assert!(serde_json::from_value::<CodingRequest>(value).is_err());
}

#[test]
fn pinned_vibe_wire_requests_pass_the_production_boundary_except_forbidden_tool_history() {
    let mut f = fixture();
    let task = coding(&mut f, None);
    let WorkSpec::Vibe { spec } = task.command.work else {
        panic!()
    };
    let requests: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/m04/vibe-requests.json"
    ))
    .unwrap();
    for (index, mut value) in requests.into_iter().enumerate() {
        value["model"] = json!(spec.policy.model_id);
        let request: CodingRequest = serde_json::from_value(value).unwrap();
        assert_eq!(
            request.prepare(&spec).is_ok(),
            index < 4,
            "recorded request {index}"
        );
    }
}

#[test]
fn resume_requires_confirmed_exit_same_owner_workspace_policy_and_exact_session() {
    let mut f = fixture();
    let task = coding(&mut f, None);
    let WorkSpec::Vibe { mut spec } = task.command.work.clone() else {
        panic!()
    };
    let native = RequestId::new();
    spec.session = VibeSession::Resume {
        task: task.id,
        native_session: native,
    };
    assert!(
        f.state
            .validate_coding_task(task.command.bot, task.command.worker, TaskId::new(), &spec)
            .is_err()
    );
    f.finish(task.id);
    let mut completed = f.state.load_task(task.id).unwrap();
    let result = VibeResult {
        native_session: native,
        exit_code: 0,
        calls: 0,
        approval_denied: 0,
        tool_errors: 0,
    };
    completed
        .process
        .as_mut()
        .unwrap()
        .result
        .as_mut()
        .unwrap()
        .stdout
        .hex = serde_json::to_vec(&result)
        .unwrap()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    tasks::save(&f.state.connection, &completed).unwrap();
    f.state
        .validate_coding_task(task.command.bot, task.command.worker, TaskId::new(), &spec)
        .unwrap();
    for change in ["owner", "workspace", "runtime", "session", "new"] {
        let mut other = spec.clone();
        let mut bot = task.command.bot;
        match change {
            "owner" => bot = BotId::new(),
            "workspace" => other.policy.workspace.path = "/srv/other".into(),
            "runtime" => other.policy.runtime = CertificateFingerprint::from_der(b"other"),
            "session" => {
                other.session = VibeSession::Resume {
                    task: task.id,
                    native_session: RequestId::new(),
                }
            }
            "new" => other.session = VibeSession::New,
            _ => unreachable!(),
        }
        assert!(
            f.state
                .validate_coding_task(bot, task.command.worker, TaskId::new(), &other)
                .is_err(),
            "{change}"
        );
    }
}

#[tokio::test]
async fn abandoned_coding_caller_during_state_admission_releases_known_unsent_hold() {
    use crate::manager::{Manager, Queue, inference::InferenceControl};
    use std::time::Duration;
    let mut f = fixture();
    let task = coding(&mut f, None);
    let WorkSpec::Vibe { spec } = &task.command.work else {
        panic!()
    };
    let bytes = serde_json::to_vec(&request(&spec.policy.model_id, false)).unwrap();
    let control = Arc::new(InferenceControl::default());
    f.state.set_inference_control(control.clone());
    let (sender, mut receiver) = mpsc::channel::<crate::manager::Job>(32);
    let queue = Queue {
        sender,
        credential: Arc::new(tokio::sync::Semaphore::new(1)),
    };
    let writer = std::thread::spawn(move || {
        while let Some(job) = receiver.blocking_recv() {
            job(&mut f.state);
        }
    });
    let manager = Manager {
        queue: queue.clone(),
        inference: control.clone(),
        provider: Provider::fixture("http://127.0.0.1:1".into()),
    };
    let (entered, ready) = tokio::sync::oneshot::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    let blocking_queue = queue.clone();
    let blocker = tokio::spawn(async move {
        blocking_queue
            .call_wait(move |_| {
                entered.send(()).unwrap();
                blocked.recv().unwrap();
                Ok(())
            })
            .await
            .unwrap();
    });
    ready.await.unwrap();
    let caller = manager.clone();
    let call = InferenceAttemptId::new();
    let request = tokio::spawn(async move {
        caller
            .coding_chat(
                VerifiedIdentity::for_test(b"M04 synthetic worker identity"),
                task.id,
                task.command.execution,
                call,
                bytes,
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        while queue.sender.capacity() == 32 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    release.send(()).unwrap();
    blocker.await.unwrap();
    let calls = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let calls = manager.coding_calls(token(), task.id).await.unwrap();
            if calls
                .first()
                .is_some_and(|c| c.phase == CodingCallPhase::NotSent)
            {
                break calls;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("abandoned pending admission settles without upstream HTTP");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].id, call);
    assert_eq!(calls[0].accounting, InferenceAccounting::Released);
    let mut jobs = control.coding_calls.lock().await;
    while let Some(result) = jobs.join_next().await {
        result.unwrap();
    }
    drop(jobs);
    drop(manager);
    drop(queue);
    tokio::task::spawn_blocking(move || writer.join().unwrap())
        .await
        .unwrap();
}
