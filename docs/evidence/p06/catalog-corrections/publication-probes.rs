include!("common.rs");
#[test]
fn independent_current_catalog_intake_preserves_complete_conversation() {
    let mut all_usable=true;
    for schema_marker in [false,true] {
        let mut f=fixture();
        let marker=format!("prior-account-{}",RequestId::new());
        let (policy,session,bot,_received)=review_catalog(&mut f.state,f.input.bot,&marker,schema_marker);
        f.input.bot=bot;
        review_conversation(&mut f.state,f.input.bot,session,Some(policy.clone())).unwrap();
        let request=RequestId::new();
        let status=f.state.write_login(token(),request,CredentialWriteTarget::Create{metadata:login_metadata()},private_values(&marker)).unwrap();
        let committed=matches!(status,CredentialWriteStatus::Committed{..});
        let after=review_conversation(&mut f.state,f.input.bot,session,Some(policy.clone()));
        let mut rotated=after.clone(); let mut removed=after.clone();
        if let CredentialWriteStatus::Committed{item,..}=status {
            let CredentialWriteStatus::Committed{item,..}=f.state.write_login(token(),RequestId::new(),CredentialWriteTarget::Rotate {
                id:item.id,expected_revision:item.revision
            },private_values(&RequestId::new().to_string())).unwrap() else { panic!("rotation failed"); };
            rotated=review_conversation(&mut f.state,f.input.bot,session,Some(policy.clone()));
            f.state.update_login(token(),item.id,item.revision,None).unwrap();
            assert!(f.state.accessible_login_items(f.input.bot).unwrap().is_empty());
            removed=review_conversation(&mut f.state,f.input.bot,session,Some(policy));
        }
        let plain=review_conversation(&mut f.state,f.input.bot,session,None);
        println!("catalog_schema={schema_marker}; baseline_complete=true; intake_committed={committed}; after={after:?}; after_rotation={rotated:?}; after_removal={removed:?}; plain={plain:?}");
        all_usable &= after.is_ok() && rotated.is_ok() && removed.is_ok();
    }
    assert!(all_usable,"accepted value broke unchanged preexisting enabled connector conversation");
}
#[test]
fn independent_fresh_configuration_and_memory_rejection_preserves_completion() {
    for memory in [false,true] {
        let mut f=fixture(); let marker=format!("existing-context-{}",RequestId::new());
        if memory { super::memory_tests::remember(&mut f,1,&format!("Existing note {marker}.")); }
        else {
            let mut policy=f.state.inference_configuration(token(),f.input.bot).unwrap().policy;
            policy.model_id=marker.clone();
            f.state.configure_inference(token(),ConfigureInference{bot:f.input.bot,expected_revision:1,policy}).unwrap();
            let p=f.state.prepare_inference_verification(token(),VerifyInference{bot:f.input.bot,revision:2}).unwrap();
            f.state.finish_inference_verification(p.configuration,p.epoch,Some((true,false))).unwrap();
        }
        let session=f.input.session.unwrap();
        review_conversation(&mut f.state,f.input.bot,session,None).unwrap();
        let id=RequestId::new();
        let status=f.state.write_login(token(),id,CredentialWriteTarget::Create{metadata:login_metadata()},private_values(&marker)).unwrap();
        assert_eq!(status,CredentialWriteStatus::Rejected{request:id,reason:CredentialWriteRejection::PublicValueCollision});
        assert_eq!(f.state.write_login(token(),id,CredentialWriteTarget::Create{metadata:login_metadata()},private_values(&RequestId::new().to_string())).unwrap(),status);
        review_conversation(&mut f.state,f.input.bot,session,None).unwrap();
        let n:i64=f.state.connection.query_row("SELECT count(*) FROM credential_epochs",[],|r|r.get(0)).unwrap(); assert_eq!(n,0);
        println!("fresh_current_memory={memory}; baseline_and_after_complete=true; rejected_final=true; epochs=0");
    }
}
#[test]
fn independent_typed_provider_wire_shapes_preserved() {
    use serde_json::json;
    let f=fixture();
    let policy=f.state.inference_configuration(token(),f.input.bot).unwrap().policy;
    let messages=vec![
      RunMessage::User{content:"plain user".into()},
      RunMessage::Assistant{content:"plain assistant".into(),tool_call:None},
      RunMessage::Assistant{content:"tool assistant".into(),tool_call:Some(RunToolCall {
        id:"independent-call".into(),proposal:ToolProposal::Memory(MemoryProposal::List{scope:MemoryScopeSelection::Individual})
      })},
      RunMessage::Tool{content:"tool result".into(),tool_call_id:"independent-call".into()},
      RunMessage::UserMultimodal{content:"empty images".into(),images:vec![]}
    ];
    let mut prepared=crate::manager::provider::PreparedChat::new(&policy,64,&messages,None);
    assert_eq!(prepared.body["messages"],json!([
      {"role":"user","content":"plain user"},
      {"role":"assistant","content":"plain assistant"},
      {"role":"assistant","content":"tool assistant","tool_calls":[{"id":"independent-call","type":"function","function":{"name":"memory","arguments":"{\"action\":\"list\",\"scope\":\"individual\"}"}}]},
      {"role":"tool","content":"tool result","tool_call_id":"independent-call"},
      {"role":"user","content":"empty images","authorized_images":[]}
    ]));
    assert_eq!(prepared.body["tool_choice"],"none");
    assert!(prepared.body.get("tools").is_none());
    assert!(prepared.body.get("parallel_tool_calls").is_none());
    prepared.materialize_images(&f.state.directory).unwrap();
    assert!(prepared.body["messages"][4].get("authorized_images").is_none());
    assert_eq!(prepared.body["messages"][4]["content"],json!([{"type":"text","text":"empty images"}]));
    println!("typed_user_assistant_tool_calls_tool_and_optional_image_json_preserved=true; no_tools_envelope_preserved=true");
}
