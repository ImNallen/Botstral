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
        "mistral",
        "max_turns",
        "execution_policy",
        "configuration_revision",
        "max_total_tokens",
        "max_cost_microusd",
        "verified",
        "assistant",
        "earlier-private-login",
        "current request takes precedence",
        "tool_calls",
        "authorized_images",
        "image_url",
    ] {
        let mut f = fixture();
        if username == "earlier-private-login" {
            super::memory_tests::remember(
                &mut f,
                1,
                "Earlier public memory contains earlier-private-login.",
            );
        }
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
        if matches!(refused, CredentialWriteStatus::Committed { .. }) {
            assert!(
                f.state.submit_run(token(), plain_request(&f)).is_ok(),
                "accepted intake poisoned plain conversation: {username}"
            );
        }
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
        let plain = plain_request(&f);
        let run = plain.id;
        f.state.submit_run(token(), plain).unwrap();
        let permit = claim(&mut f.state);
        f.state
            .finish_run_call(
                permit.run,
                permit.turn.id,
                permit.turn.attempt,
                response(None),
            )
            .unwrap();
        let permit = claim(&mut f.state);
        f.state
            .finish_run_call(
                permit.run,
                permit.turn.id,
                permit.turn.attempt,
                response(None),
            )
            .unwrap();
        assert!(
            serde_json::to_string(&f.state.load_run(run).unwrap())
                .unwrap()
                .contains("Recorded response")
        );
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

#[test]
fn public_request_and_message_tokens_cannot_poison_a_complete_conversation() {
    fn tokens(value: &serde_json::Value, out: &mut std::collections::BTreeSet<String>) {
        match value {
            serde_json::Value::Object(fields) => {
                for (key, value) in fields {
                    out.insert(key.clone());
                    if matches!(
                        key.as_str(),
                        "type" | "role" | "state" | "tool_choice" | "service_tier"
                    ) && let Some(text) = value.as_str()
                    {
                        out.insert(text.to_owned());
                    }
                    tokens(value, out);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    tokens(value, out);
                }
            }
            _ => {}
        }
    }
    fn complete_plain(f: &mut Fixture) {
        let request = plain_request(f);
        let id = request.id;
        f.state.submit_run(token(), request).unwrap();
        let permit = claim(&mut f.state);
        assert_eq!(permit.run, id);
        f.state
            .finish_run_call(
                permit.run,
                permit.turn.id,
                permit.turn.attempt,
                response(None),
            )
            .unwrap();
        assert!(
            serde_json::to_string(&f.state.load_run(id).unwrap())
                .unwrap()
                .contains("Recorded response")
        );
    }
    let mut baseline = fixture();
    let policy = baseline
        .state
        .inference_configuration(token(), baseline.input.bot)
        .unwrap()
        .policy;
    let assistant = RunMessage::Assistant {
        content: "Recorded response".into(),
        tool_call: Some(RunToolCall {
            id: "fixture-call".into(),
            proposal: ToolProposal::Memory(MemoryProposal::List {
                scope: MemoryScopeSelection::Individual,
            }),
        }),
    };
    let prepared = crate::manager::provider::PreparedChat::new(
        &policy,
        64,
        &[
            assistant.clone(),
            RunMessage::UserMultimodal {
                content: "Fixture image context".into(),
                images: vec![],
            },
        ],
        Some(&baseline.input.execution_policy),
    );
    let mut candidates = std::collections::BTreeSet::new();
    for value in [
        serde_json::to_value(&baseline.input).unwrap(),
        serde_json::to_value(&assistant).unwrap(),
        prepared.body,
    ] {
        tokens(&value, &mut candidates);
    }
    complete_plain(&mut baseline);
    let mut checked = 0;
    for candidate in candidates
        .into_iter()
        .filter(|s| (3..=128).contains(&s.len()) && !s.chars().any(char::is_control))
    {
        let mut f = fixture();
        let status = f
            .state
            .write_login(
                token(),
                RequestId::new(),
                CredentialWriteTarget::Create {
                    metadata: login_metadata(),
                },
                private_values(&candidate),
            )
            .unwrap();
        complete_plain(&mut f);
        if let CredentialWriteStatus::Committed { item, .. } = status {
            let CredentialWriteStatus::Committed { item, .. } = f
                .state
                .write_login(
                    token(),
                    RequestId::new(),
                    CredentialWriteTarget::Rotate {
                        id: item.id,
                        expected_revision: item.revision,
                    },
                    private_values(&RequestId::new().to_string()),
                )
                .unwrap()
            else {
                panic!("safe rotation refused")
            };
            complete_plain(&mut f);
            f.state
                .update_login(token(), item.id, item.revision, None)
                .unwrap();
            complete_plain(&mut f);
        }
        checked += 1;
    }
    println!(
        "actual_request_and_message_tokens_checked={checked}; complete_conversations_usable=true"
    );
}

#[test]
fn current_connector_catalog_collision_preserves_complete_conversations() {
    use crate::manager::state::test_catalog;
    for surface in ["description", "schema", "rendered_name", "binding_name"] {
        for password in [false, true] {
            let mut f = fixture();
            let marker = format!("preexisting-account-{}", RequestId::new());
            let catalog =
                test_catalog::prepare(&mut f.state, f.input.bot, &marker, surface == "schema");
            let candidate = if surface == "rendered_name" {
                catalog.policy.tools[0].public_name()
            } else if surface == "binding_name" {
                catalog.policy.tools[0].name.clone()
            } else {
                marker
            };
            test_catalog::complete(
                &mut f.state,
                catalog.bot,
                catalog.session,
                Some(catalog.policy.clone()),
            )
            .unwrap();
            let mut login = private_values(&RequestId::new().to_string());
            if password {
                login.password = candidate.clone();
            } else {
                login.username = candidate.clone();
            }
            let request = RequestId::new();
            let expected = CredentialWriteStatus::Rejected {
                request,
                reason: CredentialWriteRejection::PublicValueCollision,
            };
            let status = f
                .state
                .write_login(
                    token(),
                    request,
                    CredentialWriteTarget::Create {
                        metadata: login_metadata(),
                    },
                    login,
                )
                .unwrap();
            if matches!(status, CredentialWriteStatus::Committed { .. }) {
                test_catalog::complete(
                    &mut f.state,
                    catalog.bot,
                    catalog.session,
                    Some(catalog.policy.clone()),
                )
                .expect("accepted credentials must preserve the existing catalog conversation");
            }
            assert_eq!(status, expected);
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
                expected
            );
            let epochs: i64 = f
                .state
                .connection
                .query_row("SELECT count(*) FROM credential_epochs", [], |r| r.get(0))
                .unwrap();
            assert_eq!(epochs, 0);
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
                panic!("safe control must commit");
            };
            let mut rotation = private_values(&RequestId::new().to_string());
            if password {
                rotation.password = candidate;
            } else {
                rotation.username = candidate;
            }
            let request = RequestId::new();
            assert_eq!(
                f.state
                    .write_login(
                        token(),
                        request,
                        CredentialWriteTarget::Rotate {
                            id: item.id,
                            expected_revision: item.revision
                        },
                        rotation
                    )
                    .unwrap(),
                CredentialWriteStatus::Rejected {
                    request,
                    reason: CredentialWriteRejection::PublicValueCollision
                }
            );
            assert_eq!(f.state.load_login(item.id).unwrap(), item);
            let epochs: i64 = f
                .state
                .connection
                .query_row("SELECT count(*) FROM credential_epochs", [], |r| r.get(0))
                .unwrap();
            assert_eq!(epochs, 1);
            test_catalog::complete(
                &mut f.state,
                catalog.bot,
                catalog.session,
                Some(catalog.policy.clone()),
            )
            .unwrap();
            f.state
                .update_login(token(), item.id, item.revision, None)
                .unwrap();
            test_catalog::complete(
                &mut f.state,
                catalog.bot,
                catalog.session,
                Some(catalog.policy),
            )
            .unwrap();
            test_catalog::complete(&mut f.state, catalog.bot, catalog.session, None).unwrap();
            println!(
                "catalog_surface={surface}; private_password={password}; final_refusal=true; rotation_unchanged=true; complete_conversations=true"
            );
        }
    }
}

#[test]
fn revoked_connector_preserves_safe_login_receipts_and_plain_conversation() {
    use crate::manager::state::test_catalog;
    let mut f = fixture();
    let catalog = test_catalog::prepare(
        &mut f.state,
        f.input.bot,
        "unrelated-catalog-account",
        false,
    );
    test_catalog::complete(
        &mut f.state,
        catalog.bot,
        catalog.session,
        Some(catalog.policy.clone()),
    )
    .unwrap();
    let request = RequestId::new();
    let initial = f
        .state
        .write_login(
            token(),
            request,
            CredentialWriteTarget::Create {
                metadata: login_metadata(),
            },
            private_values(&RequestId::new().to_string()),
        )
        .unwrap();
    let CredentialWriteStatus::Committed { item, .. } = &initial else {
        panic!("safe control must commit");
    };
    let version = &catalog.policy.tools[0].connection;
    let revoked = f
        .state
        .change_connection(token(), version.connection, version.revision, true)
        .unwrap();
    assert_eq!(revoked.availability, ConnectionAvailability::Revoked);
    assert!(f.state.current_connector_catalogs().unwrap().is_empty());
    assert!(
        f.state
            .connector_tools(token(), catalog.bot)
            .unwrap()
            .is_empty()
    );
    assert!(
        f.state
            .permitted_connector_catalog(catalog.bot, &catalog.policy)
            .is_err()
    );
    f.state
        .reject_known_secrets(&serde_json::json!({"message":"Hello."}))
        .unwrap();
    let fresh = f
        .state
        .write_login(
            token(),
            RequestId::new(),
            CredentialWriteTarget::Create {
                metadata: login_metadata(),
            },
            private_values(&RequestId::new().to_string()),
        )
        .unwrap();
    assert!(matches!(fresh, CredentialWriteStatus::Committed { .. }));
    let rotated = f
        .state
        .write_login(
            token(),
            RequestId::new(),
            CredentialWriteTarget::Rotate {
                id: item.id,
                expected_revision: item.revision,
            },
            private_values(&RequestId::new().to_string()),
        )
        .unwrap();
    assert!(matches!(rotated, CredentialWriteStatus::Committed { .. }));
    assert_eq!(
        f.state
            .credential_write_status(token(), request, false)
            .unwrap(),
        initial
    );
    assert_eq!(f.state.login_items(token()).unwrap().len(), 2);
    test_catalog::complete(&mut f.state, catalog.bot, catalog.session, None).unwrap();
}
