use super::*;

#[test]
fn review_probe_numeric_secret_publication() {
    let mut f = fixture();
    let number = 100000 + (RequestId::new().0.as_u128() % 800000) as i64;
    let secret = number.to_string();
    f.state
        .write_login(
            token(),
            RequestId::new(),
            CredentialWriteTarget::Create {
                metadata: LoginMetadata {
                    label: "Numeric private value review".into(),
                    bots: BotAccess::AllOwnerBots,
                    allowed_origins: vec!["https://example.test".into()],
                    password_policy: PasswordPolicy::EveryFill,
                },
            },
            botstral_link::protected::LoginValues {
                username: RequestId::new().to_string(),
                password: secret.clone(),
                totp_seed: String::new(),
            },
        )
        .unwrap();
    assert!(f.state.reject_known_secrets(&secret).is_err());
    f.state.submit_run(token(), f.input.clone()).unwrap();
    let permit = claim(&mut f.state);
    f.state
        .finish_run_call(
            permit.run,
            permit.turn.id,
            permit.turn.attempt,
            response(Some(RunToolCall {
                id: "numeric-publication-review".into(),
                proposal: ToolProposal::RequestWakeup(RequestWakeup {
                    at: number,
                    prompt: "Synthetic scheduling probe".into(),
                }),
            })),
        )
        .unwrap();
    let stored = f.state.load_run(permit.run).unwrap();
    let durable = serde_json::to_string(&stored).unwrap().contains(&secret);
    let public = f
        .state
        .run(token(), permit.run)
        .is_ok_and(|view| serde_json::to_string(&view).unwrap().contains(&secret));
    println!(
        "raw_string_secret_refused=true; numeric_secret_in_durable_run={durable}; numeric_secret_in_public_run={public}"
    );
    assert!(
        !durable && !public,
        "raw digits of retained secret persisted and published as JSON number"
    );
}

#[test]
fn p06_private_identifiers_colliding_with_public_scaffolding_fail_closed() {
    for username in ["admin", "owner"] {
        let mut f = fixture();
        let written = f.state.write_login(
            token(),
            RequestId::new(),
            CredentialWriteTarget::Create {
                metadata: LoginMetadata {
                    label: "Private identifier probe".into(),
                    bots: BotAccess::AllOwnerBots,
                    allowed_origins: vec!["https://example.test".into()],
                    password_policy: PasswordPolicy::EveryFill,
                },
            },
            botstral_link::protected::LoginValues {
                username: username.into(),
                password: RequestId::new().to_string(),
                totp_seed: String::new(),
            },
        );
        if written.is_err() {
            assert_eq!(written, Err(OperationError::InvalidInput));
            continue;
        }
        assert_eq!(
            f.state.submit_run(token(), f.input.clone()),
            Err(OperationError::InvalidInput)
        );
        let persisted: u32 = f
            .state
            .connection
            .query_row("SELECT COUNT(*) FROM runs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(persisted, 0);
    }
}
