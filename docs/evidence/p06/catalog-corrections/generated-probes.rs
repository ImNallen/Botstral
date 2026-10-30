include!("common.rs");
#[test]
fn independent_generated_code_current_catalog_preserves_conversation() {
    let mut login=live_values(); login.totp_seed="JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP".into();
    let (code,_)=crate::manager::state::credentials::totp::current(&login.totp_seed,1000).unwrap();
    let (mut f,policy,session,bot,_received)=independent_live_with_catalog(plan("https://example.test"),CredentialFields::Totp,PasswordPolicy::EveryFill,1000,login,&code);
    review_conversation(&mut f.state,bot,session,Some(policy.clone())).unwrap();
    let review=f.prepare(None);
    f.state.decide_credential_use(owner(),review.clone(),UseDecision::AllowOnce).unwrap();
    let released=f.release(&review).is_ok();
    let retained:i64=f.state.connection.query_row("SELECT count(*) FROM credential_codes",[],|r|r.get(0)).unwrap();
    let after=review_conversation(&mut f.state,bot,session,Some(policy.clone()));
    f.state.update_login(owner(),f.item.id,f.item.revision,None).unwrap();
    f.state.test_time=Some(1030);
    let after_expiry_removal=review_conversation(&mut f.state,bot,session,Some(policy));
    let plain=review_conversation(&mut f.state,bot,session,None);
    println!("catalog_predates_code=true; baseline_complete=true; code_released={released}; retained_codes={retained}; after={after:?}; after_expiry_removal={after_expiry_removal:?}; plain={plain:?}");
    assert!(!released && retained==0 && after.is_ok(),"generated code poisoned an already enabled catalog");
}

fn independent_live_with_catalog(
    site: LoginSitePlan,
    purpose: CredentialFields,
    policy: PasswordPolicy,
    now: i64,
    login: LoginValues,
    marker: &str,
) -> (Live, ConnectorToolPolicy, ComputerSessionId, BotId, mpsc::Receiver<Envelope<ManagerMessage>>) {
    let (root, mut state, input) = inference::tests::fixture();
    state
        .connection
        .execute("UPDATE sessions SET expires_at=?1", [now + 3600])
        .unwrap();
    state.test_time = Some(now);
    let (catalog, catalog_session, catalog_bot, catalog_receive)=review_catalog(&mut state,input.bot,marker,false);
    let mut metadata = metadata();
    metadata.allowed_origins = vec![site.origin.clone()];
    metadata.password_policy = policy;
    let item = committed(
        state
            .write_login(
                owner(),
                RequestId::new(),
                CredentialWriteTarget::Create { metadata },
                login,
            )
            .unwrap(),
    );
    let invitation = state
        .invite_worker(owner(), peer().fingerprint().clone(), 600)
        .unwrap();
    let worker = state.enroll_worker(peer(), invitation.secret).unwrap();
    let session = state
        .open_computer_session(owner(), input.bot, worker)
        .unwrap();
    let (outbound, mut receive) = mpsc::channel(32);
    let mut hello = Hello::current(false);
    hello
        .capabilities
        .extend([Capability::ProcessSystemdV1, Capability::ComputerV1]);
    let lease = state
        .connect_worker(
            peer(),
            hello,
            outbound,
            watch::channel(false).0,
            RequestId::new(),
        )
        .unwrap();
    state
        .worker_report(
            lease,
            Envelope::new(WorkerMessage::SnapshotApplied {
                generation: lease.generation,
            }),
        )
        .unwrap();
    let journal = JournalId::new();
    state.recovery_hello(lease, journal, 0).unwrap();
    state
        .reconcile_inventory(lease, journal, 0, 0, vec![])
        .unwrap();
    state.set_permission(owner(), input.bot, 1, true).unwrap();
    let task = state
        .submit_task(
            owner(),
            SubmitTask {
                id: TaskId::new(),
                session_id: session.id,
                conversation_id: None,
                settings: TaskSettings {
                    model: "controlled login".into(),
                    max_turns: 1,
                    max_tokens: 1,
                    deadline: now + 120,
                    max_cost_micros: 0,
                },
                work: WorkSpec::Computer {
                    spec: Box::new(ComputerSpec {
                        profile: ComputerProfile::LinuxPlaywrightV1,
                        account: ProcessAccount {
                            uid: 1000,
                            gid: 1000,
                        },
                        runtime: ComputerRuntime {
                            sha256: CertificateFingerprint::from_der(b"controlled runtime"),
                        },
                        deadline: now + 120,
                        limits: ProcessLimits {
                            runtime_seconds: 120,
                            output_bytes: 1024,
                            processes: 32,
                            memory_bytes: 256 * 1024 * 1024,
                        },
                        viewport: Viewport {
                            width: 800,
                            height: 600,
                        },
                        operations: vec![ComputerOperation::ProtectedLogin {
                            item: item.id,
                            revision: item.revision,
                            purpose,
                            plan: Box::new(site.clone()),
                        }],
                        outputs: vec![],
                    }),
                },
            },
        )
        .unwrap();
    while receive.try_recv().is_ok() {}
    let approved = state.grant_approval(owner(), task.id, now + 90).unwrap();
    let fence = state
        .find_computer_session(session.id)
        .unwrap()
        .fence(lease.generation);
    state
        .claim(lease, journal, fence, approved.approval.binding.unwrap())
        .unwrap();
    let mut permit = None;
    while let Ok(message) = receive.try_recv() {
        if matches!(message.message, ManagerMessage::Permit { .. }) {
            permit = Some(message.message);
        }
    }
    state
        .admit_delivery(lease, &permit.expect("admitted parent execution"))
        .unwrap();
    (Live {
        _root: root,
        state,
        item,
        _outbound: receive,
        authority: BrowserAuthority {
            task: task.id,
            execution: task.command.execution,
            fence,
            journal,
        },
        destination: LiveDestination {
            profile: ProfileEpoch::new(),
            browser: BrowserInstanceId::new(),
            page: BrowserPageId::new(),
            top_document: BrowserDocumentId::new(),
            frame: BrowserFrameId::new(),
            frame_document: BrowserDocumentId::new(),
            top_origin: site.origin.clone(),
            frame_origin: site.origin.clone(),
            plan: site,
            form: RequestId::new().to_string(),
            fields: (0..if purpose == CredentialFields::Password {
                2
            } else {
                1
            })
                .map(|i| LoginField {
                    object: RequestId::new(),
                    role: if purpose == CredentialFields::Totp {
                        LoginFieldRole::Totp
                    } else if i == 0 {
                        LoginFieldRole::Username
                    } else {
                        LoginFieldRole::Password
                    },
                    name: if purpose == CredentialFields::Totp {
                        "totp"
                    } else if i == 0 {
                        "username"
                    } else {
                        "password"
                    }
                    .into(),
                    input_type: if purpose == CredentialFields::Password && i == 1 {
                        "password"
                    } else {
                        "text"
                    }
                    .into(),
                })
                .collect(),
            submitter: RequestId::new().to_string(),
            certain: true,
        },
    },catalog,catalog_session,catalog_bot,catalog_receive)
}#[test]
fn independent_fresh_generated_model_collision_refuses_without_consumption() {
    let mut login=live_values(); login.totp_seed="JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP".into();
    let (code,_)=crate::manager::state::credentials::totp::current(&login.totp_seed,1000).unwrap();
    let mut f=live_with_values(plan("https://example.test"),CredentialFields::Totp,PasswordPolicy::EveryFill,1000,login);
    let bot=f.state.load_task(f.authority.task).unwrap().command.bot;
    let mut policy=f.state.inference_configuration(owner(),bot).unwrap().policy;
    policy.model_id=format!("fresh-existing-model-{}",code.as_str());
    f.state.configure_inference(owner(),ConfigureInference{bot,expected_revision:1,policy}).unwrap();
    let verification=f.state.prepare_inference_verification(owner(),VerifyInference{bot,revision:2}).unwrap();
    f.state.finish_inference_verification(verification.configuration,verification.epoch,Some((true,false))).unwrap();
    let session=f.state.load_task(f.authority.task).unwrap().command.session;
    review_conversation(&mut f.state,bot,session,None).unwrap();
    let review=f.prepare(None); f.state.decide_credential_use(owner(),review.clone(),UseDecision::AllowOnce).unwrap();
    assert!(matches!(f.release(&review),Err(OperationError::Conflict)));
    assert!(matches!(f.release(&review),Err(OperationError::Conflict)));
    let retained:i64=f.state.connection.query_row("SELECT count(*) FROM credential_codes",[],|r|r.get(0)).unwrap(); assert_eq!(retained,0);
    let saved:String=f.state.connection.query_row("SELECT view FROM credential_uses WHERE id=?1",[review.id.to_string()],|r|r.get(0)).unwrap();
    let saved:CredentialUseReview=serde_json::from_str(&saved).unwrap();assert_eq!(saved.state,UseState::Authorized);
    review_conversation(&mut f.state,bot,session,None).unwrap();
    println!("fresh_model_generated_collision=true; release_refused=true; retained_codes=0; use_still_authorized=true; baseline_and_after_complete=true");
}
