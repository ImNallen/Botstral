use super::*;
use crate::worker_http::VerifiedIdentity;

fn fixture() -> (tempfile::TempDir, State) {
    let temp = tempfile::tempdir().unwrap();
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    db.execute_batch("INSERT INTO owner VALUES(1, 'Worker test owner', 'unused'); INSERT INTO vault VALUES(1, 1, X'00');").unwrap();
    let (configured, _) = watch::channel(false);
    let mut state = State::from_connection(db, temp.path().into(), configured).unwrap();
    state.test_time = Some(1000);
    state
        .connection
        .execute(
            "INSERT INTO sessions VALUES(?1, ?2, 1, 999, 2000)",
            params![
                Uuid::new_v4().to_string(),
                crypto::token_hash(&"o".repeat(43)).as_slice()
            ],
        )
        .unwrap();
    state.advertise_for_test();
    (temp, state)
}
fn owner() -> SessionToken {
    SessionToken::new("o".repeat(43)).unwrap()
}
fn peer() -> VerifiedIdentity {
    VerifiedIdentity::for_test(b"certificate A")
}
fn enroll(state: &mut State) -> WorkerId {
    let grant = state.invite_worker(owner(), 600).unwrap();
    state.enroll_worker(peer(), grant.secret).unwrap()
}

#[test]
fn invitation_binding_expiry_atomicity_and_replay() {
    let (_temp, mut state) = fixture();
    let grant = state.invite_worker(owner(), 1).unwrap();
    let secret = || SecretString::from(grant.secret.expose_secret());
    assert_eq!(
        state.enroll_worker(peer(), SecretString::from(crypto::random_token())),
        Err(ErrorCode::InvalidInvitation.into())
    );
    state.test_time = Some(1001);
    assert_eq!(
        state.enroll_worker(peer(), secret()),
        Err(ErrorCode::InvitationExpired.into())
    );
    state.test_time = Some(1000);
    let fresh = state.invite_worker(owner(), 600).unwrap();
    let fresh_secret = || SecretString::from(fresh.secret.expose_secret());
    let id = state.enroll_worker(peer(), fresh_secret()).unwrap();
    assert_eq!(state.enroll_worker(peer(), fresh_secret()).unwrap(), id);
    assert_eq!(
        state.enroll_worker(VerifiedIdentity::for_test(b"certificate B"), fresh_secret()),
        Err(ErrorCode::InvitationUsed.into())
    );
    let mut tampered = fresh.secret.expose_secret().to_owned();
    tampered.replace_range(0..1, "!");
    assert_eq!(
        state.enroll_worker(peer(), SecretString::from(tampered)),
        Err(ErrorCode::InvalidInvitation.into())
    );
}

#[test]
fn unregistered_worker_is_refused_and_revocation_is_immediate() {
    let (_temp, mut state) = fixture();
    let worker = enroll(&mut state);
    let (out, _) = mpsc::channel(1);
    assert!(matches!(
        state.connect_worker(
            VerifiedIdentity::for_test(b"unregistered worker"),
            Hello::current(false),
            out,
            watch::channel(false).0,
            RequestId::new(),
        ),
        Err(OperationError::Protocol(ErrorCode::Unauthorized))
    ));
    let (out, _receive) = mpsc::channel(8);
    let (stop, stopped) = watch::channel(false);
    state
        .connect_worker(peer(), Hello::current(false), out, stop, RequestId::new())
        .unwrap();
    state.revoke_worker(owner(), worker).unwrap();
    assert!(*stopped.borrow());
    let (out, _) = mpsc::channel(1);
    assert!(matches!(
        state.connect_worker(
            peer(),
            Hello::current(false),
            out,
            watch::channel(false).0,
            RequestId::new(),
        ),
        Err(OperationError::Protocol(ErrorCode::Unauthorized))
    ));
}

#[test]
fn pending_takeover_blocks_input_and_fences_queued_delivery() {
    let (_temp, mut state) = fixture();
    let worker = enroll(&mut state);
    let mut sessions = Vec::new();
    for name in ["A", "B"] {
        let bot = BotId::new();
        state
            .create_bot(
                owner(),
                NewBot {
                    id: bot,
                    name: BotName::new(name.into()).unwrap(),
                },
            )
            .unwrap();
        sessions.push(state.open_computer_session(owner(), bot, worker).unwrap());
    }
    let (out, mut receive) = mpsc::channel(32);
    let (stop, _) = watch::channel(false);
    let lease = state
        .connect_worker(peer(), Hello::current(false), out, stop, RequestId::new())
        .unwrap();
    receive.try_recv().unwrap();
    let initial = state.find_computer_session(sessions[0].id).unwrap();
    let pending = state
        .set_input_owner(owner(), initial.id, initial.revision, InputOwner::Owner)
        .unwrap();
    assert_eq!(pending.status, ControlStatus::Pending);
    state
        .worker_report(
            lease,
            Envelope::new(WorkerMessage::SnapshotApplied {
                generation: lease.generation,
            }),
        )
        .unwrap();
    assert_eq!(
        state.find_computer_session(pending.id).unwrap().status,
        ControlStatus::Pending
    );
    assert_eq!(
        state.find_computer_session(sessions[1].id).unwrap().status,
        ControlStatus::Confirmed
    );
    let input = ManagerMessage::Input {
        command_id: CommandId::new(),
        fence: pending.fence(lease.generation),
        value: 1,
    };
    assert!(matches!(
        state.dispatch(owner(), input.clone()),
        Err(OperationError::Protocol(ErrorCode::WrongPhase))
    ));
    state
        .worker_report(
            lease,
            Envelope::new(WorkerMessage::ControlApplied {
                fence: pending.fence(lease.generation),
            }),
        )
        .unwrap();
    state.dispatch(owner(), input.clone()).unwrap();
    let changed = state
        .set_input_owner(owner(), pending.id, pending.revision, InputOwner::Bot)
        .unwrap();
    assert_eq!(
        state.admit_delivery(lease, &input),
        Err(ErrorCode::StaleRevision.into())
    );
    assert_eq!(
        state.check_input_fence(worker, changed.fence(lease.generation)),
        Err(ErrorCode::WrongPhase.into())
    );
    let other = state.find_computer_session(sessions[1].id).unwrap();
    assert_eq!(
        state.check_input_fence(worker, other.fence(lease.generation)),
        Ok(())
    );
    state
        .worker_report(
            lease,
            Envelope::new(WorkerMessage::ControlApplied {
                fence: pending.fence(lease.generation),
            }),
        )
        .unwrap();
    assert_eq!(
        state.check_input_fence(worker, changed.fence(lease.generation)),
        Err(ErrorCode::WrongPhase.into())
    );
    assert_eq!(
        state.check_input_fence(worker, other.fence(lease.generation)),
        Ok(())
    );
    state
        .worker_report(
            lease,
            Envelope::new(WorkerMessage::ControlApplied {
                fence: changed.fence(lease.generation),
            }),
        )
        .unwrap();
    assert_eq!(
        state.check_input_fence(worker, changed.fence(lease.generation)),
        Ok(())
    );

    let repeated = state
        .set_input_owner(owner(), changed.id, changed.revision, InputOwner::Bot)
        .unwrap();
    assert_eq!(repeated.revision, changed.revision);
    state.test_time = Some(2000);
    assert!(matches!(
        state.set_input_owner(owner(), changed.id, changed.revision, InputOwner::Owner),
        Err(OperationError::Unauthorized)
    ));
}

#[test]
fn snapshots_generations_late_disconnect_and_full_channels() {
    let (_temp, mut state) = fixture();
    let worker = enroll(&mut state);
    let bot = BotId::new();
    state
        .create_bot(
            owner(),
            NewBot {
                id: bot,
                name: BotName::new("A".into()).unwrap(),
            },
        )
        .unwrap();
    let session = state.open_computer_session(owner(), bot, worker).unwrap();
    let connect = |state: &mut State| {
        let (out, receive) = mpsc::channel(1);
        let (stop, stopped) = watch::channel(false);
        let lease = state
            .connect_worker(peer(), Hello::current(false), out, stop, RequestId::new())
            .unwrap();
        (lease, receive, stopped)
    };
    let (old, mut receive, mut stopped) = connect(&mut state);
    receive.try_recv().unwrap();
    assert_eq!(
        state.find_computer_session(session.id).unwrap().status,
        ControlStatus::Pending
    );
    state
        .worker_report(
            old,
            Envelope::new(WorkerMessage::SnapshotApplied {
                generation: old.generation,
            }),
        )
        .unwrap();
    assert_eq!(
        state.find_computer_session(session.id).unwrap().status,
        ControlStatus::Confirmed
    );
    let (new, mut receive, _) = connect(&mut state);
    assert!(*stopped.borrow_and_update());
    assert!(new.generation > old.generation);
    state.disconnect_worker(old);
    assert!(state.check_lease(new).is_ok());
    assert!(matches!(
        state.worker_report(
            old,
            Envelope::new(WorkerMessage::Heartbeat {
                generation: old.generation
            })
        ),
        Err(OperationError::Protocol(ErrorCode::StaleGeneration))
    ));
    receive.try_recv().unwrap();
    state
        .worker_report(
            new,
            Envelope::new(WorkerMessage::SnapshotApplied {
                generation: new.generation,
            }),
        )
        .unwrap();
    assert!(state.find_computer_session(session.id).unwrap().revision > session.revision);
    state
        .deliver(worker, Envelope::new(ManagerMessage::Pong))
        .unwrap();
    assert_eq!(
        state.deliver(worker, Envelope::new(ManagerMessage::Pong)),
        Err(ErrorCode::QueueFull.into())
    );
    assert_eq!(
        state.find_computer_session(session.id).unwrap().status,
        ControlStatus::Disconnected
    );
    state.test_time = Some(2000);
    assert_eq!(
        state.revoke_worker(owner(), worker),
        Err(OperationError::Unauthorized)
    );
}

#[test]
fn certificate_renewal_rebinds_only_after_owner_revocation() {
    let (_temp, mut state) = fixture();
    let old = enroll(&mut state);
    let replacement = VerifiedIdentity::for_test(b"renewed certificate");
    let grant = state.invite_worker(owner(), 600).unwrap();
    let new = state.enroll_worker(replacement, grant.secret).unwrap();
    let bot = BotId::new();
    state
        .create_bot(
            owner(),
            NewBot {
                id: bot,
                name: BotName::new("Preserved bot".into()).unwrap(),
            },
        )
        .unwrap();
    let session = state.open_computer_session(owner(), bot, old).unwrap();
    assert_eq!(
        state.open_computer_session(owner(), bot, new),
        Err(OperationError::Conflict)
    );
    state.revoke_worker(owner(), old).unwrap();
    let rebound = state.open_computer_session(owner(), bot, new).unwrap();
    assert_eq!(rebound.id, session.id);
    assert_eq!(rebound.worker_id, new);
    assert!(rebound.revision > session.revision);
    assert_eq!(rebound.status, ControlStatus::Disconnected);
}
