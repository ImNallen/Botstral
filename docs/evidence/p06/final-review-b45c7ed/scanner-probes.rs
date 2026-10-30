#[test]
fn fresh_recheck_revoked_epoch_order_integrity_and_dedup() {
    use age::secrecy::ExposeSecret;
    for reverse in [false, true] {
        let mut f = fixture();
        let installed = f.install(false);
        let mut view = installed.clone();
        let values = ["independent-first-retained-bearer", "independent-second-retained-bearer"];
        for value in values {
            view = f.state.replace_connection_secret(token(), view.id, ReplaceConnectionSecret {
                expected_revision: view.version.revision,
                secret: ConnectionSecretInput::Bearer { value: ConnectorSecretEntry(value.into()) },
            }).unwrap();
        }
        let revoked = f.state.change_connection(token(), view.id, view.version.revision, true).unwrap();
        f.state.connection.execute_batch(if reverse { "PRAGMA reverse_unordered_selects=ON" } else { "PRAGMA reverse_unordered_selects=OFF" }).unwrap();
        let inventory = f.state.known_connector_secrets().unwrap();
        assert_eq!(inventory.len(), 2);
        for value in values { assert!(inventory.iter().any(|s| s.expose_secret() == value)); }
        assert_eq!(f.state.reject_known_secrets(&"A harmless note."), Ok(()));
        let mut duplicate = view.clone();
        duplicate.version.revision = 900;
        save_connection(&f.state.connection, &duplicate).unwrap();
        let mut duplicate_revoked = revoked.clone();
        duplicate_revoked.version.revision = 901;
        save_connection(&f.state.connection, &duplicate_revoked).unwrap();
        assert_eq!(f.state.known_connector_secrets().unwrap().len(), 2);
        let terminal = "independent-bound-terminal-private-value";
        let encrypted = crypto::encrypt_connector(f.state.connector_identity().unwrap(), &context(&revoked),
            &secret_bytes(&ConnectionSecretInput::Bearer { value: ConnectorSecretEntry(terminal.into()) }).unwrap()).unwrap();
        f.state.connection.execute("INSERT INTO connection_secrets VALUES(?1,?2,?3)", params![revoked.id.to_string(),revoked.version.credential_epoch as i64,encrypted]).unwrap();
        assert_eq!(f.state.known_connector_secrets().unwrap().len(), 3);
        assert_eq!(f.state.reject_known_secrets(&terminal), Err(OperationError::InvalidInput));
        println!("order_reverse={reverse}; legitimate_absence=true; actual_terminal_screened=true; duplicates_once=true");
    }
}

#[test]
fn fresh_recheck_missing_corrupt_and_mixed_epoch_must_refuse() {
    for reverse in [false, true] {
        for terminal in [false, true] {
            let mut f = fixture();
            let installed = f.install(false);
            let rotated = f.state.replace_connection_secret(token(), installed.id, ReplaceConnectionSecret {
                expected_revision: installed.version.revision,
                secret: ConnectionSecretInput::Bearer { value: ConnectorSecretEntry("independent-required-historical-value".into()) },
            }).unwrap();
            let revoked = f.state.change_connection(token(), rotated.id, rotated.version.revision, true).unwrap();
            f.state.connection.execute_batch(if reverse { "PRAGMA reverse_unordered_selects=ON" } else { "PRAGMA reverse_unordered_selects=OFF" }).unwrap();
            let target = if terminal { &revoked } else { &rotated };
            let encrypted = crypto::encrypt_connector(f.state.connector_identity().unwrap(), &context(target),
                &secret_bytes(&ConnectionSecretInput::Bearer { value: ConnectorSecretEntry("independent-required-historical-value".into()) }).unwrap()).unwrap();
            let triggers: Vec<(String,String)> = f.state.connection.prepare("SELECT name,sql FROM sqlite_master WHERE type='trigger' AND tbl_name='connection_secrets'").unwrap().query_map([], |r|Ok((r.get(0)?,r.get(1)?))).unwrap().map(Result::unwrap).collect();
            for (name,_) in &triggers { f.state.connection.execute_batch(&format!("DROP TRIGGER {name}")).unwrap(); }
            f.state.connection.execute("DELETE FROM connection_secrets WHERE connection=?1 AND epoch=?2",params![target.id.to_string(),target.version.credential_epoch as i64]).unwrap();
            if terminal { assert_eq!(f.state.reject_known_secrets(&"A harmless note."), Ok(())); }
            else { assert_eq!(f.state.reject_known_secrets(&"A harmless note."), Err(OperationError::Internal)); }
            f.state.connection.execute("INSERT INTO connection_secrets VALUES(?1,?2,?3)",params![target.id.to_string(),target.version.credential_epoch as i64,vec![0u8,1,2]]).unwrap();
            assert!(f.state.reject_known_secrets(&"A harmless note.").is_err(), "corrupt ciphertext must refuse");
            f.state.connection.execute("UPDATE connection_secrets SET ciphertext=?3 WHERE connection=?1 AND epoch=?2",params![target.id.to_string(),target.version.credential_epoch as i64,encrypted]).unwrap();
            for (_,sql) in triggers { f.state.connection.execute_batch(&sql).unwrap(); }
            assert_eq!(f.state.reject_known_secrets(&"independent-required-historical-value"), Err(OperationError::InvalidInput));
            assert_eq!(f.state.reject_known_secrets(&"A harmless note."), Ok(()));
            println!("order_reverse={reverse}; terminal={terminal}; missing_contract=true; corrupt_refused=true; restored_screened=true");
        }
        let mut f = fixture();
        let installed = f.install(false);
        let revoked = f.state.change_connection(token(), installed.id, installed.version.revision, true).unwrap();
        let mut mandatory = revoked.clone();
        mandatory.availability = ConnectionAvailability::NeedsDiscovery;
        mandatory.version.revision = 900;
        save_connection(&f.state.connection, &mandatory).unwrap();
        f.state.connection.execute_batch(if reverse { "PRAGMA reverse_unordered_selects=ON" } else { "PRAGMA reverse_unordered_selects=OFF" }).unwrap();
        assert_eq!(f.state.reject_known_secrets(&"A harmless note."), Err(OperationError::Internal));
        println!("order_reverse={reverse}; absent_revoked_cannot_hide_mandatory_same_epoch=true");
    }
}

#[test]
fn fresh_recheck_revoked_presence_query_failure_propagates() {
    let mut f = fixture();
    let installed = f.install(false);
    f.state.change_connection(token(), installed.id, installed.version.revision, true).unwrap();
    f.state.connection.execute_batch("PRAGMA reverse_unordered_selects=ON").unwrap();
    let first: String = f.state.connection.query_row("SELECT view FROM connection_versions", [], |r|r.get(0)).unwrap();
    assert_eq!(parse::<ConnectionView>(first).unwrap().availability, ConnectionAvailability::Revoked);
    f.state.connection.execute_batch("ALTER TABLE connection_secrets RENAME TO unavailable_secrets").unwrap();
    assert!(matches!(f.state.known_connector_secrets(), Err(OperationError::Internal)));
    f.state.connection.execute_batch("ALTER TABLE unavailable_secrets RENAME TO connection_secrets").unwrap();
    assert_eq!(f.state.reject_known_secrets(&"A harmless note."), Ok(()));
    println!("revoked_presence_query_error=Internal; restored_schema_safe=true");
}
