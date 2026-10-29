use super::*;

fn large_bindings(f: &mut Fixture, count: usize) -> Vec<ConnectorToolBinding> {
    let mut bindings = Vec::new();
    for _ in 0..count {
        let connection = f.install(false);
        let task = f.task(ConnectorOperation::Discover {
            connection: connection.version,
        });
        let task = f.approve(task.id);
        let id = ConnectorCallId::new();
        f.reserve(&task, id);
        assert!(f.state.dispatch_connector(id).unwrap().is_some());
        let large_tool = McpTool {
            name: "large".into(),
            description: "A valid tool with a bounded documented input schema.".into(),
            input_schema: json!({"type":"object","description":"x".repeat(7000)}),
        };
        let call = f
            .state
            .finish_connector(
                id,
                ConnectorOutcome::Finished {
                    result: ConnectorResult::Catalog {
                        tools: vec![large_tool],
                    },
                },
            )
            .unwrap();
        assert!(matches!(
            call.state,
            ConnectorCallState::Resolved {
                outcome: ConnectorOutcome::Finished { .. }
            }
        ));
        f.finish_task(task.id);
        let connection = f.state.load_connection(connection.id).unwrap();
        bindings.push(f.grant(&connection, f.bot, &["large"]).tools.remove(0));
    }
    bindings
}

#[test]
fn oversized_connector_catalog_is_rejected_and_legacy_runs_do_not_block_healthy_work() {
    let mut f = fixture();
    let bindings = large_bindings(&mut f, 10);
    let mut request = input(&f, bindings[0].clone());
    request.execution_policy.connectors.as_mut().unwrap().tools = bindings.clone();
    assert!(matches!(
        f.state.submit_run(token(), request.clone()),
        Err(OperationError::InvalidInput)
    ));
    let healthy = f
        .state
        .submit_run(token(), input(&f, bindings[0].clone()))
        .unwrap();
    let mut legacy = healthy.summary.clone();
    legacy.id = request.id;
    f.state
        .connection
        .execute(
            "INSERT INTO runs(rowid,id,owner,request,configuration,summary) VALUES(0,?1,?2,?3,?4,?5)",
            params![
                request.id.to_string(),
                f.state.principal().unwrap().to_string(),
                serde_json::to_string(&request).unwrap(),
                serde_json::to_string(&healthy.configuration).unwrap(),
                serde_json::to_string(&legacy).unwrap()
            ],
        )
        .unwrap();
    let permit = claim(&mut f);
    assert_eq!(permit.run, healthy.request.id);
    assert_eq!(
        f.state.load_run(legacy.id).unwrap().summary.phase,
        RunPhase::Stopped {
            reason: RunStopReason::TranscriptLimit
        }
    );
}

#[test]
fn p01_complete_provider_request_includes_memory_in_admission_bound() {
    let mut f = fixture();
    let bindings = large_bindings(&mut f, 8);
    let mut request = input(&f, bindings[0].clone());
    request.execution_policy.connectors.as_mut().unwrap().tools = bindings;
    f.state.submit_run(token(), request.clone()).unwrap();
    let p = claim(&mut f);
    assert!(p.prepared.encoded().len() < crate::manager::provider::MAX_PROVIDER_REQUEST);
    f.state.cancel_run(token(), request.id).unwrap();
    f.state
        .write_document(
            token(),
            WriteDocument {
                bot: f.bot,
                kind: DocumentKind::Memory,
                expected_revision: 0,
                sources: vec![SourceReference::External {
                    reference: "https://example.com/provider-bound".into(),
                }],
                markdown: "\\".repeat(4096),
            },
        )
        .unwrap();
    request.id = RunId::new();
    assert_eq!(
        f.state.submit_run(token(), request.clone()),
        Err(OperationError::InvalidInput)
    );
    assert_eq!(f.state.load_run(request.id), Err(OperationError::NotFound));
    assert!(
        f.state
            .claim_run_work(&std::collections::HashSet::new(), true)
            .unwrap()
            .is_none()
    );
}
