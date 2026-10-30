use super::*;

fn unrelated_login(f: &mut Live) -> LoginValues {
    let bot = f
        .state
        .create_bot(
            owner(),
            NewBot {
                id: BotId::new(),
                name: BotName::new("Other scoped account".into()).unwrap(),
            },
        )
        .unwrap();
    let mut public = metadata();
    public.label = "Other private account".into();
    public.bots = BotAccess::Selected { bots: vec![bot.id] };
    public.allowed_origins = vec!["https://unrelated.example.test".into()];
    f.state
        .write_login(
            owner(),
            RequestId::new(),
            CredentialWriteTarget::Create { metadata: public },
            values(91),
        )
        .unwrap();
    values(91)
}
fn screen(
    f: &mut Live,
    releases: Vec<ReleaseId>,
    text: &str,
) -> Result<ProtectedBrowserReply, OperationError> {
    f.call(ProtectedBrowserOperation::Screen {
        protection: ProfileProtection::Tracked {
            profile: f.destination.profile,
            releases,
            complete: true,
        },
        text: Zeroizing::new(text.into()),
    })
}

#[test]
fn pr56_security_unknown_profile_cannot_probe_global_inventory() {
    let mut f = live(
        plan("https://example.test"),
        CredentialFields::Password,
        PasswordPolicy::EveryFill,
        1000,
    );
    let unrelated = unrelated_login(&mut f);
    let benign = screen(&mut f, vec![], "Public account status");
    let guessed = screen(&mut f, vec![], &unrelated.password);
    println!(
        "unrecorded_profile_benign_accepted={}; unrelated_guess_accepted={}",
        benign.is_ok(),
        guessed.is_ok()
    );
    assert!(
        benign.is_err() && guessed.is_err(),
        "unrecorded caller-selected profiles cannot query private inventory"
    );
}

#[test]
fn pr56_security_recorded_profile_screens_only_emitted_values() {
    let mut f = live(
        plan("https://example.test"),
        CredentialFields::Password,
        PasswordPolicy::EveryFill,
        1000,
    );
    let unrelated = unrelated_login(&mut f);
    let review = f.prepare(None);
    f.state
        .decide_credential_use(owner(), review.clone(), UseDecision::AllowOnce)
        .unwrap();
    f.release(&review).unwrap();
    let local = live_values();
    let classification = [
        &local.username,
        &local.password,
        &unrelated.password,
        &local.totp_seed,
        "synthetic-state-inference-secret",
    ]
    .map(|text| screen(&mut f, vec![review.release], text).is_ok());
    println!("recorded_profile_acceptance={classification:?}");
    assert_eq!(classification, [false, false, true, true, true]);
    assert!(screen(&mut f, vec![], "Public account status").is_err());
    assert!(
        screen(
            &mut f,
            vec![review.release, review.release],
            "Public account status"
        )
        .is_err()
    );
    assert!(screen(&mut f, vec![ReleaseId::new()], "Public account status").is_err());
    f.state
        .update_login(owner(), f.item.id, f.item.revision, None)
        .unwrap();
    f.next_parent(true);
    assert!(screen(&mut f, vec![review.release], &local.password).is_err());
    assert!(screen(&mut f, vec![review.release], "Public account status").is_ok());
}

#[test]
fn pr56_security_prepare_retries_do_not_allocate_or_extend_expiry() {
    let mut f = live(
        plan("https://example.test"),
        CredentialFields::Password,
        PasswordPolicy::EveryFill,
        1000,
    );
    let first = f.prepare(None);
    f.state.test_time = Some(1001);
    let retries: Vec<_> = (0..5).map(|_| f.prepare(None)).collect();
    let count: i64 = f
        .state
        .connection
        .query_row("SELECT count(*) FROM credential_uses", [], |r| r.get(0))
        .unwrap();
    println!(
        "prepare_retry_records={count}; same_identity={}",
        retries.iter().all(|r| r.id == first.id
            && r.release == first.release
            && r.expires_at == first.expires_at)
    );
    assert_eq!(count, 1);
    assert!(retries.iter().all(|r| r.id == first.id
        && r.release == first.release
        && r.expires_at == first.expires_at));
    f.state.test_time = Some(first.expires_at + 1);
    let expired = f.prepare(None);
    assert_eq!(expired.id, first.id);
    assert_eq!(expired.state, UseState::Expired);
}

#[test]
fn pr56_security_changed_destination_and_consumed_prepare_cannot_mint_uses() {
    let mut f = live(
        plan("https://example.test"),
        CredentialFields::Password,
        PasswordPolicy::EveryFill,
        1000,
    );
    let first = f.prepare(None);
    let original = f.destination.clone();
    let changed: Vec<_> = (0..5)
        .map(|_| {
            f.destination.form = RequestId::new().to_string();
            f.call(ProtectedBrowserOperation::Prepare {
                destination: f.destination.clone(),
                previous: None,
            })
            .is_ok()
        })
        .collect();
    println!("altered_destination_prepares_accepted={changed:?}");
    assert!(changed.iter().all(|accepted| !accepted));
    f.destination = original;
    f.state
        .decide_credential_use(owner(), first.clone(), UseDecision::AllowOnce)
        .unwrap();
    f.release(&first).unwrap();
    let replay = f.prepare(None);
    assert_eq!(replay.id, first.id);
    assert_eq!(replay.state, UseState::Released);
    assert!(f.release(&replay).is_err());
    let count: i64 = f
        .state
        .connection
        .query_row("SELECT count(*) FROM credential_uses", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn pr56_security_prepare_retry_survives_full_global_capacity() {
    let mut f = live(
        plan("https://example.test"),
        CredentialFields::Password,
        PasswordPolicy::EveryFill,
        1000,
    );
    let first = f.prepare(None);
    let tx = f.state.connection.transaction().unwrap();
    for _ in 1..MAX_CREDENTIAL_USES {
        let mut filler = first.clone();
        filler.id = CredentialUseId::new();
        filler.release = ReleaseId::new();
        filler.binding.task = TaskId::new();
        filler.binding.execution = ExecutionId::new();
        filler.destination.profile = ProfileEpoch::new();
        filler.state = UseState::Expired;
        tx.execute(
            "INSERT INTO credential_uses VALUES(?1,?2,?3)",
            params![
                filler.id.to_string(),
                filler.release.to_string(),
                serde_json::to_string(&filler).unwrap()
            ],
        )
        .unwrap();
    }
    tx.commit().unwrap();
    let result = f.call(ProtectedBrowserOperation::Prepare {
        destination: f.destination.clone(),
        previous: None,
    });
    println!("cached_prepare_at_capacity_returned={}", result.is_ok());
    let ProtectedBrowserReply::Review { review } = result.unwrap() else {
        panic!("cached review expected")
    };
    assert_eq!(review.id, first.id);
    assert_eq!(review.release, first.release);
}

#[test]
fn pr56_security_rejected_prepare_attempt_cannot_probe_again() {
    let mut f = live(
        plan("https://example.test"),
        CredentialFields::Password,
        PasswordPolicy::EveryFill,
        1000,
    );
    let unrelated = unrelated_login(&mut f);
    f.destination.fields[0].name = unrelated.username.clone();
    assert!(matches!(
        f.call(ProtectedBrowserOperation::Prepare {
            destination: f.destination.clone(),
            previous: None
        }),
        Err(OperationError::InvalidInput)
    ));
    let count: i64 = f
        .state
        .connection
        .query_row(
            "SELECT count(*) FROM credential_prepare_attempts WHERE task=?1",
            [f.authority.task.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    f.state
        .connection
        .execute_batch("ALTER TABLE connection_versions RENAME TO unavailable_versions")
        .unwrap();
    f.destination.fields[0].name = "ordinary_login_name".into();
    let retry = f.call(ProtectedBrowserOperation::Prepare {
        destination: f.destination.clone(),
        previous: None,
    });
    println!(
        "rejected_prepare_attempts={count}; changed_guess_error={:?}",
        retry.as_ref().err()
    );
    assert!(matches!(retry, Err(OperationError::Conflict)));
    assert_eq!(count, 1);
    f.state
        .connection
        .execute_batch("ALTER TABLE unavailable_versions RENAME TO connection_versions")
        .unwrap();
    let uses: i64 = f
        .state
        .connection
        .query_row("SELECT count(*) FROM credential_uses", [], |r| r.get(0))
        .unwrap();
    assert_eq!(uses, 0);
    let snapshot = f
        ._root
        .path()
        .canonicalize()
        .unwrap()
        .join("rejected-attempt.sqlite3");
    f.state
        .connection
        .execute("VACUUM INTO ?1", [snapshot.to_str().unwrap()])
        .unwrap();
    let restored = Connection::open(snapshot).unwrap();
    let execution: String = restored
        .query_row(
            "SELECT execution FROM credential_prepare_attempts WHERE task=?1",
            [f.authority.task.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(execution, f.authority.execution.to_string());
    println!(
        "rejected_attempt_persisted=true; changed_guess_not_rescanned=true; reviews_allocated=0"
    );
}

#[test]
fn pr56_security_unemitted_profile_does_not_screen_any_private_value() {
    let mut f = live(
        plan("https://example.test"),
        CredentialFields::Password,
        PasswordPolicy::EveryFill,
        1000,
    );
    let review = f.prepare(None);
    assert!(screen(&mut f, vec![], &live_values().password).is_ok());
    assert!(screen(&mut f, vec![], &live_values().totp_seed).is_ok());
    assert!(screen(&mut f, vec![review.release], "Public account status").is_err());
}

#[test]
fn pr56_security_profile_requires_all_emissions_and_correct_worker() {
    let mut f = live(
        plan("https://example.test"),
        CredentialFields::Password,
        PasswordPolicy::EveryFill,
        1000,
    );
    let first = f.prepare(None);
    f.state
        .decide_credential_use(owner(), first.clone(), UseDecision::AllowOnce)
        .unwrap();
    f.release(&first).unwrap();
    let first_task = f.state.load_task(f.authority.task).unwrap();
    f.next_parent(false);
    let second = f.prepare(None);
    f.state
        .decide_credential_use(owner(), second.clone(), UseDecision::AllowOnce)
        .unwrap();
    f.release(&second).unwrap();
    assert!(screen(&mut f, vec![second.release], "Public status").is_err());
    assert!(screen(&mut f, vec![first.release, second.release], "Public status").is_ok());
    let other = crate::worker_http::VerifiedIdentity::for_test(b"foreign profile scope fixture");
    let invitation = f
        .state
        .invite_worker(owner(), other.fingerprint().clone(), 600)
        .unwrap();
    let foreign = f.state.enroll_worker(other, invitation.secret).unwrap();
    let current_first = f.state.load_task(first_task.id).unwrap();
    f.state
        .connection
        .execute(
            "UPDATE tasks SET worker=?1 WHERE id=?2",
            params![foreign.to_string(), first_task.id.to_string()],
        )
        .unwrap();
    assert!(screen(&mut f, vec![first.release, second.release], "Public status").is_err());
    let mut foreign_first = current_first.clone();
    foreign_first.command.worker = foreign;
    tasks::save(&f.state.connection, &foreign_first).unwrap();
    assert!(matches!(
        screen(&mut f, vec![first.release, second.release], "Public status"),
        Err(OperationError::Unauthorized)
    ));
    f.state
        .connection
        .execute(
            "UPDATE tasks SET worker=?1 WHERE id=?2",
            params![
                current_first.command.worker.to_string(),
                current_first.id.to_string()
            ],
        )
        .unwrap();
    tasks::save(&f.state.connection, &current_first).unwrap();
    assert!(screen(&mut f, vec![first.release, second.release], "Public status").is_ok());
}

#[test]
fn pr56_security_totp_profile_uses_only_issued_code_and_retains_integrity() {
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
    let ProtectedBrowserReply::Released {
        values: FillValues::Totp { code },
        ..
    } = f.release(&review).unwrap()
    else {
        panic!("code expected")
    };
    assert!(screen(&mut f, vec![review.release], &code).is_err());
    let values = live_values();
    for text in [&values.username, &values.password, &values.totp_seed] {
        assert!(screen(&mut f, vec![review.release], text).is_ok());
    }
    let ciphertext: Vec<u8> = f
        .state
        .connection
        .query_row(
            "SELECT ciphertext FROM credential_codes WHERE release=?1",
            [review.release.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    f.state
        .connection
        .execute_batch("DROP TRIGGER retained_credential_code")
        .unwrap();
    f.state
        .connection
        .execute(
            "DELETE FROM credential_codes WHERE release=?1",
            [review.release.to_string()],
        )
        .unwrap();
    assert!(screen(&mut f, vec![review.release], "Public status").is_err());
    f.state
        .connection
        .execute(
            "INSERT INTO credential_codes VALUES(?1,?2)",
            params![review.release.to_string(), ciphertext],
        )
        .unwrap();
    f.state.connection.execute_batch("CREATE TRIGGER retained_credential_code BEFORE DELETE ON credential_codes BEGIN SELECT RAISE(ABORT,'retained issued code'); END;").unwrap();
    assert!(screen(&mut f, vec![review.release], "Public status").is_ok());
}

#[test]
fn pr56_security_remembered_successor_is_unique_and_keeps_root_deadline() {
    let mut f = live(
        plan("https://example.test"),
        CredentialFields::Password,
        PasswordPolicy::RememberedOrigins,
        1000,
    );
    let root = f.prepare(None);
    f.state.test_time = Some(1005);
    f.state
        .decide_credential_use(owner(), root.clone(), UseDecision::RememberPassword)
        .unwrap();
    let successor = f.prepare(Some(root.id));
    assert_eq!(successor.expires_at, root.expires_at);
    assert_eq!(successor.state, UseState::Authorized);
    assert_eq!(f.prepare(Some(root.id)), successor);
    assert_eq!(f.prepare(None).id, root.id);
    assert!(
        f.call(ProtectedBrowserOperation::Prepare {
            destination: f.destination.clone(),
            previous: Some(successor.id)
        })
        .is_err()
    );
    f.release(&successor).unwrap();
    assert_eq!(f.prepare(Some(root.id)).state, UseState::Released);
    let count: i64 = f
        .state
        .connection
        .query_row("SELECT count(*) FROM credential_uses", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 2);
}
