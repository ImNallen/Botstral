use super::*;

fn review_screen(f: &mut Live, releases: Vec<ReleaseId>, text: &str) -> Result<ProtectedBrowserReply, OperationError> {
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
fn independent_review_retained_epoch_after_rotation_and_corruption() {
    let mut f = live(plan("https://example.test"), CredentialFields::Password, PasswordPolicy::EveryFill, 1000);
    let review = f.prepare(None);
    f.state.decide_credential_use(owner(), review.clone(), UseDecision::AllowOnce).unwrap();
    f.release(&review).unwrap();
    f.call(ProtectedBrowserOperation::Settle { id: review.id, applied: true }).unwrap();
    let newer = values(92);
    f.state.write_login(owner(), RequestId::new(), CredentialWriteTarget::Rotate {
        id: f.item.id, expected_revision: f.item.revision,
    }, values(92)).unwrap();
    f.next_parent(true);
    assert!(review_screen(&mut f, vec![review.release], &live_values().password).is_err());
    assert!(review_screen(&mut f, vec![review.release], &newer.password).is_ok());
    assert!(review_screen(&mut f, vec![review.release], "Historical account status").is_ok());
    f.state.connection.execute_batch("DROP TRIGGER immutable_credential_epoch").unwrap();
    f.state.connection.execute("UPDATE credential_epochs SET ciphertext=x'00' WHERE item=?1 AND epoch=?2", params![review.item.id.to_string(), review.item.epoch.get() as i64]).unwrap();
    assert!(review_screen(&mut f, vec![review.release], "Historical account status").is_err());
    println!("old_applied_epoch_screened=true; newer_unemitted_password_accepted=true; corrupt_recorded_epoch_refused=true");
}

#[test]
fn independent_review_rejected_attempt_is_durable_and_closed_after_reopen() {
    let mut f = live(plan("https://example.test"), CredentialFields::Password, PasswordPolicy::EveryFill, 1000);
    f.destination.fields[0].name = live_values().username.clone();
    let rejected = f.call(ProtectedBrowserOperation::Prepare { destination: f.destination.clone(), previous: None });
    assert!(matches!(rejected, Err(OperationError::InvalidInput)));
    let snapshot = f._root.path().canonicalize().unwrap().join("independent-attempt.sqlite3");
    f.state.connection.execute("VACUUM INTO ?1", [snapshot.to_str().unwrap()]).unwrap();
    f.state.connection = Connection::open(&snapshot).unwrap();
    let saved: (String, i64) = f.state.connection.query_row("SELECT execution,(SELECT user_version FROM pragma_user_version) FROM credential_prepare_attempts WHERE task=?1", [f.authority.task.to_string()], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(saved, (f.authority.execution.to_string(), 16));
    assert!(f.state.connection.execute("DELETE FROM credential_prepare_attempts", []).is_err());
    assert!(f.state.connection.execute("UPDATE credential_prepare_attempts SET execution='changed'", []).is_err());
    f.state.connection.execute_batch("ALTER TABLE connection_versions RENAME TO unavailable_versions").unwrap();
    f.destination.fields[0].name = "ordinary_public_name".into();
    let retry = f.call(ProtectedBrowserOperation::Prepare { destination: f.destination.clone(), previous: None });
    assert!(matches!(retry, Err(OperationError::Conflict)));
    println!("reopened_rejected_attempt_closed=true; attempt_update_and_delete_refused=true; no_rescan_after_reopen=true");
}

#[test]
fn independent_review_terminal_successor_cache_precedes_capacity_and_secret_inventory() {
    let mut f = live(plan("https://example.test"), CredentialFields::Password, PasswordPolicy::RememberedOrigins, 1000);
    let root = f.prepare(None);
    f.state.decide_credential_use(owner(), root.clone(), UseDecision::RememberPassword).unwrap();
    let successor = f.prepare(Some(root.id));
    f.state.recover_credential_uses().unwrap();
    let tx = f.state.connection.transaction().unwrap();
    for _ in 2..MAX_CREDENTIAL_USES {
        let mut filler = root.clone();
        filler.id = CredentialUseId::new();
        filler.release = ReleaseId::new();
        filler.binding.task = TaskId::new();
        filler.destination.profile = ProfileEpoch::new();
        filler.state = UseState::Expired;
        tx.execute("INSERT INTO credential_uses VALUES(?1,?2,?3)", params![filler.id.to_string(), filler.release.to_string(), serde_json::to_string(&filler).unwrap()]).unwrap();
    }
    tx.commit().unwrap();
    f.state.connection.execute_batch("ALTER TABLE connection_versions RENAME TO unavailable_versions").unwrap();
    let cached = f.prepare(Some(root.id));
    assert_eq!(cached.id, successor.id);
    assert_eq!(cached.expires_at, root.expires_at);
    assert_eq!(cached.state, UseState::Revoked);
    assert_eq!(f.prepare(None).id, root.id);
    assert!(f.release(&cached).is_err());
    let count: i64 = f.state.connection.query_row("SELECT COUNT(*) FROM credential_uses", [], |r| r.get(0)).unwrap();
    assert_eq!(count, MAX_CREDENTIAL_USES as i64);
    println!("revoked_successor_cached_at_capacity=true; no_global_secret_scan_on_retry=true; no_release_reopened=true");
}

#[test]
fn independent_review_uncertain_history_survives_recovery_without_old_authority() {
    let mut f = live(plan("https://example.test"), CredentialFields::Password, PasswordPolicy::EveryFill, 1000);
    let review = f.prepare(None);
    f.state.decide_credential_use(owner(), review.clone(), UseDecision::AllowOnce).unwrap();
    f.release(&review).unwrap();
    f.state.recover_credential_uses().unwrap();
    let saved: CredentialUseReview = parse(f.state.connection.query_row("SELECT view FROM credential_uses WHERE id=?1", [review.id.to_string()], |r| r.get(0)).unwrap()).unwrap();
    assert_eq!(saved.state, UseState::Uncertain);
    f.next_parent(true);
    f.state.connection.execute("DELETE FROM credential_authorities WHERE use_id=?1", [review.id.to_string()]).unwrap();
    f.state.connection.execute_batch("ALTER TABLE connection_versions RENAME TO unavailable_versions").unwrap();
    assert!(review_screen(&mut f, vec![review.release], "Recovered account status").is_ok());
    assert!(review_screen(&mut f, vec![review.release], &live_values().password).is_err());
    assert!(review_screen(&mut f, vec![review.release], &live_values().totp_seed).is_ok());
    assert!(review_screen(&mut f, vec![], "Recovered account status").is_err());
    println!("uncertain_history_retained=true; old_fill_authority_not_required=true; global_inventory_not_read=true; seed_not_screened=true");
}

#[test]
fn independent_review_migration16_preserves_legacy_review_and_denies_multiplicity() {
    let mut f = live(plan("https://example.test"), CredentialFields::Password, PasswordPolicy::EveryFill, 1000);
    let root = f.prepare(None);
    f.state.connection.execute_batch("DROP TABLE credential_prepare_attempts; PRAGMA user_version=15;").unwrap();
    migrate(&mut f.state.connection).unwrap();
    assert_eq!(f.prepare(None), root);
    let mut legacy = root.clone();
    legacy.id = CredentialUseId::new();
    legacy.release = ReleaseId::new();
    f.state.connection.execute("INSERT INTO credential_uses VALUES(?1,?2,?3)", params![legacy.id.to_string(), legacy.release.to_string(), serde_json::to_string(&legacy).unwrap()]).unwrap();
    assert!(f.call(ProtectedBrowserOperation::Prepare { destination: f.destination.clone(), previous: None }).is_err());
    let count: i64 = f.state.connection.query_row("SELECT COUNT(*) FROM credential_uses", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 2);
    println!("migration16_preserves_existing_root=true; legacy_multiplicity_refused_without_deletion=true");
}
