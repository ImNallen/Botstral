#[test]
fn recheck_fixed_public_surface_matrix() {
    let mut failures = Vec::new();
    for candidate in ["admin", "owner", "standard_only", "none", "public_value_collision", "parallel_tool_calls", "request_wakeup", "max_turns", "execution_policy", "configuration_revision", "max_total_tokens", "max_cost_microusd", "assistant", "global", "verified", "mistral-small-latest"] {
        let mut f = fixture();
        let baseline = plain_request(&f);
        f.state.submit_run(token(), baseline).unwrap();
        let request = RequestId::new();
        let status = f.state.write_login(token(), request, CredentialWriteTarget::Create { metadata: login_metadata() }, private_values(candidate)).unwrap();
        let accepted = matches!(status, CredentialWriteStatus::Committed {..});
        let admitted = f.state.submit_run(token(), plain_request(&f)).is_ok();
        println!("candidate={candidate}; accepted={accepted}; plain_admitted={admitted}; receipt={status:?}");
        if accepted && !admitted { failures.push(candidate); }
        if !accepted {
            assert_eq!(f.state.credential_write_status(token(), request, true).unwrap(), status);
            assert_eq!(f.state.write_login(token(), request, CredentialWriteTarget::Create { metadata: login_metadata() }, private_values(&RequestId::new().to_string())).unwrap(), status);
        }
    }
    assert!(failures.is_empty(), "accepted values poison unchanged benign conversation: {failures:?}");
}

#[test]
fn recheck_accepted_fixed_key_survives_rotation_and_removal() {
    let mut f = fixture();
    f.state.submit_run(token(), plain_request(&f)).unwrap();
    let CredentialWriteStatus::Committed { item, .. } = f.state.write_login(token(), RequestId::new(), CredentialWriteTarget::Create { metadata: login_metadata() }, private_values("max_turns")).unwrap() else { panic!("candidate refused before commit"); };
    assert!(f.state.reject_known_secrets(&plain_request(&f)).is_err());
    let after_create = f.state.submit_run(token(), plain_request(&f));
    let CredentialWriteStatus::Committed { item: rotated, .. } = f.state.write_login(token(), RequestId::new(), CredentialWriteTarget::Rotate { id:item.id, expected_revision:item.revision }, private_values(&RequestId::new().to_string())).unwrap() else { panic!("rotation did not commit"); };
    let after_rotation = f.state.submit_run(token(), plain_request(&f));
    f.state.update_login(token(), rotated.id, rotated.revision, None).unwrap();
    assert!(f.state.accessible_login_items(f.input.bot).unwrap().is_empty());
    let after_removal = f.state.submit_run(token(), plain_request(&f));
    println!("accepted max_turns; real rotation committed; removal committed; active inventory empty; create={:?}; rotation={:?}; removal={:?}", after_create.as_ref().err(), after_rotation.as_ref().err(), after_removal.as_ref().err());
    assert!(after_removal.is_ok(), "fixed mandatory request key still poisons plain chat after successful rotation and removal");
}

#[test]
fn recheck_plain_assistant_completion_after_accepted_value() {
    let mut baseline = fixture();
    let input = plain_request(&baseline);
    let id = input.id;
    baseline.state.submit_run(token(), input).unwrap();
    let permit=claim(&mut baseline.state);
    baseline.state.finish_run_call(permit.run,permit.turn.id,permit.turn.attempt,response(None)).unwrap();
    assert!(serde_json::to_string(&baseline.state.load_run(id).unwrap()).unwrap().contains("Recorded response"));
    let mut f = fixture();
    let CredentialWriteStatus::Committed {..} = f.state.write_login(token(), RequestId::new(), CredentialWriteTarget::Create { metadata: login_metadata() }, private_values("assistant")).unwrap() else { panic!("expected committed candidate"); };
    let input=plain_request(&f);
    let id=input.id;
    f.state.submit_run(token(), input).unwrap();
    let permit=claim(&mut f.state);
    f.state.finish_run_call(permit.run,permit.turn.id,permit.turn.attempt,response(None)).unwrap();
    let stored=f.state.load_run(id).unwrap();
    println!("accepted assistant; plain completion phase={:?}; turns={:?}",stored.summary.phase,stored.turns);
    assert!(serde_json::to_string(&stored).unwrap().contains("Recorded response"), "accepted fixed assistant discriminator prevents benign completion");
}
