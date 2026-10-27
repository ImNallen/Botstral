use super::*;
use std::{fs, os::unix::fs::PermissionsExt};

fn reopen(fixture: &mut Fixture) {
    let path = fixture.state.directory.join("manager.sqlite3");
    fixture
        .state
        .connection
        .execute("VACUUM INTO ?1", [path.to_str().unwrap()])
        .unwrap();
    fs::set_permissions(
        fixture.state.directory.join("memory-index.sqlite3"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let installation = std::mem::replace(
        &mut fixture.state.installation,
        Installation::Configured {
            vault: VaultState::Locked,
        },
    );
    let (configured, _) = watch::channel(false);
    let mut state = State::open(fixture.state.directory.clone(), configured).unwrap();
    state.test_time = Some(1000);
    state.installation = installation;
    state
        .connection
        .execute(
            "INSERT INTO sessions VALUES(?1,?2,1,1,10000)",
            params![
                Uuid::new_v4().to_string(),
                crypto::token_hash(&"x".repeat(43)).as_slice()
            ],
        )
        .unwrap();
    fixture.state = state;
}

#[test]
fn reopening_at_committed_boundaries_preserves_call_child_and_result_identity() {
    for boundary in ["run", "call", "link", "result", "final"] {
        let mut f = fixture();
        let mut child = None;
        match boundary {
            "run" => {
                f.state.submit_run(token(), f.input.clone()).unwrap();
            }
            "call" | "final" => {
                f.state.submit_run(token(), f.input.clone()).unwrap();
                let permit = claim(&mut f.state);
                if boundary == "final" {
                    f.state
                        .finish_run_call(
                            f.input.id,
                            permit.turn.id,
                            permit.turn.attempt,
                            response(None),
                        )
                        .unwrap();
                }
            }
            "link" | "result" => {
                let task = f.propose();
                child = Some(task.command.clone());
                if boundary == "result" {
                    f.approve_and_claim(task.id);
                    f.finish(task.id);
                    let RunJob::Collect(permit) = f
                        .state
                        .claim_run_work(&HashSet::new(), true)
                        .unwrap()
                        .unwrap()
                    else {
                        panic!()
                    };
                    let turn = permit.turn;
                    let snapshot = permit.task.clone();
                    let result = crate::manager::runs::results::collect(*permit).unwrap();
                    f.state
                        .finish_run_result(f.input.id, turn, snapshot, Ok(result))
                        .unwrap();
                }
            }
            _ => unreachable!(),
        }
        let before = f.state.load_run(f.input.id).unwrap();
        reopen(&mut f);
        let after = f.state.load_run(f.input.id).unwrap();
        assert_eq!(before.request, after.request);
        assert_eq!(before.summary.accounting, after.summary.accounting);
        if let Some(command) = child {
            assert_eq!(f.state.load_task(command.task).unwrap().command, command);
        }
        for (old, new) in before.turns.iter().zip(&after.turns) {
            assert_eq!(old.id, new.id);
            assert_eq!(old.attempt, new.attempt);
            assert_eq!(old.messages, new.messages);
            assert_eq!(old.intent, new.intent);
        }
        match boundary {
            "run" => {
                let next = claim(&mut f.state);
                assert_eq!(next.turn.ordinal, 1);
            }
            "result" => {
                let next = claim(&mut f.state);
                assert_eq!(next.turn.ordinal, 2);
                assert!(
                    matches!(&next.turn.messages[2],RunMessage::Tool {tool_call_id,content} if tool_call_id=="call00001" && content.contains("hello"))
                );
            }
            "call" => {
                assert_eq!(
                    after.turns[0].phase,
                    RunTurnPhase::AwaitingAttention {
                        reason: InferenceAttentionReason::Restart
                    }
                );
                assert!(
                    f.state
                        .claim_run_work(&HashSet::new(), true)
                        .unwrap()
                        .is_none()
                );
            }
            _ => {
                assert_eq!(before, after);
                assert!(
                    f.state
                        .claim_run_work(&HashSet::new(), true)
                        .unwrap()
                        .is_none()
                );
            }
        }
    }
}

#[test]
fn admission_rollback_cannot_count_or_send_an_uncommitted_turn() {
    let mut f = fixture();
    f.state.submit_run(token(), f.input.clone()).unwrap();
    f.state.connection.execute_batch("CREATE TRIGGER reject_run_event BEFORE INSERT ON events BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(f.state.claim_run_work(&HashSet::new(), true).is_err());
    let view = f.state.load_run(f.input.id).unwrap();
    assert_eq!(view.summary.accounting, RunAccounting::default());
    assert!(view.turns.is_empty());
    f.state
        .connection
        .execute_batch("DROP TRIGGER reject_run_event")
        .unwrap();
    let permit = claim(&mut f.state);
    assert_eq!(permit.turn.ordinal, 1);
}
