use super::*;
use crate::manager::{
    provider::{ModelOutcome, ModelRequest, Provider},
    runs::{RunCallPermit, RunJob},
};
use std::collections::HashSet;

fn input(f: &Fixture, binding: ConnectorToolBinding) -> SubmitRun {
    SubmitRun {
        id: RunId::new(),
        bot: f.bot,
        session: f.session,
        configuration_revision: 1,
        prompt: "Use the granted connector.".into(),
        images: vec![],
        limits: RunLimits {
            deadline: 1500,
            max_turns: 4,
            max_total_tokens: 200_000,
            max_cost_microusd: 200_000,
            max_output_tokens: 64,
            max_operations: 4,
            max_concurrent_operations: 1,
        },
        execution_policy: RunExecutionPolicy {
            process: None,
            computer: None,
            vibe: None,
            vision: None,
            connectors: Some(ConnectorToolPolicy {
                tools: vec![binding],
                max_calls: 1,
                max_result_bytes: 8192,
                runner: runner(),
            }),
        },
    }
}
fn claim(f: &mut Fixture) -> RunCallPermit {
    let Some(RunJob::Call(permit)) = f.state.claim_run_work(&HashSet::new(), true).unwrap() else {
        panic!("provider turn")
    };
    *permit
}
fn response(binding: ConnectorToolBinding, id: &str) -> ModelOutcome {
    ModelOutcome::Billed {
        usage: InferenceUsage {
            prompt_tokens: 12,
            completion_tokens: 8,
            total_tokens: 20,
        },
        request_id: "synthetic-connector-response".into(),
        finish_reason: None,
        response: Ok(RunMessage::Assistant {
            content: String::new(),
            tool_call: Some(RunToolCall {
                id: id.into(),
                proposal: ToolProposal::Connector(ConnectorProposal {
                    tool: binding,
                    arguments: json!({"value":"input"}),
                }),
            }),
        }),
    }
}
fn child(f: &mut Fixture, permit: &RunCallPermit, outcome: ModelOutcome) -> TaskView {
    f.state
        .finish_run_call(permit.run, permit.turn.id, permit.turn.attempt, outcome)
        .unwrap();
    let run = f.state.load_run(permit.run).unwrap();
    f.state
        .load_task(run.turns.last().unwrap().intent.as_ref().unwrap().task)
        .unwrap()
}

#[tokio::test]
async fn dynamic_model_tool_schema_and_binding_reach_an_exact_approved_task() {
    let mut f = fixture();
    let c = f.ready(false);
    let binding = f.grant(&c, f.bot, &["read"]).tools.remove(0);
    let request = input(&f, binding.clone());
    f.state.submit_run(token(), request).unwrap();
    let permit = claim(&mut f);
    let name = binding.public_name();
    let expected = name.clone();
    let model = permit.configuration.policy.model_id.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let provider = Provider::fixture(format!("http://{}", listener.local_addr().unwrap()));
    let app = axum::Router::new().route("/v1/chat/completions", axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
        let name = expected.clone(); let model = model.clone();
        async move {
            let tools = body["tools"].as_array().unwrap();
            assert_eq!(tools.len(), 1);
            assert_eq!(tools[0]["function"]["name"], name);
            assert_eq!(tools[0]["function"]["parameters"], tool("read").input_schema);
            axum::Json(json!({"id":"synthetic-connector-response","model":model,
                "choices":[{"index":0,"message":{"role":"assistant","content":null,"tool_calls":[{"id":"call-connector","type":"function","function":{"name":name,"arguments":"{\"value\":\"input\"}"}}]},"finish_reason":"tool_calls"}],
                "usage":{"prompt_tokens":12,"completion_tokens":8,"total_tokens":20}}))
        }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let outcome = provider
        .chat(
            ModelRequest {
                policy: &permit.configuration.policy,
                deadline: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64
                    + 10,
                max_output_tokens: 64,
                max_total_tokens: 200_000,
                prepared: &permit.prepared,
            },
            &permit.secret,
            None,
        )
        .await;
    let task = child(&mut f, &permit, outcome);
    assert!(!task.approval.consumed);
    let WorkSpec::Connector { spec } = &task.command.work else {
        panic!("ordinary connector task")
    };
    assert_eq!(
        spec.operation,
        ConnectorOperation::Call {
            proposal: ConnectorProposal {
                tool: binding.clone(),
                arguments: json!({"value":"input"})
            }
        }
    );
    assert_eq!(spec.result.max_bytes, MAX_RUN_TEXT_BYTES as u64);
    let persisted: String = f
        .state
        .connection
        .query_row(
            "SELECT provider_request FROM run_turns WHERE id=?1",
            [permit.turn.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert!(persisted.contains(&name));
    f.grant(&c, f.bot, &[]);
    f.grant(&c, f.bot, &["read"]);
    assert!(f.state.grant_approval(token(), task.id, 1400).is_err());
    server.abort();
}

#[test]
fn stale_model_turn_cannot_create_a_child_after_regrant() {
    let mut f = fixture();
    let c = f.ready(false);
    let binding = f.grant(&c, f.bot, &["read"]).tools.remove(0);
    f.state
        .submit_run(token(), input(&f, binding.clone()))
        .unwrap();
    let permit = claim(&mut f);
    f.grant(&c, f.bot, &[]);
    f.grant(&c, f.bot, &["read"]);
    f.state
        .finish_run_call(
            permit.run,
            permit.turn.id,
            permit.turn.attempt,
            response(binding, "stale"),
        )
        .unwrap();
    let run = f.state.load_run(permit.run).unwrap();
    assert!(run.turns[0].intent.is_none());
    assert!(matches!(run.summary.phase, RunPhase::Stopped { .. }));
}

#[test]
fn unknown_connector_stops_parent_and_late_evidence_never_resumes_it() {
    let mut f = fixture();
    let c = f.ready(true);
    let binding = f.grant(&c, f.bot, &["read"]).tools.remove(0);
    f.state
        .submit_run(token(), input(&f, binding.clone()))
        .unwrap();
    let permit = claim(&mut f);
    let task = child(&mut f, &permit, response(binding, "unknown"));
    let task = f.approve(task.id);
    let id = ConnectorCallId::new();
    f.reserve(&task, id);
    assert!(f.state.dispatch_connector(id).unwrap().is_some());
    f.state
        .finish_connector(
            id,
            ConnectorOutcome::Unknown {
                reason: ConnectorFailure::Transport,
            },
        )
        .unwrap();
    let before = f.state.load_run(permit.run).unwrap();
    assert!(matches!(
        before.summary.phase,
        RunPhase::AwaitingAttention { .. }
    ));
    f.state
        .settle_local_connector(peer(), task.id, task.command.execution, id, outcome())
        .unwrap();
    assert!(
        f.state
            .load_connector_call(id)
            .unwrap()
            .late_result
            .is_some()
    );
    assert_eq!(
        f.state.load_run(permit.run).unwrap().summary.phase,
        before.summary.phase
    );
    assert!(
        f.state
            .claim_run_work(&HashSet::new(), true)
            .unwrap()
            .is_none()
    );
}

#[test]
fn connector_policy_call_limit_counts_finished_children() {
    let mut f = fixture();
    let c = f.ready(false);
    let binding = f.grant(&c, f.bot, &["read"]).tools.remove(0);
    f.state
        .submit_run(token(), input(&f, binding.clone()))
        .unwrap();
    let permit = claim(&mut f);
    let task = child(&mut f, &permit, response(binding.clone(), "first"));
    f.finish_task(task.id);
    f.state
        .finish_run_result(
            permit.run,
            permit.turn.id,
            f.state.load_task(task.id).unwrap(),
            Ok("synthetic completed connector result".into()),
        )
        .unwrap();
    let permit = claim(&mut f);
    f.state
        .finish_run_call(
            permit.run,
            permit.turn.id,
            permit.turn.attempt,
            response(binding, "second"),
        )
        .unwrap();
    let view = f.state.load_run(permit.run).unwrap();
    assert!(view.turns[1].intent.is_none());
    assert_eq!(
        view.summary.phase,
        RunPhase::Stopped {
            reason: RunStopReason::OperationLimit
        }
    );
}

#[test]
fn run_result_uses_durable_connector_evidence_and_verifies_the_declared_receipt() {
    use crate::manager::runs::{CollectPermit, results};
    let mut f = fixture();
    let c = f.ready(true);
    let task = f.call(&c);
    let id = ConnectorCallId::new();
    f.reserve(&task, id);
    f.state.dispatch_connector(id).unwrap().unwrap();
    f.state.finish_connector(id, outcome()).unwrap();
    let artifact = ArtifactId::new();
    let directory = f.state.directory.clone();
    std::fs::create_dir_all(directory.join("artifacts")).unwrap();
    let path = directory.join("artifacts").join(format!("{artifact}.blob"));
    let mut task = task;
    task.state = TaskState::Completed;
    for valid in [true, false] {
        let bytes =
            serde_json::to_vec(&json!({"call": if valid {id} else {ConnectorCallId::new()}}))
                .unwrap();
        std::fs::write(&path, &bytes).unwrap();
        task.outputs[0].state = ArtifactState::Available {
            id: artifact,
            content: ArtifactContent {
                length: bytes.len() as u64,
                sha256: CertificateFingerprint::from_der(&bytes),
            },
        };
        let result = results::collect(CollectPermit {
            run: RunId::new(),
            turn: RunTurnId::new(),
            task: task.clone(),
            connectors: f.state.task_connector_calls(task.id).unwrap(),
            directory: directory.clone(),
            secret: f.state.provider_secret().unwrap(),
        });
        if valid {
            let result = result.unwrap();
            assert!(result.contains("recorded result"));
            assert!(result.contains(&id.to_string()));
            assert!(result.len() <= MAX_RUN_RESULT_BYTES);
        } else {
            assert!(result.is_err());
        }
    }
}

#[path = "catalog_bounds.rs"]
mod catalog_bounds;
