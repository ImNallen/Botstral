use super::*;

fn referenced_schema(depth: u32) -> serde_json::Value {
    let mut definitions = serde_json::Map::new();
    definitions.insert("d0".into(), json!({"type":"object"}));
    for level in 1..=depth {
        let reference = format!("#/$defs/d{}", level - 1);
        definitions.insert(
            format!("d{level}"),
            json!({"allOf":[{"$ref":reference},{"$ref":reference}]}),
        );
    }
    json!({"type":"object", "$ref":format!("#/$defs/d{depth}"), "$defs":definitions})
}

#[test]
fn schema_expansion_is_bounded_at_discovery_and_call_admission() {
    let simple = referenced_schema(2);
    assert!(validate_arguments(&simple, &json!({})).is_ok());
    let mut f = fixture();
    let connection = f.install(false);
    let task = f.task(ConnectorOperation::Discover {
        connection: connection.version,
    });
    let task = f.approve(task.id);
    let id = ConnectorCallId::new();
    f.reserve(&task, id);
    f.state.dispatch_connector(id).unwrap().unwrap();
    let schema = referenced_schema(32);
    assert!(bounded_connector_json(&schema, 8192));
    let call = f
        .state
        .finish_connector(
            id,
            ConnectorOutcome::Finished {
                result: ConnectorResult::Catalog {
                    tools: vec![McpTool {
                        name: "fanout".into(),
                        description: String::new(),
                        input_schema: schema.clone(),
                    }],
                },
            },
        )
        .unwrap();
    assert!(matches!(
        call.state,
        ConnectorCallState::Resolved {
            outcome: ConnectorOutcome::Unknown {
                reason: ConnectorFailure::Protocol
            }
        }
    ));
    assert_ne!(
        f.state.load_connection(connection.id).unwrap().availability,
        ConnectionAvailability::Ready
    );
    assert!(validate_arguments(&schema, &json!({})).is_err());
    assert!(validate_arguments(&simple, &json!({})).is_ok());
}

#[test]
fn recursive_and_scope_changing_schema_references_are_refused() {
    for schema in [
        json!({"type":"object", "$ref":"#"}),
        json!({"type":"object", "$defs":{"cycle":{"$ref":"#/$defs/cycle"}}, "$ref":"#/$defs/cycle"}),
        json!({"type":"object", "$id":"https://example.com/schema", "$ref":"#/properties/value"}),
        json!({"type":"object", "$dynamicRef":"#anchor"}),
        json!({"type":"object", "$recursiveRef":"#"}),
    ] {
        assert!(
            validate_catalog(&[McpTool {
                name: "read".into(),
                description: String::new(),
                input_schema: schema.clone()
            }])
            .is_err()
        );
        assert!(validate_arguments(&schema, &json!({})).is_err());
    }
}
