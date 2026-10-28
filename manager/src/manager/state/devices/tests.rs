use super::*;
use std::os::unix::fs::PermissionsExt;

fn setup(root: &std::path::Path) -> (State, SessionToken) {
    fs::set_permissions(root, fs::Permissions::from_mode(0o700)).unwrap();
    let _local = crate::LocalState::open(root).unwrap();
    let (configured, _) = watch::channel(false);
    let mut state = State::open(root.canonicalize().unwrap(), configured).unwrap();
    let proof =
        BootstrapToken::new(fs::read_to_string(root.join("bootstrap.token")).unwrap()).unwrap();
    state
        .create_owner(
            proof,
            OwnerSetup {
                name: OwnerName::new("Synthetic owner".into()).unwrap(),
                login_password: LoginPassword::new("synthetic login password".into()).unwrap(),
                vault_passphrase: VaultPassphrase::new("synthetic vault passphrase".into())
                    .unwrap(),
                recovery: RecoveryAcknowledgment::OfflinePassphraseCopy,
            },
        )
        .unwrap();
    let token = state
        .sign_in(LoginPassword::new("synthetic login password".into()).unwrap())
        .unwrap()
        .token;
    (state, token)
}
fn token(token: &SessionToken) -> SessionToken {
    SessionToken::new(token.0.expose_secret().into()).unwrap()
}
fn auth(manager: ManagerId, secret: &str) -> OwnerHello {
    OwnerHello::Authenticate {
        version: OWNER_VERSION,
        manager,
        role: DeviceRole::Owner,
        credential: secret.into(),
    }
}
fn enroll(manager: ManagerId, secret: &str, invitation: &str) -> OwnerHello {
    OwnerHello::Enroll {
        version: OWNER_VERSION,
        manager,
        role: DeviceRole::Owner,
        credential: secret.into(),
        invitation: invitation.into(),
    }
}
fn stop() -> watch::Sender<bool> {
    watch::channel(false).0
}

#[test]
fn enrollment_binding_atomicity_lost_response_and_revocation_survive_restart() {
    let root = tempfile::tempdir().unwrap();
    let (mut state, local) = setup(root.path());
    state.test_time = Some(1000);
    let manager = state.manager_identity().unwrap();
    let secret = crypto::random_token();
    let request = PairingRequest {
        fingerprint: DeviceFingerprint::from_credential(&secret),
        label: "Probe".into(),
    };
    let grant = state
        .invite_device(token(&local), request.clone(), 60)
        .unwrap();
    let invitation = grant.secret.expose_secret();
    assert!(matches!(
        state.connect_owner(enroll(ManagerId::new(), &secret, invitation), stop()),
        Err(OperationError::Unauthorized)
    ));
    let mut wrong_role = enroll(manager, &secret, invitation);
    if let OwnerHello::Enroll { role, .. } = &mut wrong_role {
        *role = DeviceRole::Worker;
    }
    assert!(matches!(
        state.connect_owner(wrong_role, stop()),
        Err(OperationError::Unauthorized)
    ));
    assert!(matches!(
        state.connect_owner(enroll(manager, &crypto::random_token(), invitation), stop()),
        Err(OperationError::Unauthorized)
    ));
    state.connection.execute_batch("CREATE TRIGGER reject_consume BEFORE UPDATE ON owner_invitations BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(matches!(
        state.connect_owner(enroll(manager, &secret, invitation), stop()),
        Err(OperationError::Internal)
    ));
    assert_eq!(
        state
            .connection
            .query_row::<i64, _, _>("SELECT count(*) FROM owner_devices", [], |r| r.get(0))
            .unwrap(),
        0
    );
    state
        .connection
        .execute_batch("DROP TRIGGER reject_consume")
        .unwrap();
    let (first_stop, first_stopped) = watch::channel(false);
    let (_, first) = state
        .connect_owner(enroll(manager, &secret, invitation), first_stop)
        .unwrap();
    assert!(matches!(
        state.connect_owner(enroll(manager, &secret, invitation), stop()),
        Err(OperationError::Unauthorized)
    ));
    let (second_stop, second_stopped) = watch::channel(false);
    let (_, second) = state
        .connect_owner(auth(manager, &secret), second_stop)
        .unwrap();
    assert_eq!(first.device, second.device);
    state.disconnect_owner(first);
    assert!(state.resume(second, 0, 64).is_ok());
    assert!(matches!(
        state.resume(first, 0, 64),
        Err(OperationError::Unauthorized)
    ));
    let (third_stop, third_stopped) = watch::channel(false);
    state
        .connect_owner(auth(manager, &secret), third_stop)
        .unwrap();
    state.revoke_device(token(&local), second.device).unwrap();
    assert!(*second_stopped.borrow());
    assert!(*third_stopped.borrow());
    assert!(!*first_stopped.borrow());
    assert!(matches!(
        state.resume(second, 0, 64),
        Err(OperationError::Unauthorized)
    ));
    assert!(matches!(
        state.invite_device(token(&local), request, 60),
        Err(OperationError::Conflict)
    ));
    assert!(matches!(
        state.connect_owner(auth(manager, &secret), stop()),
        Err(OperationError::Unauthorized)
    ));
    drop(state);
    let (configured, _) = watch::channel(false);
    let mut state = State::open(root.path().canonicalize().unwrap(), configured).unwrap();
    assert_eq!(state.manager_identity().unwrap(), manager);
    assert!(matches!(
        state.owner(local),
        Err(OperationError::Unauthorized)
    ));
    assert!(matches!(
        state.connect_owner(auth(manager, &secret), stop()),
        Err(OperationError::Unauthorized)
    ));
    assert_eq!(
        state
            .connection
            .query_row::<i64, _, _>("SELECT revoked FROM owner_devices", [], |r| r.get(0))
            .unwrap(),
        1
    );
}

#[test]
fn rejected_handshakes_cannot_lock_out_valid_owner_or_worker_credentials() {
    let root = tempfile::tempdir().unwrap();
    let (mut state, local) = setup(root.path());
    state.test_time = Some(state.now());
    let secret = crypto::random_token();
    let grant = state
        .invite_device(
            token(&local),
            PairingRequest {
                fingerprint: DeviceFingerprint::from_credential(&secret),
                label: "Reconnect".into(),
            },
            600,
        )
        .unwrap();
    let (_, lease) = state
        .connect_owner(
            enroll(grant.manager, &secret, grant.secret.expose_secret()),
            stop(),
        )
        .unwrap();
    state.disconnect_owner(lease);
    for _ in 0..120 {
        let _ = state.connect_owner(auth(ManagerId::new(), &secret), stop());
    }
    assert!(matches!(
        state.connect_owner(auth(ManagerId::new(), &secret), stop()),
        Err(OperationError::RateLimited)
    ));
    let authenticated = state.connect_owner(auth(grant.manager, &secret), stop());
    assert!(
        authenticated.is_ok(),
        "Valid owner authentication failed with {:?}",
        authenticated.err()
    );
    let peer = crate::worker_http::VerifiedIdentity::for_test(b"valid worker after flood");
    let worker = state
        .invite_worker(token(&local), peer.fingerprint().clone(), 600)
        .unwrap();
    assert!(state.enroll_worker(peer, worker.secret).is_ok());
}

#[test]
fn invitations_expire_and_attempt_limits_persist() {
    let root = tempfile::tempdir().unwrap();
    let (mut state, local) = setup(root.path());
    state.test_time = Some(state.now());
    let now = state.now();
    let secret = crypto::random_token();
    let grant = state
        .invite_device(
            token(&local),
            PairingRequest {
                fingerprint: DeviceFingerprint::from_credential(&secret),
                label: "Expiring".into(),
            },
            1,
        )
        .unwrap();
    state.test_time = Some(now + 1);
    assert!(matches!(
        state.connect_owner(
            enroll(grant.manager, &secret, grant.secret.expose_secret()),
            stop()
        ),
        Err(OperationError::Unauthorized)
    ));
    state.test_time = Some(now);
    for _ in 1..120 {
        let _ = state.connect_owner(auth(grant.manager, &secret), stop());
    }
    assert!(matches!(
        state.connect_owner(auth(grant.manager, &secret), stop()),
        Err(OperationError::RateLimited)
    ));
    drop(state);
    let (configured, _) = watch::channel(false);
    let mut state = State::open(root.path().canonicalize().unwrap(), configured).unwrap();
    state.test_time = Some(now + 2);
    assert!(matches!(
        state.connect_owner(auth(grant.manager, &secret), stop()),
        Err(OperationError::RateLimited)
    ));
    state.test_time = Some(now + 62);
    assert!(matches!(
        state.connect_owner(auth(grant.manager, &secret), stop()),
        Err(OperationError::Unauthorized)
    ));
}
