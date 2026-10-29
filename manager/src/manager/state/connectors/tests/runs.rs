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
        session: Some(f.session),
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
            assert_eq!(tools.len(), 3);
            assert_eq!(tools[2]["function"]["name"], name);
            assert_eq!(tools[2]["function"]["parameters"], tool("read").input_schema);
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

#[tokio::test]
async fn p03_duplicate_authenticated_event_restart_has_one_external_connector_effect() {
    use sha2::{Digest, Sha256};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let mut f = fixture();
    let c = f.ready(false);
    let binding = f.grant(&c, f.bot, &["write"]).tools.remove(0);
    let input = input(&f, binding.clone());
    let publisher = PublisherId::new();
    let credential = "p".repeat(48);
    let routine = RoutineId::new();
    let commands = [
        RoutineAction::SetBudget {
            budget: RoutineBudget {
                max_runs: 4,
                max_tokens: 2_000_000,
                max_cost_microusd: 2_000_000,
            },
        },
        RoutineAction::InstallPublisher {
            id: publisher,
            bot: f.bot,
            name: "Build publisher".into(),
            topics: vec!["build.done".into()],
            credential_sha256: format!("{:x}", Sha256::digest(credential.as_bytes())),
        },
        RoutineAction::Create {
            id: routine,
            definition: Box::new(RoutineDefinition {
                bot: f.bot,
                name: "Publish build result".into(),
                trigger: RoutineTrigger::Event {
                    publisher,
                    topic: "build.done".into(),
                    equals: Default::default(),
                },
                catchup_seconds: 60,
                wakeups: None,
                template: RunTemplate {
                    session: f.session,
                    session_revision: f.state.find_computer_session(f.session).unwrap().revision,
                    configuration_revision: 1,
                    permission_revision: 1,
                    prompt: input.prompt,
                    execution_policy: input.execution_policy,
                    duration_seconds: 500,
                    max_turns: 4,
                    max_total_tokens: 200_000,
                    max_cost_microusd: 200_000,
                    max_output_tokens: 64,
                    max_operations: 4,
                },
            }),
        },
    ];
    for action in commands {
        f.state
            .routine_command(
                token(),
                RoutineCommand {
                    id: RequestId::new(),
                    action,
                },
            )
            .unwrap();
    }
    let bytes = serde_json::to_vec(&PublisherEvent {
        id: "build-42".into(),
        timestamp: 1000,
        topic: "build.done".into(),
        fields: Default::default(),
    })
    .unwrap();
    f.state
        .ingest_routine_event(publisher, credential.clone(), bytes.clone())
        .unwrap();
    f.state
        .ingest_routine_event(publisher, credential.clone(), bytes.clone())
        .unwrap();
    let run = f.state.routine_history(token(), routine, None, 32).unwrap()[0]
        .run
        .unwrap();
    let permit = claim(&mut f);
    let task = child(&mut f, &permit, response(binding, "write-result"));
    let task = f.approve(task.id);
    let id = ConnectorCallId::new();
    f.reserve(&task, id);
    let mut dispatch = f.state.dispatch_connector(id).unwrap().unwrap();
    let effects = Arc::new(AtomicUsize::new(0));
    let counted = effects.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    dispatch.connection.transport = ConnectionTransport::RemoteHttp {
        endpoint: format!("http://{}/mcp", listener.local_addr().unwrap()),
    };
    dispatch.deadline = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        + 30;
    let app=axum::Router::new().route("/mcp",axum::routing::post(move |axum::Json(body):axum::Json<serde_json::Value>| {
        let effects=counted.clone();async move {
            use axum::response::IntoResponse;
            let result=match body["method"].as_str().unwrap() {
                "initialize"=>json!({"protocolVersion":MCP_VERSION,"capabilities":{"tools":{}},"serverInfo":{"name":"counted fixture","version":"1"}}),
                "notifications/initialized"=>return axum::http::StatusCode::ACCEPTED.into_response(),
                "tools/list"=>json!({"tools":[{"name":"read","description":tool("read").description,"inputSchema":tool("read").input_schema},{"name":"write","description":tool("write").description,"inputSchema":tool("write").input_schema}]}),
                "tools/call"=>{effects.fetch_add(1,Ordering::SeqCst);json!({"content":[{"type":"text","text":"External write counted"}]})},
                method=>panic!("unexpected {method}"),
            };
            axum::Json(json!({"jsonrpc":"2.0","id":body["id"],"result":result})).into_response()
        }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let outcome = crate::manager::connectors::execute_fixture(&dispatch).await;
    assert!(
        matches!(outcome, ConnectorOutcome::Finished { .. }),
        "{outcome:?}"
    );
    f.state.finish_connector(id, outcome).unwrap();
    assert_eq!(effects.load(Ordering::SeqCst), 1);
    f.finish_task(task.id);
    f.state
        .finish_run_result(
            run,
            permit.turn.id,
            f.state.load_task(task.id).unwrap(),
            Ok("External write counted".into()),
        )
        .unwrap();
    let last = claim(&mut f);
    f.state
        .finish_run_call(
            run,
            last.turn.id,
            last.turn.attempt,
            ModelOutcome::Billed {
                usage: InferenceUsage {
                    prompt_tokens: 12,
                    completion_tokens: 8,
                    total_tokens: 20,
                },
                request_id: "done".into(),
                response: Ok(RunMessage::Assistant {
                    content: "External write counted".into(),
                    tool_call: None,
                }),
                finish_reason: Some(InferenceFinishReason::Stop),
            },
        )
        .unwrap();
    f.state.reconcile_routines().unwrap();
    let path = f.state.directory.join("manager.sqlite3");
    f.state
        .connection
        .execute("VACUUM INTO ?1", [path.to_str().unwrap()])
        .unwrap();
    let installation = std::mem::replace(
        &mut f.state.installation,
        Installation::Configured {
            vault: VaultState::Locked,
        },
    );
    let (configured, _) = watch::channel(false);
    let mut state = State::open(f.state.directory.clone(), configured).unwrap();
    state.test_time = Some(1000);
    state.installation = installation;
    state
        .ingest_routine_event(publisher, credential, bytes)
        .unwrap();
    assert!(state.dispatch_connector(id).unwrap().is_none());
    assert_eq!(effects.load(Ordering::SeqCst), 1);
    let inbox = state.inbox().unwrap();
    assert_eq!(inbox[0].run, Some(run));
    assert!(inbox[0].message.contains("counted"));
    server.abort();
}

#[test]
fn parent_cancel_keeps_stopped_unknown_child() {
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
    f.state.dispatch_connector(id).unwrap().unwrap();
    f.state
        .finish_connector(
            id,
            ConnectorOutcome::Unknown {
                reason: ConnectorFailure::Transport,
            },
        )
        .unwrap();
    let reports = stop_reports(&f, &task);
    f.state
        .durable_reports(f.lease, f.journal, 2, reports)
        .unwrap();
    let stopped = f.state.load_task(task.id).unwrap();
    assert_eq!(stopped.state, TaskState::AwaitingAttention);
    f.state.cancel_run(token(), permit.run).unwrap();
    f.state.reconcile_run_stops().unwrap();
    assert_eq!(f.state.load_task(task.id).unwrap(), stopped);
    assert!(matches!(
        f.state.load_run(permit.run).unwrap().summary.phase,
        RunPhase::AwaitingAttention { .. }
    ));
    assert!(
        f.state
            .claim_run_work(&HashSet::new(), true)
            .unwrap()
            .is_none()
    );
    assert!(f.state.dispatch_connector(id).unwrap().is_none());
}

#[test]
fn conversational_memory_excludes_connector_bearer_environment_and_rotated_epochs() {
    for local in [false, true] {
        let mut f = fixture();
        let mut connection = f.install(local);
        let secret = "synthetic-connector-memory-secret";
        let replace = |value: &str| {
            if local {
                ConnectionSecretInput::LocalEnvironment {
                    values: std::collections::BTreeMap::from([(
                        "ACCESS_TOKEN".into(),
                        ConnectorSecretEntry(value.into()),
                    )]),
                }
            } else {
                ConnectionSecretInput::Bearer {
                    value: ConnectorSecretEntry(value.into()),
                }
            }
        };
        let source = MemoryEntry {
            id: MemoryId::new(),
            scope: MemoryScope::Individual { bot: f.bot },
            revision: 1,
            markdown: secret.into(),
            source: MemorySource::Run {
                bot: f.bot,
                run: RunId::new(),
                turn: RunTurnId::new(),
            },
        };
        let mut header = source.clone();
        header.markdown.clear();
        let path = f.state.directory.join(format!("entry-{}.md", source.id));
        std::fs::write(
            &path,
            format!(
                "<!-- botstral-memory-entry {} -->\n{secret}",
                json!({"type":"live","entry":header})
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut request = SubmitRun {
            id: RunId::new(),
            bot: f.bot,
            session: None,
            images: vec![],
            configuration_revision: 1,
            prompt: "A memory conversation.".into(),
            execution_policy: RunExecutionPolicy {
                computer: None,
                process: None,
                vibe: None,
                vision: None,
                connectors: None,
            },
            limits: RunLimits {
                deadline: 1500,
                max_turns: 4,
                max_total_tokens: 200000,
                max_cost_microusd: 200000,
                max_output_tokens: 64,
                max_operations: 4,
                max_concurrent_operations: 1,
            },
        };
        f.state.submit_run(token(), request.clone()).unwrap();
        connection = f
            .state
            .replace_connection_secret(
                token(),
                connection.id,
                ReplaceConnectionSecret {
                    expected_revision: connection.version.revision,
                    secret: replace(secret),
                },
            )
            .unwrap();
        assert!(
            f.state
                .claim_run_work(&HashSet::new(), true)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            f.state.load_run(request.id).unwrap().summary.phase,
            RunPhase::AwaitingAttention {
                reason: RunAttentionReason::ResultInvalid
            }
        );
        assert_eq!(
            f.state.memory_context(f.bot),
            Err(OperationError::InvalidInput)
        );
        std::fs::remove_file(path).unwrap();
        for text in [secret.to_owned(), serde_json::to_string(secret).unwrap()] {
            request.id = RunId::new();
            request.prompt = text.clone();
            assert_eq!(
                f.state.submit_run(token(), request.clone()),
                Err(OperationError::InvalidInput)
            );
            assert_eq!(
                f.state
                    .prepare_memory_change(
                        RunId::new(),
                        f.bot,
                        RunTurnId::new(),
                        &MemoryProposal::Create {
                            scope: MemoryScopeSelection::Shared,
                            markdown: text
                        }
                    )
                    .err(),
                Some(OperationError::InvalidInput)
            );
        }
        f.state
            .replace_connection_secret(
                token(),
                connection.id,
                ReplaceConnectionSecret {
                    expected_revision: connection.version.revision,
                    secret: replace("synthetic-new-connector-secret"),
                },
            )
            .unwrap();
        assert_eq!(
            f.state.reject_known_secrets(&secret),
            Err(OperationError::InvalidInput)
        );
        assert_eq!(
            f.state
                .reject_known_secrets(&"synthetic-new-connector-secret"),
            Err(OperationError::InvalidInput)
        );
    }
}
