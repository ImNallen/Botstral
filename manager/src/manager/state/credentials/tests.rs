use super::*;
use crate::manager::devices::DeviceLease;
use std::os::unix::fs::PermissionsExt;

struct Fixture {
    root: tempfile::TempDir,
    state: State,
    token: SessionToken,
    lease: DeviceLease,
}
fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let _local = crate::LocalState::open(root.path()).unwrap();
    let mut state =
        State::open(root.path().canonicalize().unwrap(), watch::channel(false).0).unwrap();
    let bootstrap =
        BootstrapToken::new(fs::read_to_string(root.path().join("bootstrap.token")).unwrap())
            .unwrap();
    state
        .create_owner(
            bootstrap,
            OwnerSetup {
                name: OwnerName::new("Fixture owner".into()).unwrap(),
                login_password: LoginPassword::new("synthetic p06 owner password".into()).unwrap(),
                vault_passphrase: VaultPassphrase::new("synthetic p06 vault passphrase".into())
                    .unwrap(),
                recovery: RecoveryAcknowledgment::OfflinePassphraseCopy,
            },
        )
        .unwrap();
    let token = state
        .sign_in(LoginPassword::new("synthetic p06 owner password".into()).unwrap())
        .unwrap()
        .token;
    let secret = crypto::random_token();
    let grant = state
        .invite_device(
            copy_token(&token),
            PairingRequest {
                fingerprint: DeviceFingerprint::from_credential(&secret),
                label: "Fixture device".into(),
            },
            60,
        )
        .unwrap();
    let (_, lease) = state
        .connect_owner(
            OwnerHello::Enroll {
                version: OWNER_VERSION,
                manager: state.manager_identity().unwrap(),
                role: DeviceRole::Owner,
                credential: secret,
                invitation: grant.secret.expose_secret().into(),
            },
            watch::channel(false).0,
        )
        .unwrap();
    state
        .unlock_protected(
            &OwnerAuthority::Device(lease),
            VaultPassphrase::new("synthetic p06 vault passphrase".into()).unwrap(),
        )
        .unwrap();
    Fixture {
        root,
        state,
        token,
        lease,
    }
}
fn copy_token(token: &SessionToken) -> SessionToken {
    SessionToken::new(token.0.expose_secret().into()).unwrap()
}
fn metadata() -> LoginMetadata {
    LoginMetadata {
        label: "Fixture account".into(),
        bots: BotAccess::AllOwnerBots,
        allowed_origins: vec!["https://example.test".into()],
        password_policy: PasswordPolicy::EveryFill,
    }
}
fn values(number: u8) -> LoginValues {
    LoginValues {
        username: format!("synthetic-p06-user-{number}"),
        password: format!("synthetic-p06-private-\"\\\n-{number}"),
        totp_seed: "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ".into(),
    }
}
fn committed(status: CredentialWriteStatus) -> LoginItemView {
    match status {
        CredentialWriteStatus::Committed { item, .. } => item,
        _ => panic!("expected committed receipt"),
    }
}

#[test]
fn create_rotate_remove_retain_inventory_and_value_free_receipts() {
    let mut f = fixture();
    let request = RequestId::new();
    let status = f
        .state
        .write_login(
            f.lease,
            request,
            CredentialWriteTarget::Create {
                metadata: metadata(),
            },
            values(1),
        )
        .unwrap();
    let item = committed(status.clone());
    assert_eq!(
        f.state
            .write_login(
                f.lease,
                request,
                CredentialWriteTarget::Create {
                    metadata: metadata()
                },
                values(2)
            )
            .unwrap(),
        status
    );
    assert_eq!(f.state.login_items(f.lease).unwrap(), vec![item.clone()]);
    let rotated = committed(
        f.state
            .write_login(
                f.lease,
                RequestId::new(),
                CredentialWriteTarget::Rotate {
                    id: item.id,
                    expected_revision: item.revision,
                },
                values(2),
            )
            .unwrap(),
    );
    assert_eq!(rotated.epoch.get(), 2);
    assert!(
        f.state
            .update_login(f.lease, item.id, item.revision, None)
            .is_err()
    );
    let removed = f
        .state
        .update_login(f.lease, item.id, rotated.revision, None)
        .unwrap();
    assert_eq!(removed.state, CredentialState::Removed);
    assert!(
        f.state
            .write_login(
                f.lease,
                RequestId::new(),
                CredentialWriteTarget::Rotate {
                    id: item.id,
                    expected_revision: removed.revision
                },
                values(3)
            )
            .is_err()
    );
    for number in [1, 2] {
        for secret in [
            &values(number).username,
            &values(number).password,
            &values(number).totp_seed,
        ] {
            let escaped = serde_json::json!({"nested": serde_json::to_string(&serde_json::json!({"echo": secret})).unwrap()});
            assert!(f.state.reject_known_secrets(&escaped).is_err());
            assert!(!json(&status).unwrap().contains(secret));
        }
    }
    f.state
        .connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    for path in fs::read_dir(f.root.path())
        .unwrap()
        .map(|p| p.unwrap().path())
        .filter(|p| p.is_file())
    {
        let bytes = fs::read(path).unwrap();
        for number in [1, 2] {
            for secret in [
                &values(number).username,
                &values(number).password,
                &values(number).totp_seed,
            ] {
                assert!(
                    !bytes.windows(secret.len()).any(|b| b == secret.as_bytes()),
                    "plaintext in operational root"
                );
            }
        }
    }
}

#[test]
fn closure_is_final_and_revoked_connections_cannot_write() {
    let mut f = fixture();
    let request = RequestId::new();
    assert!(matches!(
        f.state
            .credential_write_status(f.lease, request, false)
            .unwrap(),
        CredentialWriteStatus::Unresolved { .. }
    ));
    let closed = f
        .state
        .credential_write_status(f.lease, request, true)
        .unwrap();
    assert!(matches!(
        closed,
        CredentialWriteStatus::ClosedWithoutCommit { .. }
    ));
    assert_eq!(
        f.state
            .write_login(
                f.lease,
                request,
                CredentialWriteTarget::Create {
                    metadata: metadata()
                },
                values(1)
            )
            .unwrap(),
        closed
    );
    assert!(f.state.login_items(f.lease).unwrap().is_empty());
    f.state.disconnect_owner(f.lease);
    assert!(matches!(
        f.state.write_login(
            f.lease,
            RequestId::new(),
            CredentialWriteTarget::Create {
                metadata: metadata()
            },
            values(1)
        ),
        Err(OperationError::Unauthorized)
    ));
}

#[test]
fn ciphertext_binding_restart_and_restore_stay_locked() {
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
    let cipher: Vec<u8> = f
        .state
        .connection
        .query_row("SELECT ciphertext FROM credential_epochs", [], |r| r.get(0))
        .unwrap();
    assert!(
        f.state
            .login_values(CredentialId::new(), item.epoch, &cipher)
            .is_err()
    );
    assert!(
        f.state
            .login_values(item.id, item.epoch.advance().unwrap(), &cipher)
            .is_err()
    );
    let backup = f
        .root
        .path()
        .canonicalize()
        .unwrap()
        .join("snapshot.sqlite");
    f.state
        .connection
        .execute("VACUUM INTO ?1", [backup.to_str().unwrap()])
        .unwrap();
    f.state.lock_vault(copy_token(&f.token)).unwrap();
    assert!(matches!(
        f.state.known_login_secrets(),
        Err(OperationError::Locked)
    ));
    let mut restored = State::from_connection(
        Connection::open(backup).unwrap(),
        f.root.path().to_path_buf(),
        watch::channel(false).0,
    )
    .unwrap();
    assert!(matches!(
        restored.known_login_secrets(),
        Err(OperationError::Locked)
    ));
    let token = restored
        .sign_in(LoginPassword::new("synthetic p06 owner password".into()).unwrap())
        .unwrap()
        .token;
    restored
        .unlock_vault(
            copy_token(&token),
            VaultPassphrase::new("synthetic p06 vault passphrase".into()).unwrap(),
        )
        .unwrap();
    assert!(restored.reject_known_secrets(&values(1).password).is_err());
    assert_eq!(restored.login_items(token).unwrap(), vec![item]);
}

#[test]
fn metadata_cannot_publish_private_values_or_ambiguous_origins() {
    let mut f = fixture();
    for origin in [
        "http://example.test",
        "https://example.test/",
        "https://example.test:443",
        "https://user@example.test",
        "https://example.test?q=x",
        "https://*.example.test",
        "https://example.test/path",
    ] {
        let mut metadata = metadata();
        metadata.allowed_origins = vec![origin.into()];
        assert!(
            f.state
                .write_login(
                    f.lease,
                    RequestId::new(),
                    CredentialWriteTarget::Create { metadata },
                    values(1)
                )
                .is_err(),
            "origin admitted"
        );
    }
    let mut metadata = metadata();
    metadata.label = values(1).username.clone();
    assert!(matches!(
        f.state
            .write_login(
                f.lease,
                RequestId::new(),
                CredentialWriteTarget::Create { metadata },
                values(1)
            )
            .unwrap(),
        CredentialWriteStatus::Rejected {
            reason: CredentialWriteRejection::PublicValueCollision,
            ..
        }
    ));
}

mod live;
