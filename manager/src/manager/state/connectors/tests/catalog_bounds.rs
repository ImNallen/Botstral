use super::*;

#[test]
fn oversized_connector_catalog_is_rejected_and_legacy_runs_do_not_block_healthy_work() {
    let mut f = fixture();
    let mut bindings = Vec::new();
    for _ in 0..10 {
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
