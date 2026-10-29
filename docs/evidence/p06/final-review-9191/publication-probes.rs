#[test]
fn final_review_identifier_admission_poison_survives_rotation_and_removal() {
    let mut baseline = fixture();
    assert!(baseline.state.submit_run(token(), baseline.input.clone()).is_ok());
    let mut plain_baseline = baseline.input.clone();
    plain_baseline.id = RunId::new();
    plain_baseline.session = None;
    plain_baseline.execution_policy.computer = None;
    plain_baseline.execution_policy.process = None;
    plain_baseline.prompt = "Hello.".into();
    assert!(baseline.state.submit_run(token(), plain_baseline).is_ok());
    let mut f = fixture();
    let status = f.state.write_login(token(), RequestId::new(), CredentialWriteTarget::Create {
        metadata: LoginMetadata { label: "Isolated credential".into(), bots: BotAccess::AllOwnerBots,
            allowed_origins: vec!["https://example.test".into()], password_policy: PasswordPolicy::EveryFill }
    }, botstral_link::protected::LoginValues { username: "admin".into(), password: RequestId::new().to_string(), totp_seed: String::new() }).unwrap();
    let CredentialWriteStatus::Committed { item, .. } = status else { panic!("expected real commit") };
    let after_create = f.state.submit_run(token(), f.input.clone());
    assert_eq!(after_create, Err(OperationError::InvalidInput));
    let rotated = f.state.write_login(token(), RequestId::new(), CredentialWriteTarget::Rotate { id:item.id, expected_revision:item.revision },
        botstral_link::protected::LoginValues { username: RequestId::new().to_string(), password:RequestId::new().to_string(), totp_seed:String::new() }).unwrap();
    let CredentialWriteStatus::Committed { item, .. } = rotated else { panic!("expected rotation") };
    let after_rotation = f.state.submit_run(token(), f.input.clone());
    let removed = f.state.update_login(token(), item.id, item.revision, None).unwrap();
    assert_eq!(removed.state, CredentialState::Removed);
    assert!(f.state.accessible_login_items(f.input.bot).unwrap().is_empty());
    let after_remove = f.state.submit_run(token(), f.input.clone());
    let mut plain = f.input.clone();
    plain.id = RunId::new();
    plain.session = None;
    plain.execution_policy.computer = None;
    plain.execution_policy.process = None;
    plain.prompt = "Hello.".into();
    let plain_after_remove = f.state.submit_run(token(), plain);
    println!("baseline_admitted=true; plain_chat_baseline_admitted=true; login_committed=true; create_refused={}; rotation_refused={}; removed_inventory_empty=true; removal_refused={}; plain_chat_refused={}", after_create.is_err(), after_rotation.is_err(), after_remove.is_err(), plain_after_remove.is_err());
    assert!(after_remove.is_ok() && plain_after_remove.is_ok(), "accepted private identifier permanently prevents benign conversations even after credential rotation and removal");
}

#[test]
fn final_review_composed_encoding_matrix() {
    use crate::manager::provider::reflects_secret;
    use base64::{Engine, engine::general_purpose::*};
    let secret = "fresh / probe+é\"\\";
    let percent = secret.as_bytes().iter().enumerate().map(|(i,b)| if i%2==0 {format!("%{b:02X}")} else {format!("%{b:02x}")}).collect::<String>();
    let nested = percent.replace('%', "%25");
    let escaped = secret.chars().map(|c| format!("\\u{:04x}", c as u32)).collect::<String>();
    let matrix = vec![percent, nested, format!("\"{escaped}\""), STANDARD.encode(secret), STANDARD_NO_PAD.encode(secret), URL_SAFE.encode(secret), URL_SAFE_NO_PAD.encode(secret), serde_json::to_string(&serde_json::json!({"message":secret})).unwrap()];
    for (index, value) in matrix.into_iter().enumerate() {
        assert!(reflects_secret(&serde_json::json!({"nested": [value.clone()]}), secret), "case {index}");
        assert!(reflects_secret(&serde_json::json!({value: "benign"}), secret), "key case {index}");
    }
    for (value, scalar) in [(serde_json::json!(true), "true"), (serde_json::Value::Null, "null"), (serde_json::json!(293847), "293847")] {
        assert!(reflects_secret(&value, scalar));
    }
    assert!(!reflects_secret(&serde_json::json!({"safe": "Welcome, signed in."}), secret));
    println!("eight_composed_encodings_in_values_and_keys_refused=true; three_json_scalars_refused=true; benign_control_passes=true");
}
