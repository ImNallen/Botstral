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

fn login_metadata() -> LoginMetadata {
    LoginMetadata {
        label: "Isolated credential".into(),
        bots: BotAccess::AllOwnerBots,
        allowed_origins: vec!["https://example.test".into()],
        password_policy: PasswordPolicy::EveryFill,
    }
}
fn private_values(username: &str) -> botstral_link::protected::LoginValues {
    botstral_link::protected::LoginValues {
        username: username.into(),
        password: RequestId::new().to_string(),
        totp_seed: String::new(),
    }
}
fn plain_request(f: &Fixture) -> SubmitRun {
    let mut plain = f.input.clone();
    plain.id = RunId::new();
    plain.session = None;
    plain.execution_policy.computer = None;
    plain.execution_policy.process = None;
    plain.prompt = "Hello.".into();
    plain
}

#[test]
fn p06_public_collision_intake_is_final_and_preserves_conversation() {
    for username in [
        "admin",
        "owner",
        "memory",
        "object",
        "standard_only",
        "none",
        "public_value_collision",
    ] {
        let mut f = fixture();
        let request = RequestId::new();
        let refused = f
            .state
            .write_login(
                token(),
                request,
                CredentialWriteTarget::Create {
                    metadata: login_metadata(),
                },
                private_values(username),
            )
            .unwrap();
        assert_eq!(
            refused,
            CredentialWriteStatus::Rejected {
                request,
                reason: CredentialWriteRejection::PublicValueCollision
            }
        );
        assert_eq!(
            f.state
                .credential_write_status(token(), request, true)
                .unwrap(),
            refused
        );
        assert_eq!(
            f.state
                .write_login(
                    token(),
                    request,
                    CredentialWriteTarget::Create {
                        metadata: login_metadata()
                    },
                    private_values(&RequestId::new().to_string())
                )
                .unwrap(),
            refused
        );
        let epochs: u32 = f
            .state
            .connection
            .query_row("SELECT COUNT(*) FROM credential_epochs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(epochs, 0);
        assert!(f.state.login_items(token()).unwrap().is_empty());
        f.state.submit_run(token(), f.input.clone()).unwrap();
        f.state.submit_run(token(), plain_request(&f)).unwrap();
    }
}

#[test]
fn p06_public_collision_rotation_preserves_item_and_conversation() {
    let mut f = fixture();
    let CredentialWriteStatus::Committed { item, .. } = f
        .state
        .write_login(
            token(),
            RequestId::new(),
            CredentialWriteTarget::Create {
                metadata: login_metadata(),
            },
            botstral_link::protected::LoginValues {
                username: "synthetic-native-linux-private-user".into(),
                password: "synthetic-native-linux-private-password".into(),
                totp_seed: "JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP".into(),
            },
        )
        .unwrap()
    else {
        panic!("expected initial commit")
    };
    let request = RequestId::new();
    let mut values = private_values(&RequestId::new().to_string());
    values.password = "admin".into();
    assert_eq!(
        f.state
            .write_login(
                token(),
                request,
                CredentialWriteTarget::Rotate {
                    id: item.id,
                    expected_revision: item.revision
                },
                values
            )
            .unwrap(),
        CredentialWriteStatus::Rejected {
            request,
            reason: CredentialWriteRejection::PublicValueCollision
        }
    );
    assert_eq!(f.state.load_login(item.id).unwrap(), item);
    let epochs: u32 = f
        .state
        .connection
        .query_row("SELECT COUNT(*) FROM credential_epochs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(epochs, 1);
    f.state.submit_run(token(), f.input.clone()).unwrap();
    f.state
        .update_login(token(), item.id, item.revision, None)
        .unwrap();
    f.state.submit_run(token(), plain_request(&f)).unwrap();
}

#[test]
fn p06_explicit_pre_entry_restore_preserves_a_separate_affected_snapshot() {
    let mut f = fixture();
    let snapshot = f._root.path().join("before-entry.sqlite3");
    f.state
        .connection
        .execute("VACUUM INTO ?1", [snapshot.to_str().unwrap()])
        .unwrap();
    let CredentialWriteStatus::Committed { item, .. } = f
        .state
        .write_login(
            token(),
            RequestId::new(),
            CredentialWriteTarget::Create {
                metadata: login_metadata(),
            },
            private_values(&RequestId::new().to_string()),
        )
        .unwrap()
    else {
        panic!("expected initial commit")
    };
    let payload = zeroize::Zeroizing::new(serde_json::to_vec(&private_values("admin")).unwrap());
    let Installation::Configured {
        vault: VaultState::Unlocked { identity },
    } = &f.state.installation
    else {
        panic!("expected unlocked fixture")
    };
    let context = format!(
        "{}\0{}\0{}\0login",
        f.state.manager_identity().unwrap(),
        item.id,
        item.epoch.get()
    );
    let ciphertext = crypto::encrypt_credential(identity, &context, &payload).unwrap();
    f.state
        .connection
        .execute_batch("DROP TRIGGER immutable_credential_epoch")
        .unwrap();
    f.state
        .connection
        .execute(
            "UPDATE credential_epochs SET ciphertext=?1 WHERE item=?2",
            params![ciphertext, item.id.to_string()],
        )
        .unwrap();
    f.state.connection.execute_batch("CREATE TRIGGER immutable_credential_epoch BEFORE UPDATE ON credential_epochs BEGIN SELECT RAISE(ABORT,'immutable credential epoch'); END;").unwrap();
    assert_eq!(
        f.state.submit_run(token(), plain_request(&f)),
        Err(OperationError::InvalidInput)
    );
    f.state
        .update_login(token(), item.id, item.revision, None)
        .unwrap();
    assert_eq!(
        f.state.submit_run(token(), plain_request(&f)),
        Err(OperationError::InvalidInput)
    );
    let affected = f._root.path().join("affected-store.sqlite3");
    f.state
        .connection
        .execute("VACUUM INTO ?1", [affected.to_str().unwrap()])
        .unwrap();
    f.state.connection = Connection::open(&snapshot).unwrap();
    migrate(&mut f.state.connection).unwrap();
    f.state.submit_run(token(), plain_request(&f)).unwrap();
    assert!(f.state.login_items(token()).unwrap().is_empty());
    let preserved = Connection::open(affected).unwrap();
    let retained: Vec<u8> = preserved
        .query_row(
            "SELECT ciphertext FROM credential_epochs WHERE item=?1",
            [item.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(retained, ciphertext);
}
