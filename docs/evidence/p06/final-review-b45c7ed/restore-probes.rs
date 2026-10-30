#[test]
fn fresh_recheck_existing_revoked_snapshot_unlock_and_relock() {
    let mut f = fixture();
    let mut ids = vec![];
    let secrets = ["restore-independent-private-one", "restore-independent-private-two"];
    for (i, secret) in secrets.iter().enumerate() {
        let mut view = f.state.install_connection(copy_token(&f.token), InstallConnection {
            id: ConnectionId::new(), label: format!("Recovery connection {i}"),
            transport: ConnectionTransport::RemoteHttp { endpoint: "https://example.test/mcp".into() },
        }).unwrap();
        view = f.state.replace_connection_secret(copy_token(&f.token), view.id, ReplaceConnectionSecret {
            expected_revision: view.version.revision,
            secret: ConnectionSecretInput::Bearer { value: ConnectorSecretEntry((*secret).into()) },
        }).unwrap();
        view = f.state.change_connection(copy_token(&f.token), view.id, view.version.revision, false).unwrap();
        view = f.state.change_connection(copy_token(&f.token), view.id, view.version.revision, true).unwrap();
        ids.push(view.id);
    }
    let request = RequestId::new();
    let receipt = f.state.write_login(copy_token(&f.token), request, CredentialWriteTarget::Create { metadata: metadata() }, values(91)).unwrap();
    assert!(matches!(receipt, CredentialWriteStatus::Committed { .. }));
    let snapshot = f.root.path().canonicalize().unwrap().join("independent-revoked.sqlite3");
    f.state.connection.execute("VACUUM INTO ?1", [snapshot.to_str().unwrap()]).unwrap();
    let count_before: i64 = f.state.connection.query_row("SELECT count(*) FROM connection_secrets", [], |r| r.get(0)).unwrap();
    let mut restored = State::from_connection(Connection::open(snapshot).unwrap(), f.root.path().canonicalize().unwrap(), watch::channel(false).0).unwrap();
    assert!(matches!(restored.known_connector_secrets(), Err(OperationError::Locked)));
    let owner = restored.sign_in(LoginPassword::new("synthetic p06 owner password".into()).unwrap()).unwrap().token;
    for cycle in 0..2 {
        restored.unlock_vault(copy_token(&owner), VaultPassphrase::new("synthetic p06 vault passphrase".into()).unwrap()).unwrap();
        for secret in secrets { assert_eq!(restored.reject_known_secrets(&secret), Err(OperationError::InvalidInput)); }
        assert_eq!(restored.reject_known_secrets(&"Independent benign text."), Ok(()));
        assert_eq!(restored.credential_write_status(copy_token(&owner), request, false).unwrap(), receipt);
        assert_eq!(restored.login_items(copy_token(&owner)).unwrap().len(), 1);
        for id in &ids {
            let view = restored.load_connection(*id).unwrap();
            assert_eq!(view.availability, ConnectionAvailability::Revoked);
            assert!(restored.change_connection(copy_token(&owner), *id, view.version.revision, false).is_err());
        }
        if cycle == 0 {
            restored.lock_vault(copy_token(&owner)).unwrap();
            assert!(matches!(restored.known_connector_secrets(), Err(OperationError::Locked)));
        }
    }
    assert!(matches!(restored.write_login(copy_token(&owner), RequestId::new(), CredentialWriteTarget::Create { metadata: metadata() }, values(92)).unwrap(), CredentialWriteStatus::Committed { .. }));
    let count_after: i64 = restored.connection.query_row("SELECT count(*) FROM connection_secrets", [], |r|r.get(0)).unwrap();
    assert_eq!(count_before, 6);
    assert_eq!(count_before, count_after);
    println!("restored_connections=2; reconnect_history_retained=6; real_passphrase_unlock_cycles=2; safe_entry=true; receipt_recovery=true; revoked_authority_refused=true");
}
