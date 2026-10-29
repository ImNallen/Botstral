use super::*;
use crate::worker_http::VerifiedIdentity;
use botstral_link::protected::*;

struct Live {
    _root: tempfile::TempDir,
    state: State,
    authority: BrowserAuthority,
    destination: LiveDestination,
    item: LoginItemView,
    _outbound: mpsc::Receiver<Envelope<ManagerMessage>>,
}
fn owner() -> SessionToken {
    inference::tests::token()
}
fn peer() -> VerifiedIdentity {
    VerifiedIdentity::for_test(b"P06 controlled worker")
}
fn live_values() -> LoginValues {
    let mut login = values(1);
    login.password = login.password.replace('\n', "");
    login
}
fn plan(origin: &str) -> LoginSitePlan {
    LoginSitePlan {
        version: 1,
        origin: origin.into(),
        route: "/login".into(),
        frame: None,
        form: "#login".into(),
        username: "#username".into(),
        password: "#password".into(),
        totp: "#totp".into(),
        submitter: "#submit".into(),
        action: "/session".into(),
    }
}
fn live(site: LoginSitePlan, purpose: CredentialFields, policy: PasswordPolicy, now: i64) -> Live {
    let (root, mut state, input) = inference::tests::fixture();
    state
        .connection
        .execute("UPDATE sessions SET expires_at=?1", [now + 3600])
        .unwrap();
    state.test_time = Some(now);
    let mut metadata = metadata();
    metadata.allowed_origins = vec![site.origin.clone()];
    metadata.password_policy = policy;
    let item = committed(
        state
            .write_login(
                owner(),
                RequestId::new(),
                CredentialWriteTarget::Create { metadata },
                live_values(),
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
    Live {
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
    }
}
impl Live {
    fn call(
        &mut self,
        operation: ProtectedBrowserOperation,
    ) -> Result<ProtectedBrowserReply, OperationError> {
        self.state.protected_browser(
            peer(),
            ProtectedBrowserRequest {
                authority: BrowserAuthority {
                    task: self.authority.task,
                    execution: self.authority.execution,
                    fence: self.authority.fence,
                    journal: self.authority.journal,
                },
                operation,
            },
        )
    }
    fn prepare(&mut self, previous: Option<CredentialUseId>) -> CredentialUseReview {
        match self
            .call(ProtectedBrowserOperation::Prepare {
                destination: self.destination.clone(),
                previous,
            })
            .unwrap()
        {
            ProtectedBrowserReply::Review { review } => *review,
            _ => panic!("expected review"),
        }
    }
    fn release(
        &mut self,
        review: &CredentialUseReview,
    ) -> Result<ProtectedBrowserReply, OperationError> {
        self.call(ProtectedBrowserOperation::Release {
            id: review.id,
            destination: self.destination.clone(),
            protection: ProfileProtection::Tracked {
                profile: self.destination.profile,
                releases: vec![review.release],
                complete: true,
            },
        })
    }
    fn allow_rule(&mut self) {
        let review = self.prepare(None);
        self.state
            .connection
            .execute(
                "INSERT INTO credential_authorities VALUES(?1,?2)",
                params![review.id.to_string(), r#"{"kind":"owner"}"#],
            )
            .unwrap();
        self.state
            .connection
            .execute(
                "INSERT INTO credential_rules VALUES(?1,?2,?3,'allow',?4)",
                params![
                    self.item.id.to_string(),
                    self.state
                        .load_task(self.authority.task)
                        .unwrap()
                        .command
                        .bot
                        .to_string(),
                    uses::contract(&self.destination).unwrap(),
                    review.id.to_string()
                ],
            )
            .unwrap();
    }
}

#[test]
fn p06_exact_authority_releases_once_and_refuses_changed_documents() {
    let mut f = live(
        plan("https://example.test"),
        CredentialFields::Password,
        PasswordPolicy::EveryFill,
        1000,
    );
    let review = f.prepare(None);
    assert!(f.release(&review).is_err());
    f.state
        .decide_credential_use(owner(), review.clone(), UseDecision::AllowOnce)
        .unwrap();
    let document = f.destination.frame_document;
    f.destination.frame_document = BrowserDocumentId::new();
    assert!(f.release(&review).is_err());
    f.destination.frame_document = document;
    assert!(matches!(
        f.release(&review).unwrap(),
        ProtectedBrowserReply::Released {
            values: FillValues::Password { .. },
            ..
        }
    ));
    assert!(f.release(&review).is_err());
    f.call(ProtectedBrowserOperation::Settle {
        id: review.id,
        applied: true,
    })
    .unwrap();
    f.call(ProtectedBrowserOperation::Settle {
        id: review.id,
        applied: true,
    })
    .unwrap();
    assert!(
        f.state
            .decide_credential_use(owner(), review, UseDecision::AllowOnce)
            .is_err()
    );
}

#[test]
fn p06_remembered_rule_precedence_and_totp_fresh_approval() {
    for (purpose, policy, certain, automatic) in [
        (
            CredentialFields::Password,
            PasswordPolicy::RememberedOrigins,
            true,
            true,
        ),
        (
            CredentialFields::Password,
            PasswordPolicy::EveryFill,
            true,
            false,
        ),
        (
            CredentialFields::Password,
            PasswordPolicy::RememberedOrigins,
            false,
            false,
        ),
        (
            CredentialFields::Totp,
            PasswordPolicy::RememberedOrigins,
            true,
            false,
        ),
    ] {
        let mut f = live(plan("https://example.test"), purpose, policy, 1000);
        f.destination.certain = certain;
        f.allow_rule();
        let review = f.prepare(None);
        assert_eq!(review.state == UseState::Authorized, automatic);
        if !automatic {
            f.state
                .decide_credential_use(owner(), review.clone(), UseDecision::AllowOnce)
                .unwrap();
        }
        let reply = f.release(&review).unwrap();
        if let ProtectedBrowserReply::Released {
            values: FillValues::Totp { code },
            valid_until,
            ..
        } = reply
        {
            assert_eq!(code.len(), 6);
            assert_eq!(valid_until, 1020);
            assert!(f.state.reject_known_secrets(&*code).is_err());
            assert_eq!(f.prepare(None).state, UseState::AwaitingApproval);
        }
    }
    let mut f = live(
        plan("https://example.test"),
        CredentialFields::Password,
        PasswordPolicy::RememberedOrigins,
        1000,
    );
    let review = f.prepare(None);
    f.state
        .decide_credential_use(owner(), review.clone(), UseDecision::RememberPassword)
        .unwrap();
    assert!(f.release(&review).is_err());
    let fresh = f.prepare(Some(review.id));
    assert_ne!(fresh.id, review.id);
    assert_eq!(fresh.state, UseState::Authorized);
    f.release(&fresh).unwrap();
}

#[test]
fn p06_revocation_rotation_lock_expiry_and_fences_precede_release() {
    for mutation in [
        "rotate",
        "remove",
        "lock",
        "deny",
        "expiry",
        "permission",
        "generation",
        "journal",
        "takeover",
        "restart",
    ] {
        let mut f = live(
            plan("https://example.test"),
            CredentialFields::Password,
            PasswordPolicy::EveryFill,
            1000,
        );
        let review = f.prepare(None);
        if mutation == "deny" {
            f.state
                .decide_credential_use(owner(), review.clone(), UseDecision::Deny)
                .unwrap();
        } else {
            f.state
                .decide_credential_use(owner(), review.clone(), UseDecision::AllowOnce)
                .unwrap();
        }
        match mutation {
            "rotate" => {
                f.state
                    .write_login(
                        owner(),
                        RequestId::new(),
                        CredentialWriteTarget::Rotate {
                            id: f.item.id,
                            expected_revision: f.item.revision,
                        },
                        values(2),
                    )
                    .unwrap();
            }
            "remove" => {
                f.state
                    .update_login(owner(), f.item.id, f.item.revision, None)
                    .unwrap();
            }
            "lock" => f.state.lock_vault(owner()).unwrap(),
            "expiry" => f.state.test_time = Some(1031),
            "permission" => {
                let bot = review.binding.bot;
                f.state
                    .set_permission(owner(), bot, review.binding.permission_revision, false)
                    .unwrap();
            }
            "generation" => {
                f.authority.fence.generation = f.authority.fence.generation.advance().unwrap()
            }
            "journal" => f.authority.journal = JournalId::new(),
            "takeover" => {
                f.state
                    .set_input_owner(
                        owner(),
                        f.authority.fence.session_id,
                        f.authority.fence.revision,
                        InputOwner::Owner,
                    )
                    .unwrap();
            }
            "restart" => f.state.recover_credential_uses().unwrap(),
            _ => {}
        }
        assert!(
            f.release(&review).is_err(),
            "stale release accepted for {mutation}"
        );
    }
}

#[test]
fn p06_profile_coverage_refuses_rollback_and_retains_removed_values() {
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
    let covered = ProfileProtection::Tracked {
        profile: f.destination.profile,
        releases: vec![review.release],
        complete: true,
    };
    assert!(matches!(
        f.call(ProtectedBrowserOperation::Screen {
            protection: covered.clone(),
            text: Zeroizing::new("Safe account status".into())
        })
        .unwrap(),
        ProtectedBrowserReply::Screened { .. }
    ));
    assert!(
        f.call(ProtectedBrowserOperation::Screen {
            protection: covered,
            text: Zeroizing::new(live_values().password.clone())
        })
        .is_err()
    );
    for protection in [
        ProfileProtection::Unknown,
        ProfileProtection::Tracked {
            profile: f.destination.profile,
            releases: vec![ReleaseId::new()],
            complete: true,
        },
        ProfileProtection::Tracked {
            profile: f.destination.profile,
            releases: vec![],
            complete: false,
        },
    ] {
        assert!(
            f.call(ProtectedBrowserOperation::Screen {
                protection,
                text: Zeroizing::new("No current secret matches this observation".into())
            })
            .is_err()
        );
    }
    f.state
        .update_login(owner(), f.item.id, f.item.revision, None)
        .unwrap();
    assert!(
        f.state
            .reject_known_secrets(&live_values().password)
            .is_err()
    );
    assert_eq!(
        f.state.load_task(f.authority.task).unwrap().state,
        TaskState::Cancelling
    );
}

#[test]
fn p06_code_coverage_refuses_a_restore_before_possible_totp_release() {
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
    let protection = ProfileProtection::Tracked {
        profile: f.destination.profile,
        releases: vec![review.release],
        complete: true,
    };
    assert!(
        f.call(ProtectedBrowserOperation::Screen {
            protection,
            text: Zeroizing::new("A later code has no retained issuance in this snapshot".into())
        })
        .is_err()
    );
}

#[test]
fn p06_lost_release_becomes_uncertain_after_parent_authority_expires() {
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
    f.state.test_time = Some(1031);
    let reviews = f.state.credential_uses(owner()).unwrap();
    assert_eq!(reviews[0].state, UseState::Uncertain);
    assert!(f.release(&review).is_err());
}

#[test]
fn p06_metadata_edits_clear_rules_and_bot_inventory_never_includes_removed_items() {
    let mut f = live(
        plan("https://example.test"),
        CredentialFields::Password,
        PasswordPolicy::RememberedOrigins,
        1000,
    );
    let bot = f.state.load_task(f.authority.task).unwrap().command.bot;
    assert_eq!(f.state.accessible_login_items(bot).unwrap().len(), 1);
    f.allow_rule();
    let review = f.prepare(None);
    assert_eq!(review.state, UseState::Authorized);
    f.state
        .update_login(
            owner(),
            f.item.id,
            f.item.revision,
            Some(f.item.metadata.clone()),
        )
        .unwrap();
    assert!(f.release(&review).is_err());
    assert_eq!(
        f.state
            .connection
            .query_row("SELECT COUNT(*) FROM credential_rules", [], |r| r
                .get::<_, u32>(0))
            .unwrap(),
        0
    );
    let item = f.state.load_login(f.item.id).unwrap();
    f.state
        .update_login(owner(), item.id, item.revision, None)
        .unwrap();
    assert!(f.state.accessible_login_items(bot).unwrap().is_empty());
}

mod browser;

mod authority;
