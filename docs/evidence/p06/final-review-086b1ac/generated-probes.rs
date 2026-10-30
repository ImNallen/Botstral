mod fresh_catalog_fixture { include!("common.rs"); }

#[test]
fn fresh_catalog_generated_codes_nested_numeric_encoded_and_raw_binding() {
    use fresh_catalog_fixture as c;
    for shape in ["raw_name", "nested_enum", "numeric_default", "encoded_description"] {
        let mut login=live_values();
        login.totp_seed="JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP".into();
        let (code,_)=totp::current(&login.totp_seed,1000).unwrap();
        let encoded=code.bytes().map(|b|format!("%{b:02X}")).collect::<String>();
        let mut tool=McpTool{name:format!("independent_{}",RequestId::new()),description:"Independent catalog".into(),input_schema:serde_json::json!({"type":"object","properties":{}})};
        match shape {
            "raw_name" => tool.name=format!("verify_{}",code.as_str()),
            "nested_enum" => tool.input_schema=serde_json::json!({"type":"object","properties":{"q":{"type":"string","enum":[code.as_str()]}}}),
            "numeric_default" => tool.input_schema=serde_json::json!({"type":"object","properties":{"q":{"type":"integer","default":code.parse::<u64>().unwrap()}}}),
            _ => tool.description=format!("Current encoded identifier {encoded}"),
        }
        let (mut f,catalog)=live_with_setup(plan("https://example.test"),CredentialFields::Totp,PasswordPolicy::EveryFill,1000,login,|state,bot|c::prepare(state,bot,tool));
        let task_bot=f.state.load_task(f.authority.task).unwrap().command.bot;
        assert_ne!(catalog.bot,task_bot);
        c::complete(&mut f.state,catalog.bot,catalog.session,Some(catalog.policy.clone())).unwrap();
        let review=f.prepare(None);
        f.state.decide_credential_use(owner(),review.clone(),UseDecision::AllowOnce).unwrap();
        for _ in 0..2 { assert!(matches!(f.release(&review),Err(OperationError::Conflict)),"shape={shape}"); }
        assert_eq!(f.state.connection.query_row("SELECT count(*) FROM credential_codes",[],|r|r.get::<_,i64>(0)).unwrap(),0);
        assert_eq!(f.state.credential_uses(owner()).unwrap()[0].state,UseState::Authorized);
        c::complete(&mut f.state,catalog.bot,catalog.session,Some(catalog.policy.clone())).unwrap();
        f.state.update_login(owner(),f.item.id,f.item.revision,None).unwrap();
        f.state.test_time=Some(1030);
        c::complete(&mut f.state,catalog.bot,catalog.session,Some(catalog.policy)).unwrap();
        c::complete(&mut f.state,catalog.bot,catalog.session,None).unwrap();
        println!("fresh_generated shape={shape}; cross_bot=true; catalog_preexists=true; repeated_release_refused=true; retained=0; authorized=true; expiry_removal_complete=true");
    }
}
