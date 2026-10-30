mod fresh_catalog_fixture { include!("common.rs"); }

#[test]
fn fresh_catalog_intake_nested_encoded_and_identity_cases() {
    use fresh_catalog_fixture as c;
    for shape in ["nested_enum", "encoded_description", "binding_identity", "schema_digest", "rendered_alias", "raw_name"] {
        for password in [false, true] {
            let mut f = fixture();
            let marker = format!("fresh-{}", RequestId::new());
            let encoded = marker.bytes().map(|b| format!("%{b:02X}")).collect::<String>();
            let mut tool = McpTool { name: format!("tool_{}", RequestId::new()), description: "Independent lookup".into(), input_schema: serde_json::json!({"type":"object","properties":{}}) };
            if shape == "nested_enum" { tool.input_schema = serde_json::json!({"type":"object","properties":{"query":{"type":"array","items":{"type":"string","enum":[marker]}}}}); }
            if shape == "encoded_description" { tool.description = format!("Existing encoded label {encoded}"); }
            let catalog = c::prepare(&mut f.state, f.input.bot, tool);
            assert_ne!(catalog.bot, f.input.bot);
            let binding = &catalog.policy.tools[0];
            let candidate = match shape {
                "binding_identity" => binding.connection.connection.to_string(),
                "schema_digest" => binding.schema_digest.as_str().to_string(),
                "rendered_alias" => binding.public_name(),
                "raw_name" => binding.name.clone(),
                _ => marker,
            };
            c::complete(&mut f.state, catalog.bot, catalog.session, Some(catalog.policy.clone())).unwrap();
            let make_values = || { let mut v = private_values(&RequestId::new().to_string()); if password { v.password = candidate.clone(); } else { v.username = candidate.clone(); } v };
            let mut metadata = login_metadata();
            metadata.bots = BotAccess::Selected { bots: vec![f.input.bot] };
            let id = RequestId::new();
            let expected = CredentialWriteStatus::Rejected { request:id, reason:CredentialWriteRejection::PublicValueCollision };
            let status = f.state.write_login(token(), id, CredentialWriteTarget::Create { metadata:metadata.clone() }, make_values()).unwrap();
            assert_eq!(status, expected, "shape={shape}; password={password}");
            assert!(!serde_json::to_string(&status).unwrap().contains(&candidate));
            assert_eq!(f.state.write_login(token(), id, CredentialWriteTarget::Create { metadata:metadata.clone() }, private_values(&RequestId::new().to_string())).unwrap(), expected);
            let count = |state:&State| state.connection.query_row("SELECT count(*) FROM credential_epochs", [], |r| r.get::<_,i64>(0)).unwrap();
            assert_eq!(count(&f.state), 0);
            let safe_username = format!("safe-user-{}", RequestId::new());
            let safe = private_values(&safe_username);
            let safe_password = safe.password.clone();
            let CredentialWriteStatus::Committed { item,.. } = f.state.write_login(token(), RequestId::new(), CredentialWriteTarget::Create { metadata }, safe).unwrap() else { panic!("safe control failed"); };
            let rotation = RequestId::new();
            let target = CredentialWriteTarget::Rotate { id:item.id, expected_revision:item.revision };
            let rejected = f.state.write_login(token(), rotation, target.clone(), make_values()).unwrap();
            assert_eq!(rejected, CredentialWriteStatus::Rejected { request:rotation, reason:CredentialWriteRejection::PublicValueCollision });
            assert_eq!(f.state.write_login(token(), rotation, target, private_values(&RequestId::new().to_string())).unwrap(), rejected);
            assert_eq!(f.state.load_login(item.id).unwrap(), item);
            assert_eq!(count(&f.state), 1);
            c::complete(&mut f.state, catalog.bot, catalog.session, Some(catalog.policy.clone())).unwrap();
            let CredentialWriteStatus::Committed { item,.. } = f.state.write_login(token(), RequestId::new(), CredentialWriteTarget::Rotate { id:item.id,expected_revision:item.revision }, private_values(&RequestId::new().to_string())).unwrap() else { panic!("safe rotation failed"); };
            f.state.update_login(token(), item.id,item.revision,None).unwrap();
            assert_eq!(count(&f.state), 2);
            assert!(f.state.reject_known_secrets(&safe_username).is_err());
            assert!(f.state.reject_known_secrets(&safe_password).is_err());
            c::complete(&mut f.state, catalog.bot,catalog.session,Some(catalog.policy)).unwrap();
            c::complete(&mut f.state, catalog.bot,catalog.session,None).unwrap();
            println!("fresh_intake shape={shape} password={password} cross_bot_selected_scope=true final_replay=true rotation_atomic=true retired_screened=true complete=true");
        }
    }
}

#[test]
fn fresh_catalog_filtering_matches_usable_inventory() {
    use fresh_catalog_fixture as c;
    for mode in ["permission", "empty_grants", "stale_grant", "reconnect"] {
        let mut f=fixture();
        let marker=format!("filter-{}",RequestId::new());
        let tool=McpTool{name:format!("tool_{}",RequestId::new()),description:marker.clone(),input_schema:serde_json::json!({"type":"object","properties":{}})};
        let mut catalog=c::prepare(&mut f.state,f.input.bot,tool);
        c::complete(&mut f.state,catalog.bot,catalog.session,Some(catalog.policy.clone())).unwrap();
        let old=catalog.policy.clone();
        let binding=old.tools[0].clone();
        match mode {
            "permission" => { let p=f.state.permission(catalog.bot).unwrap();f.state.set_permission(token(),catalog.bot,p.revision,false).unwrap(); }
            "empty_grants" | "stale_grant" => {
                let g=f.state.set_connector_grants(token(),catalog.bot,SetConnectorGrants{connection:binding.connection.clone(),catalog_revision:binding.catalog_revision,expected_revision:binding.grant_revision,tools:if mode=="empty_grants"{vec![]}else{vec![binding.name.clone()]}}).unwrap();
                catalog.policy.tools=g.tools;
            }
            _ => { f.state.change_connection(token(),binding.connection.connection,binding.connection.revision,mode=="revoke").unwrap(); }
        }
        assert!(f.state.permitted_connector_catalog(catalog.bot,&old).is_err());
        assert!(c::complete(&mut f.state,catalog.bot,catalog.session,Some(old)).is_err());
        let inventory=f.state.connector_tools(token(),catalog.bot).unwrap();
        let prospective=f.state.current_connector_catalogs().unwrap();
        assert_eq!(inventory,prospective);
        if mode=="stale_grant" {
            assert_eq!(inventory.len(),1);
            assert_eq!(inventory,f.state.permitted_connector_catalog(catalog.bot,&catalog.policy).unwrap());
            c::complete(&mut f.state,catalog.bot,catalog.session,Some(catalog.policy.clone())).unwrap();
        } else { assert!(inventory.is_empty()); }
        let request=RequestId::new();
        let status=f.state.write_login(token(),request,CredentialWriteTarget::Create{metadata:login_metadata()},private_values(&marker)).unwrap();
        if mode=="stale_grant" {
            assert_eq!(status,CredentialWriteStatus::Rejected{request,reason:CredentialWriteRejection::PublicValueCollision});
            c::complete(&mut f.state,catalog.bot,catalog.session,Some(catalog.policy)).unwrap();
        } else { assert!(matches!(status,CredentialWriteStatus::Committed{..})); }
        c::complete(&mut f.state,catalog.bot,catalog.session,None).unwrap();
        println!("filter={mode}; stale_policy_refused=true; inventory_matches_prospective=true; current_usable_preserved=true");
    }
}

#[test]
fn fresh_catalog_revocation_preserves_unrelated_private_entry_and_chat() {
    use fresh_catalog_fixture as c;
    let mut f=fixture();
    let tool=McpTool{name:"fresh_revocation_probe".into(),description:"Independent revocation control".into(),input_schema:serde_json::json!({"type":"object","properties":{}})};
    let catalog=c::prepare(&mut f.state,f.input.bot,tool);
    c::complete(&mut f.state,catalog.bot,catalog.session,Some(catalog.policy.clone())).unwrap();
    let harmless=serde_json::json!({"message":"Hello."});
    f.state.reject_known_secrets(&harmless).unwrap();
    let initial_request=RequestId::new();
    let initial=f.state.write_login(token(),initial_request,CredentialWriteTarget::Create{metadata:login_metadata()},private_values(&RequestId::new().to_string())).unwrap();
    let CredentialWriteStatus::Committed{item,..}=initial.clone() else {panic!("safe entry before revocation failed");};
    let version=catalog.policy.tools[0].connection.clone();
    let revoked=f.state.change_connection(token(),version.connection,version.revision,true).unwrap();
    assert_eq!(revoked.availability,ConnectionAvailability::Revoked);
    assert!(f.state.current_connector_catalogs().unwrap().is_empty());
    assert!(f.state.connector_tools(token(),catalog.bot).unwrap().is_empty());
    assert!(f.state.permitted_connector_catalog(catalog.bot,&catalog.policy).is_err());
    let missing:i64=f.state.connection.query_row("SELECT count(*) FROM connection_secrets WHERE connection=?1 AND epoch=?2",params![version.connection.to_string(),revoked.version.credential_epoch as i64],|r|r.get(0)).unwrap();
    let scan=f.state.reject_known_secrets(&harmless);
    let write=f.state.write_login(token(),RequestId::new(),CredentialWriteTarget::Create{metadata:login_metadata()},private_values(&RequestId::new().to_string()));
    let rotate=f.state.write_login(token(),RequestId::new(),CredentialWriteTarget::Rotate{id:item.id,expected_revision:item.revision},private_values(&RequestId::new().to_string()));
    let replay=f.state.credential_write_status(token(),initial_request,false);
    let metadata=f.state.login_items(token());
    let plain=c::complete(&mut f.state,catalog.bot,catalog.session,None);
    println!("revoked_connection=true; usable_catalog_empty=true; revoked_epoch_ciphertexts={missing}; benign_scan={scan:?}; fresh_write={write:?}; safe_rotation={rotate:?}; metadata={metadata:?}; receipt_replay={replay:?}; plain_chat={plain:?}");
    assert!(scan.is_ok() && write.is_ok() && rotate.is_ok() && metadata.is_ok() && replay==Ok(initial) && plain.is_ok(),"revoking an optional connector must preserve unrelated safe private entry and plain chat");
}

#[test]
fn fresh_catalog_multiple_live_bots_do_not_hide_each_other() {
    use fresh_catalog_fixture as c;
    let mut f=fixture();
    let mut catalogs=vec![];
    for _ in 0..3 {
        let marker=format!("multiple-live-{}",RequestId::new());
        let tool=McpTool{name:format!("t_{}",RequestId::new()),description:marker.clone(),input_schema:serde_json::json!({"type":"object","properties":{}})};
        let catalog=c::prepare(&mut f.state,f.input.bot,tool);
        c::complete(&mut f.state,catalog.bot,catalog.session,Some(catalog.policy.clone())).unwrap();
        catalogs.push((catalog,marker));
    }
    assert_eq!(f.state.current_connector_catalogs().unwrap().len(),3);
    let disabled=catalogs[0].0.bot;
    let permission=f.state.permission(disabled).unwrap();
    f.state.set_permission(token(),disabled,permission.revision,false).unwrap();
    assert_eq!(f.state.current_connector_catalogs().unwrap().len(),2);
    for (index,(catalog,marker)) in catalogs.iter().enumerate() {
        let request=RequestId::new();
        let status=f.state.write_login(token(),request,CredentialWriteTarget::Create{metadata:login_metadata()},private_values(marker)).unwrap();
        if index==0 {assert!(matches!(status,CredentialWriteStatus::Committed{..}));}
        else {
            assert_eq!(status,CredentialWriteStatus::Rejected{request,reason:CredentialWriteRejection::PublicValueCollision});
            c::complete(&mut f.state,catalog.bot,catalog.session,Some(catalog.policy.clone())).unwrap();
        }
    }
    println!("multiple_live_bots=3; disabled=1; current_catalogs=2; both_usable_catalogs_rejected_collision=true; complete=true");
}
