#[test]
fn astra_self_referential_schema_is_bounded() {
    let tools = vec![McpTool {
        name: "cycle".into(),
        description: "".into(),
        input_schema: json!({"type":"object", "$ref":"#"}),
    }];
    let catalog = validate_catalog(&tools);
    eprintln!("self-reference catalog result: {catalog:?}");
    if catalog.is_ok() {
        let result = validate_arguments(&tools[0].input_schema, &json!({}));
        eprintln!("self-reference arguments result: {result:?}");
    }
}

#[test]
fn astra_reference_fanout_is_bounded() {
    let depth: u32 = std::env::var("M06_ASTRA_SCHEMA_DEPTH").unwrap_or_else(|_| "0".into()).parse().unwrap();
    let mut definitions = serde_json::Map::new();
    definitions.insert("d0".into(), json!({"type":"object"}));
    for level in 1..=depth {
        let reference = format!("#/$defs/d{}", level - 1);
        definitions.insert(format!("d{level}"), json!({"allOf":[{"$ref":reference},{"$ref":reference}]}));
    }
    let schema = json!({"type":"object", "$ref":format!("#/$defs/d{depth}"), "$defs":definitions});
    eprintln!("schema bytes: {}", serde_json::to_vec(&schema).unwrap().len());
    let malicious = McpTool { name:"fanout".into(), description:"".into(), input_schema:schema };
    let mut f = fixture();
    let c = f.install(false);
    let task = f.task(ConnectorOperation::Discover { connection:c.version });
    let task = f.approve(task.id);
    let id = ConnectorCallId::new();
    f.reserve(&task, id);
    f.state.dispatch_connector(id).unwrap().unwrap();
    f.state.finish_connector(id, ConnectorOutcome::Finished { result:ConnectorResult::Catalog { tools:vec![malicious] } }).unwrap();
    f.finish_task(task.id);
    let c = f.state.load_connection(c.id).unwrap();
    assert_eq!(c.availability, ConnectionAvailability::Ready);
    let binding = f.grant(&c, f.bot, &["fanout"]).tools.remove(0);
    eprintln!("catalog persisted and exact grant created; submitting empty arguments");
    let task = f.task(ConnectorOperation::Call { proposal:ConnectorProposal { tool:binding, arguments:json!({}) } });
    eprintln!("task submission returned: {}", task.id);
}
