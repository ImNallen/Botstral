use super::*;

fn review_device(f: &mut Live) -> crate::manager::devices::DeviceLease {
    let credential = crypto::random_token();
    let invitation = f
        .state
        .invite_device(
            owner(),
            PairingRequest {
                fingerprint: DeviceFingerprint::from_credential(&credential),
                label: "Independent synthetic owner".into(),
            },
            60,
        )
        .unwrap();
    f.state
        .connect_owner(
            OwnerHello::Enroll {
                version: OWNER_VERSION,
                manager: f.state.manager_identity().unwrap(),
                role: DeviceRole::Owner,
                credential,
                invitation: invitation.secret.expose_secret().into(),
            },
            watch::channel(false).0,
        )
        .unwrap()
        .1
}

#[test]
fn review_probe_device_revocation_before_release() {
    let mut f = live(
        plan("https://example.test"),
        CredentialFields::Password,
        PasswordPolicy::EveryFill,
        1000,
    );
    let lease = review_device(&mut f);
    let review = f.prepare(None);
    f.state
        .decide_credential_use(lease, review.clone(), UseDecision::AllowOnce)
        .unwrap();
    f.state.revoke_device(owner(), lease.device).unwrap();
    assert!(
        f.state
            .authorize_owner(&OwnerAuthority::Device(lease))
            .is_err()
    );
    let released = matches!(
        f.release(&review),
        Ok(ProtectedBrowserReply::Released { .. })
    );
    println!("revoked_device_rejected=true; previously_approved_release_returned={released}");
    assert!(
        !released,
        "credential bytes released after approving device was revoked"
    );
}

#[test]
fn review_probe_device_revocation_after_release() {
    let mut f = live(
        plan("https://example.test"),
        CredentialFields::Password,
        PasswordPolicy::EveryFill,
        1000,
    );
    let lease = review_device(&mut f);
    let review = f.prepare(None);
    f.state
        .decide_credential_use(lease, review.clone(), UseDecision::AllowOnce)
        .unwrap();
    f.release(&review).unwrap();
    f.state.revoke_device(owner(), lease.device).unwrap();
    let history = f.state.credential_uses(owner()).unwrap();
    let task = f.state.load_task(f.authority.task).unwrap();
    let cancellations: u32 = f
        .state
        .connection
        .query_row(
            "SELECT COUNT(*) FROM cancellations WHERE task=?1",
            [task.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    println!(
        "after_device_revoke use={:?}; task={:?}; durable_cancellations={cancellations}",
        history[0].state, task.state
    );
    assert_eq!(
        history[0].state,
        UseState::Uncertain,
        "released use must become uncertain after device revocation"
    );
    assert_eq!(task.state, TaskState::Cancelling);
    assert_eq!(cancellations, 1);
}

#[test]
fn review_probe_lowercase_percent_screening() {
    let mut f = live(
        plan("https://example.test"),
        CredentialFields::Password,
        PasswordPolicy::EveryFill,
        1000,
    );
    let review = f.prepare(None);
    f.state
        .decide_credential_use(owner(), review.clone(), UseDecision::AllowOnce)
        .unwrap();
    f.release(&review).unwrap();
    let secret = live_values().password.clone();
    let encode = |upper: bool| {
        secret
            .bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                    (b as char).to_string()
                } else if upper {
                    format!("%{b:02X}")
                } else {
                    format!("%{b:02x}")
                }
            })
            .collect::<String>()
    };
    let protection = ProfileProtection::Tracked {
        profile: f.destination.profile,
        releases: vec![review.release],
        complete: true,
    };
    let control = f.call(ProtectedBrowserOperation::Screen {
        protection: protection.clone(),
        text: Zeroizing::new(encode(true)),
    });
    assert!(control.is_err(), "uppercase control must refuse");
    let encoded = encode(false);
    let response = f.call(ProtectedBrowserOperation::Screen {
        protection,
        text: Zeroizing::new(encoded.clone()),
    });
    let screened = matches!(response, Ok(ProtectedBrowserReply::Screened { .. }));
    let mut metadata = f.item.metadata.clone();
    metadata.label = encoded.clone();
    let published = f
        .state
        .update_login(owner(), f.item.id, f.item.revision, Some(metadata))
        .is_ok();
    let bot = f.state.load_task(f.authority.task).unwrap().command.bot;
    let visible = f
        .state
        .accessible_login_items(bot)
        .unwrap()
        .iter()
        .any(|i| i.metadata.label == encoded);
    println!(
        "uppercase_percent_refused=true; lowercase_percent_screened={screened}; encoded_metadata_persisted={published}; bot_visible={visible}"
    );
    assert!(
        !screened && !published && !visible,
        "ordinary lowercase percent encoding bypassed retained-secret publication checks"
    );
}

#[test]
fn review_probe_form_urlencoded_screening() {
    let mut f = live(
        plan("https://example.test"),
        CredentialFields::Password,
        PasswordPolicy::EveryFill,
        1000,
    );
    let secret = format!("owned review {} /", RequestId::new());
    let rotated = committed(
        f.state
            .write_login(
                owner(),
                RequestId::new(),
                CredentialWriteTarget::Rotate {
                    id: f.item.id,
                    expected_revision: f.item.revision,
                },
                LoginValues {
                    username: live_values().username.clone(),
                    password: secret.clone(),
                    totp_seed: String::new(),
                },
            )
            .unwrap(),
    );
    let mut url = reqwest::Url::parse("https://example.test").unwrap();
    url.query_pairs_mut().append_pair("value", &secret);
    let encoded = url
        .query()
        .unwrap()
        .strip_prefix("value=")
        .unwrap()
        .to_owned();
    assert!(encoded.contains('+'));
    assert!(f.state.reject_known_secrets(&secret).is_err());
    let mut metadata = rotated.metadata.clone();
    metadata.label = encoded.clone();
    let published = f
        .state
        .update_login(owner(), rotated.id, rotated.revision, Some(metadata))
        .is_ok();
    let bot = f.state.load_task(f.authority.task).unwrap().command.bot;
    let visible = f
        .state
        .accessible_login_items(bot)
        .unwrap()
        .iter()
        .any(|i| i.metadata.label == encoded);
    println!(
        "raw_refused=true; form_urlencoded_metadata_persisted={published}; bot_visible={visible}"
    );
    assert!(
        !published && !visible,
        "standard URL form encoding bypassed retained-secret publication checks"
    );
}

#[test]
fn review_probe_release_transaction_rollback() {
    let mut f = live(
        plan("https://example.test"),
        CredentialFields::Totp,
        PasswordPolicy::EveryFill,
        1000,
    );
    let review = f.prepare(None);
    f.state
        .decide_credential_use(owner(), review.clone(), UseDecision::AllowOnce)
        .unwrap();
    f.state.connection.execute_batch("CREATE TEMP TRIGGER review_fail_release BEFORE UPDATE ON credential_uses BEGIN SELECT RAISE(ABORT, 'synthetic release failure'); END;").unwrap();
    assert!(f.release(&review).is_err());
    let count: u32 = f
        .state
        .connection
        .query_row("SELECT COUNT(*) FROM credential_codes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(
        f.state.credential_uses(owner()).unwrap()[0].state,
        UseState::Authorized
    );
    f.state
        .connection
        .execute_batch("DROP TRIGGER review_fail_release")
        .unwrap();
    f.release(&review).unwrap();
    assert!(f.release(&review).is_err());
    println!("failed_release_rolled_back_code_and_consumption=true; retry_released_once=true");
}

#[test]
fn review_probe_totp_validity_boundary() {
    for (time, accepted) in [(1015, true), (1016, false)] {
        let mut f = live(
            plan("https://example.test"),
            CredentialFields::Totp,
            PasswordPolicy::EveryFill,
            time,
        );
        let review = f.prepare(None);
        f.state
            .decide_credential_use(owner(), review.clone(), UseDecision::AllowOnce)
            .unwrap();
        assert_eq!(f.release(&review).is_ok(), accepted);
        let codes: u32 = f
            .state
            .connection
            .query_row("SELECT COUNT(*) FROM credential_codes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(codes, u32::from(accepted));
    }
    println!("five_seconds_remaining_accepted=true; four_seconds_remaining_refused=true");
}

#[test]
fn review_probe_cancel_and_peer_fences() {
    for mutation in [
        "cancel",
        "peer",
        "worker_revoke",
        "execution",
        "task",
        "origin",
        "profile",
        "form",
        "field",
        "submitter",
        "item_scope",
    ] {
        let mut f = live(
            plan("https://example.test"),
            CredentialFields::Password,
            PasswordPolicy::EveryFill,
            1000,
        );
        let review = f.prepare(None);
        f.state
            .decide_credential_use(owner(), review.clone(), UseDecision::AllowOnce)
            .unwrap();
        match mutation {
            "cancel" => {
                f.state.cancel_task(owner(), f.authority.task).unwrap();
            }
            "peer" => {
                assert!(
                    f.state
                        .protected_browser(
                            VerifiedIdentity::for_test(b"different owned synthetic worker"),
                            ProtectedBrowserRequest {
                                authority: f.authority,
                                operation: ProtectedBrowserOperation::Poll { id: review.id }
                            }
                        )
                        .is_err()
                );
                continue;
            }
            "worker_revoke" => {
                f.state
                    .revoke_worker(
                        owner(),
                        f.state.load_task(f.authority.task).unwrap().command.worker,
                    )
                    .unwrap();
            }
            "execution" => f.authority.execution = ExecutionId::new(),
            "task" => f.authority.task = TaskId::new(),
            "origin" => f.destination.top_origin = "https://different.example.test".into(),
            "profile" => f.destination.profile = ProfileEpoch::new(),
            "form" => f.destination.form = RequestId::new().to_string(),
            "field" => f.destination.fields[0].object = RequestId::new(),
            "submitter" => f.destination.submitter = RequestId::new().to_string(),
            "item_scope" => {
                let mut metadata = f.item.metadata.clone();
                metadata.allowed_origins = vec!["https://different.example.test".into()];
                f.state
                    .update_login(owner(), f.item.id, f.item.revision, Some(metadata))
                    .unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            f.release(&review).is_err(),
            "changed {mutation} admitted release"
        );
        println!("independent_{mutation}_fence=refused");
    }
}

#[test]
fn review_probe_closed_rotation_stays_closed() {
    let mut f = fixture();
    let item = committed(
        f.state
            .write_login(
                f.lease,
                RequestId::new(),
                CredentialWriteTarget::Create {
                    metadata: metadata(),
                },
                values(1),
            )
            .unwrap(),
    );
    let request = RequestId::new();
    let closed = f
        .state
        .credential_write_status(f.lease, request, true)
        .unwrap();
    let result = f
        .state
        .write_login(
            f.lease,
            request,
            CredentialWriteTarget::Rotate {
                id: item.id,
                expected_revision: item.revision,
            },
            values(2),
        )
        .unwrap();
    assert_eq!(result, closed);
    assert_eq!(f.state.login_items(f.lease).unwrap(), vec![item]);
    let epochs: u32 = f
        .state
        .connection
        .query_row("SELECT COUNT(*) FROM credential_epochs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(epochs, 1);
    println!("late_rotation_after_closure_refused=true; original_epoch_preserved=true");
}

#[test]
fn review_probe_cross_installation_and_kind_binding() {
    let mut f = fixture();
    let item = committed(
        f.state
            .write_login(
                f.lease,
                RequestId::new(),
                CredentialWriteTarget::Create {
                    metadata: metadata(),
                },
                values(1),
            )
            .unwrap(),
    );
    let ciphertext: Vec<u8> = f
        .state
        .connection
        .query_row("SELECT ciphertext FROM credential_epochs", [], |r| r.get(0))
        .unwrap();
    let identity = f.state.credential_identity().unwrap();
    let context = f.state.credential_context(item.id, item.epoch).unwrap();
    let changed_installation = format!(
        "{}\0{}\0{}\0login",
        ManagerId::new(),
        item.id,
        item.epoch.get()
    );
    let changed_kind = context.replace("\0login", "\0issued-code");
    for changed in [changed_installation, changed_kind] {
        assert!(crypto::decrypt_credential(identity, &changed, &ciphertext).is_err());
    }
    assert!(crypto::decrypt_credential(identity, &context, &ciphertext).is_ok());
    println!(
        "cross_installation_decryption_refused=true; cross_kind_decryption_refused=true; original_binding_decrypts=true"
    );
}

#[test]
fn remembered_device_grant_revocation_requires_new_approval() {
    let mut f = live(
        plan("https://example.test"),
        CredentialFields::Password,
        PasswordPolicy::RememberedOrigins,
        1000,
    );
    let lease = review_device(&mut f);
    let review = f.prepare(None);
    f.state
        .decide_credential_use(lease, review.clone(), UseDecision::RememberPassword)
        .unwrap();
    let automatic = f.prepare(Some(review.id));
    assert_eq!(automatic.state, UseState::Authorized);
    f.state.revoke_device(owner(), lease.device).unwrap();
    assert!(f.release(&automatic).is_err());
    assert_eq!(f.prepare(Some(review.id)).state, UseState::AwaitingApproval);
}

#[test]
fn unrelated_revocation_and_disconnect_preserve_approval() {
    for disconnect in [false, true] {
        let mut f = live(
            plan("https://example.test"),
            CredentialFields::Password,
            PasswordPolicy::EveryFill,
            1000,
        );
        let source = review_device(&mut f);
        let unrelated = review_device(&mut f);
        let review = f.prepare(None);
        f.state
            .decide_credential_use(source, review.clone(), UseDecision::AllowOnce)
            .unwrap();
        if disconnect {
            f.state.disconnect_owner(source);
        } else {
            f.state.revoke_device(owner(), unrelated.device).unwrap();
        }
        f.release(&review).unwrap();
    }
}

#[test]
fn remembered_rules_without_provenance_require_approval() {
    let mut f = live(
        plan("https://example.test"),
        CredentialFields::Password,
        PasswordPolicy::RememberedOrigins,
        1000,
    );
    f.allow_rule();
    f.state
        .connection
        .execute("DELETE FROM credential_authorities", [])
        .unwrap();
    assert_eq!(f.prepare(None).state, UseState::AwaitingApproval);
}

#[test]
fn totp_collision_cannot_poison_existing_public_configuration() {
    let mut login = live_values();
    login.totp_seed = "AAAAAAAAAAAAAAAAAAAAAAAAAAAASBSQ".into();
    let mut f = live_with_values(
        plan("https://example.test"),
        CredentialFields::Totp,
        PasswordPolicy::EveryFill,
        1000,
        login,
    );
    let bot = f.state.load_task(f.authority.task).unwrap().command.bot;
    let mut policy = f
        .state
        .inference_configuration(owner(), bot)
        .unwrap()
        .policy;
    policy.model_id = "mistral-test-429496".into();
    f.state
        .configure_inference(
            owner(),
            ConfigureInference {
                bot,
                expected_revision: 1,
                policy,
            },
        )
        .unwrap();
    assert!(
        f.state
            .reject_known_secrets(&f.state.inference_configurations().unwrap())
            .is_ok()
    );
    let review = f.prepare(None);
    f.state
        .decide_credential_use(owner(), review.clone(), UseDecision::AllowOnce)
        .unwrap();
    let released = f.release(&review).is_ok();
    let retained: u32 = f
        .state
        .connection
        .query_row("SELECT COUNT(*) FROM credential_codes", [], |r| r.get(0))
        .unwrap();
    let configuration_readable = f
        .state
        .reject_known_secrets(&f.state.inference_configurations().unwrap())
        .is_ok();
    println!(
        "generated_code_released={released}; retained_codes={retained}; public_configuration_readable={configuration_readable}"
    );
    assert!(
        !released && retained == 0 && configuration_readable,
        "generated code permanently poisoned the existing public configuration"
    );
}

#[test]
fn generated_code_current_catalog_collision_preserves_complete_conversations() {
    use crate::manager::state::test_catalog;
    for schema_marker in [false, true] {
        let mut login = live_values();
        login.totp_seed = "JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP".into();
        let (code, _) = totp::current(&login.totp_seed, 1000).unwrap();
        let (mut f, catalog) = live_with_setup(
            plan("https://example.test"),
            CredentialFields::Totp,
            PasswordPolicy::EveryFill,
            1000,
            login,
            |state, bot| test_catalog::prepare(state, bot, &code, schema_marker),
        );
        test_catalog::complete(
            &mut f.state,
            catalog.bot,
            catalog.session,
            Some(catalog.policy.clone()),
        )
        .unwrap();
        let review = f.prepare(None);
        f.state
            .decide_credential_use(owner(), review.clone(), UseDecision::AllowOnce)
            .unwrap();
        assert!(matches!(f.release(&review), Err(OperationError::Conflict)));
        assert!(matches!(f.release(&review), Err(OperationError::Conflict)));
        let retained: i64 = f
            .state
            .connection
            .query_row("SELECT count(*) FROM credential_codes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(retained, 0);
        assert_eq!(
            f.state.credential_uses(owner()).unwrap()[0].state,
            UseState::Authorized
        );
        test_catalog::complete(
            &mut f.state,
            catalog.bot,
            catalog.session,
            Some(catalog.policy.clone()),
        )
        .unwrap();
        f.state
            .update_login(owner(), f.item.id, f.item.revision, None)
            .unwrap();
        f.state.test_time = Some(1030);
        test_catalog::complete(
            &mut f.state,
            catalog.bot,
            catalog.session,
            Some(catalog.policy),
        )
        .unwrap();
        test_catalog::complete(&mut f.state, catalog.bot, catalog.session, None).unwrap();
        println!(
            "generated_catalog_schema={schema_marker}; release_refused=true; retained_codes=0; use_authorized=true; complete_after_expiry_and_removal=true"
        );
    }
}

#[test]
fn retained_review_public_metadata_cannot_be_poisoned_by_private_intake() {
    let marker = format!("review-account-{}", RequestId::new());
    let mut site = plan("https://example.test");
    site.route = format!("/login/{marker}");
    let mut f = live(
        site,
        CredentialFields::Password,
        PasswordPolicy::EveryFill,
        1000,
    );
    let review = f.prepare(None);
    f.state
        .decide_credential_use(owner(), review.clone(), UseDecision::Deny)
        .unwrap();
    for removed in [false, true] {
        if removed {
            let current = f.state.load_login(f.item.id).unwrap();
            f.state
                .update_login(owner(), current.id, current.revision, None)
                .unwrap();
        }
        let mut candidate = values(62);
        candidate.username = marker.clone();
        let request = RequestId::new();
        let status = f
            .state
            .write_login(
                owner(),
                request,
                CredentialWriteTarget::Create {
                    metadata: metadata(),
                },
                candidate,
            )
            .unwrap();
        let reviews = f.state.credential_uses(owner());
        println!(
            "retained_review_removed_item={removed}; intake={status:?}; history_readable={}",
            reviews.is_ok()
        );
        assert_eq!(
            status,
            CredentialWriteStatus::Rejected {
                request,
                reason: CredentialWriteRejection::PublicValueCollision
            }
        );
        assert_eq!(reviews.unwrap().len(), 1);
        assert_eq!(f.state.login_items(owner()).unwrap().len(), 1);
    }
}

#[test]
fn generated_code_cannot_poison_existing_login_public_records() {
    for receipt_only in [false, true] {
        let mut login = live_values();
        login.totp_seed = "AAAAAAAAAAAAAAAAAAAAAAAAAAAASBSQ".into();
        let (code, _) = totp::current(&login.totp_seed, 1000).unwrap();
        let (mut f, prior_request) = live_with_setup(
            plan("https://example.test"),
            CredentialFields::Totp,
            PasswordPolicy::EveryFill,
            1000,
            login,
            |state, _| {
                let mut public = metadata();
                public.label = format!("Existing public account {}", code.as_str());
                let request = RequestId::new();
                let item = committed(
                    state
                        .write_login(
                            owner(),
                            request,
                            CredentialWriteTarget::Create {
                                metadata: public.clone(),
                            },
                            values(63),
                        )
                        .unwrap(),
                );
                if receipt_only {
                    public.label = "Renamed public account".into();
                    state
                        .update_login(owner(), item.id, item.revision, Some(public))
                        .unwrap();
                }
                request
            },
        );
        let bot = f.state.load_task(f.authority.task).unwrap().command.bot;
        assert_eq!(f.state.accessible_login_items(bot).unwrap().len(), 2);
        let review = f.prepare(None);
        f.state
            .decide_credential_use(owner(), review.clone(), UseDecision::AllowOnce)
            .unwrap();
        let released = f.release(&review);
        let retained: i64 = f
            .state
            .connection
            .query_row("SELECT count(*) FROM credential_codes", [], |r| r.get(0))
            .unwrap();
        let listing = f.state.login_items(owner());
        let receipt = f
            .state
            .credential_write_status(owner(), prior_request, false);
        println!(
            "generated_public_receipt_only={receipt_only}; released={}; retained_codes={retained}; metadata_readable={}; receipt_readable={}",
            released.is_ok(),
            listing.is_ok(),
            receipt.is_ok()
        );
        assert!(matches!(released, Err(OperationError::Conflict)));
        assert_eq!(retained, 0);
        assert_eq!(listing.unwrap().len(), 2);
        assert!(matches!(
            receipt.unwrap(),
            CredentialWriteStatus::Committed { .. }
        ));
        assert_eq!(f.state.accessible_login_items(bot).unwrap().len(), 2);
    }
}
