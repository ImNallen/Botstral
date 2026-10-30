fn review_token() -> SessionToken { crate::manager::state::inference::tests::token() }
fn review_catalog(state: &mut State, bot: BotId, marker: &str, schema_marker: bool)
 -> (ConnectorToolPolicy, ComputerSessionId, BotId, mpsc::Receiver<Envelope<ManagerMessage>>) {
    use crate::manager::connectors::ConnectorAdmission;
    let policy=state.inference_configuration(review_token(),bot).unwrap().policy;
    let bot=BotId::new();
    state.create_bot(review_token(),NewBot{id:bot,name:BotName::new("Independent catalog bot".into()).unwrap()}).unwrap();
    state.configure_inference(review_token(),ConfigureInference{bot,expected_revision:0,policy}).unwrap();
    let verification=state.prepare_inference_verification(review_token(),VerifyInference{bot,revision:1}).unwrap();
    state.finish_inference_verification(verification.configuration,verification.epoch,Some((true,false))).unwrap();
    let peer = || crate::worker_http::VerifiedIdentity::for_test(b"fresh independent connector peer");
    let invitation = state.invite_worker(review_token(), peer().fingerprint().clone(), 600).unwrap();
    let worker = state.enroll_worker(peer(), invitation.secret).unwrap();
    let session = state.open_computer_session(review_token(), bot, worker).unwrap().id;
    let (out, mut received) = mpsc::channel(32);
    let mut hello = Hello::current(false);
    hello.capabilities.extend([Capability::ProcessSystemdV1, Capability::ConnectorV1]);
    let lease = state.connect_worker(peer(), hello, out, watch::channel(false).0, RequestId::new()).unwrap();
    state.worker_report(lease, Envelope::new(WorkerMessage::SnapshotApplied { generation:lease.generation })).unwrap();
    let journal = JournalId::new();
    state.recovery_hello(lease,journal,0).unwrap();
    state.reconcile_inventory(lease,journal,0,0,vec![]).unwrap();
    let permission = state.permission(bot).unwrap();
    if !permission.allow { state.set_permission(review_token(),bot,permission.revision,true).unwrap(); }
    let connection = state.install_connection(review_token(), InstallConnection {
        id:ConnectionId::new(), label:"Independent catalog".into(),
        transport:ConnectionTransport::RemoteHttp { endpoint:"https://connector.example.test/mcp".into() }
    }).unwrap();
    let runner = ConnectorRunner { runtime:CertificateFingerprint::from_der(b"fresh connector runtime"),
      account:ProcessAccount{uid:1000,gid:1000}, limits:ProcessLimits{runtime_seconds:30,output_bytes:1024,processes:8,memory_bytes:67108864} };
    let task = state.discover_connection(review_token(), DiscoverConnection {
      task:TaskId::new(),session, expected:connection.version, runner:runner.clone(),deadline:state.now()+100
    }).unwrap();
    while received.try_recv().is_ok() {}
    let task = state.grant_approval(review_token(),task.id,state.now()+90).unwrap();
    let fence = state.find_computer_session(session).unwrap().fence(lease.generation);
    state.claim(lease,journal,fence,task.approval.binding.unwrap()).unwrap();
    let mut permit=None;
    while let Ok(message)=received.try_recv() { if matches!(message.message,ManagerMessage::Permit{..}) { permit=Some(message.message); } }
    state.admit_delivery(lease,&permit.unwrap()).unwrap();
    let operation = match &task.command.work { WorkSpec::Connector{spec}=>spec.operation.clone(), _=>panic!() };
    let call=ConnectorCallId::new();
    assert!(matches!(state.admit_connector(peer(),task.id,task.command.execution,call,operation).unwrap(),ConnectorAdmission::Reserved(_)));
    assert!(state.dispatch_connector(call).unwrap().is_some());
    let input_schema=if schema_marker {
      serde_json::json!({"type":"object","properties":{marker:{"type":"string"}},"additionalProperties":false})
    } else { serde_json::json!({"type":"object","properties":{},"additionalProperties":false}) };
    state.finish_connector(call,ConnectorOutcome::Finished { result:ConnectorResult::Catalog { tools:vec![McpTool {
      name:"lookup".into(),description:if schema_marker { "Independent lookup".into() } else { format!("Look up existing account {marker}.") },input_schema
    }] } }).unwrap();
    let mut done=state.load_task(task.id).unwrap(); done.state=TaskState::Completed;
    crate::manager::state::tasks::save(&state.connection,&done).unwrap();
    let connection=state.load_connection(connection.id).unwrap();
    assert_eq!(connection.availability,ConnectionAvailability::Ready);
    let old=state.connector_grants(review_token(),bot,connection.id).unwrap();
    let grants=state.set_connector_grants(review_token(),bot,SetConnectorGrants {
      connection:connection.version,catalog_revision:connection.catalog_revision,expected_revision:old.revision,tools:vec!["lookup".into()]
    }).unwrap();
    (ConnectorToolPolicy{tools:grants.tools,max_calls:1,max_result_bytes:8192,runner},session,bot,received)
}
fn review_conversation(state: &mut State, bot:BotId, session:ComputerSessionId, connectors:Option<ConnectorToolPolicy>) -> Result<(),OperationError> {
    let configuration=state.inference_configuration(review_token(),bot)?;
    let request=SubmitRun { id:RunId::new(),bot,session:Some(session),configuration_revision:configuration.revision,
      prompt:"Hello.".into(), images:vec![], limits:RunLimits { deadline:state.now()+100,max_turns:2,max_total_tokens:200000,
      max_cost_microusd:200000,max_output_tokens:64,max_operations:4,max_concurrent_operations:1 },
      execution_policy:RunExecutionPolicy { connectors,vision:None,vibe:None,process:None,computer:None } };
    let id=request.id;
    state.submit_run(review_token(),request)?;
    let job=state.claim_run_work(&std::collections::HashSet::new(),true)?.unwrap();
    let crate::manager::runs::RunJob::Call(permit)=job else { panic!("expected real provider call permit"); };
    assert_eq!(permit.run,id);
    state.finish_run_call(id,permit.turn.id,permit.turn.attempt,crate::manager::provider::ModelOutcome::Billed {
      usage:InferenceUsage{prompt_tokens:12,completion_tokens:8,total_tokens:20},request_id:"independent-response".into(),
      response:Ok(RunMessage::Assistant{content:"Independent completion succeeded.".into(),tool_call:None}),finish_reason:Some(InferenceFinishReason::Stop)
    })?;
    let view=state.run(review_token(),id)?;
    assert_eq!(view.summary.phase,RunPhase::Completed);
    assert!(serde_json::to_string(&view).unwrap().contains("Independent completion succeeded."));
    Ok(())
}
