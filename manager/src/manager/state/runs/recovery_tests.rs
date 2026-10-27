use super::*;

#[test]
fn cancellation_requires_stop_evidence_and_reconciliation_is_idempotent() {
    let mut f = fixture();
    let task = f.propose();
    f.approve_and_claim(task.id);
    f.state.cancel_run(token(), f.input.id).unwrap();
    let before = f.state.change_sequence();
    for _ in 0..3 {
        assert!(
            f.state
                .claim_run_work(&HashSet::new(), true)
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(
        f.state.change_sequence(),
        before,
        "waiting cancellation must not write events repeatedly"
    );
    assert_eq!(
        f.state.load_run(f.input.id).unwrap().summary.phase,
        RunPhase::Cancelling
    );
    f.state
        .durable_reports(
            f.lease,
            f.journal,
            1,
            vec![DurableReport {
                sequence: 1,
                command: task.command.id,
                execution: task.command.execution,
                event: ExecutionEvent::Uncertain {
                    reason: "worker stop not proved".into(),
                },
            }],
        )
        .unwrap();
    assert!(
        f.state
            .claim_run_work(&HashSet::new(), true)
            .unwrap()
            .is_none()
    );
    // Cancelling tasks retain their state until a stop report; an uncertain report is still evidence.
    assert_ne!(
        f.state.load_run(f.input.id).unwrap().summary.phase,
        RunPhase::Cancelled
    );
    let scope = ProcessScope {
        journal: f.journal,
        execution: task.command.execution,
        host: CertificateFingerprint::from_der(b"fixture host"),
        boot: RequestId::new().to_string(),
        invocation: "1".repeat(32),
        unit: format!("botstral-{}-{}.service", f.journal, task.command.execution),
        cgroup: format!(
            "/fixture/botstral-{}-{}.service",
            f.journal, task.command.execution
        ),
    };
    f.state
        .durable_reports(
            f.lease,
            f.journal,
            2,
            vec![DurableReport {
                sequence: 2,
                command: task.command.id,
                execution: task.command.execution,
                event: ExecutionEvent::Stopped {
                    applied: 0,
                    evidence: StopEvidence::SystemdScopeEmpty { scope },
                },
            }],
        )
        .unwrap();
    assert!(
        f.state
            .claim_run_work(&HashSet::new(), true)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        f.state.load_run(f.input.id).unwrap().summary.phase,
        RunPhase::Cancelled
    );
    assert!(
        !f.state
            .load_task(task.id)
            .unwrap()
            .effect_evidence
            .is_empty()
    );
}

#[test]
fn revised_authority_is_checked_at_permit_publication() {
    for change in ["permission", "revoke", "expiry"] {
        let mut f = fixture();
        let task = f.propose();
        let permit = f.approve_and_claim(task.id);
        match change {
            "permission" => {
                f.state
                    .set_permission(token(), f.input.bot, 1, true)
                    .unwrap();
            }
            "revoke" => {
                f.state.revoke_approval(token(), task.id).unwrap();
            }
            "expiry" => f.state.test_time = Some(1400),
            _ => unreachable!(),
        }
        assert!(
            f.state.admit_delivery(f.lease, &permit).is_err(),
            "{change}"
        );
    }
}

#[test]
fn duplicate_call_ids_and_new_call_authority_cannot_create_replacement_work() {
    for change in ["duplicate", "configuration", "vault", "key", "deadline"] {
        let mut f = fixture();
        let task = f.propose();
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
        f.state
            .finish_run_result(f.input.id, permit.turn, permit.task, Ok("result".into()))
            .unwrap();
        match change {
            "configuration" => {
                let config = f.state.load_run(f.input.id).unwrap().configuration;
                f.state
                    .configure_inference(
                        token(),
                        ConfigureInference {
                            bot: f.input.bot,
                            expected_revision: 1,
                            policy: config.policy,
                        },
                    )
                    .unwrap();
            }
            "vault" => {
                f.state.lock_vault(token()).unwrap();
            }
            "key" => {
                f.state
                    .replace_provider_key(
                        token(),
                        ProviderSecret::new("replacement fixture key".into()).unwrap(),
                    )
                    .unwrap();
            }
            "deadline" => f.state.test_time = Some(1500),
            "duplicate" => {
                let permit = claim(&mut f.state);
                f.state
                    .finish_run_call(
                        f.input.id,
                        permit.turn.id,
                        permit.turn.attempt,
                        response(Some(process_call("call00001"))),
                    )
                    .unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            f.state
                .claim_run_work(&HashSet::new(), true)
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            f.state.load_run(f.input.id).unwrap().summary.phase,
            RunPhase::Stopped { .. }
        ));
        assert_eq!(
            f.state
                .connection
                .query_row::<i64, _, _>("SELECT count(*) FROM tasks", [], |r| r.get(0))
                .unwrap(),
            1
        );
    }
}
