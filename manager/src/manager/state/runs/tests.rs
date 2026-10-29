use super::*;
use crate::worker_http::VerifiedIdentity;
use std::collections::{BTreeMap, HashSet};

struct Fixture {
    _root: tempfile::TempDir,
    state: State,
    input: SubmitRun,
    lease: ConnectionLease,
    journal: JournalId,
    outbound: mpsc::Receiver<Envelope<ManagerMessage>>,
}
fn token() -> SessionToken {
    inference::tests::token()
}
fn fixture() -> Fixture {
    let (root, mut state, inference) = inference::tests::fixture();
    let peer = || VerifiedIdentity::for_test(b"M04 synthetic worker identity");
    let grant = state
        .invite_worker(token(), peer().fingerprint().clone(), 600)
        .unwrap();
    let worker = state.enroll_worker(peer(), grant.secret).unwrap();
    let session = state
        .open_computer_session(token(), inference.bot, worker)
        .unwrap();
    let (out, outbound) = mpsc::channel(32);
    let (stop, _) = watch::channel(false);
    let mut hello = Hello::current(false);
    hello
        .capabilities
        .extend([Capability::ProcessSystemdV1, Capability::ComputerV1]);
    let lease = state
        .connect_worker(peer(), hello, out, stop, RequestId::new())
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
    let input = SubmitRun {
        images: Vec::new(),
        id: RunId::new(),
        bot: inference.bot,
        session: Some(session.id),
        configuration_revision: 1,
        prompt: "Read a shared guest file and explain the result.".into(),
        limits: RunLimits {
            deadline: 1500,
            max_turns: 4,
            max_total_tokens: 200_000,
            max_cost_microusd: 200_000,
            max_output_tokens: 64,
            max_operations: 12,
            max_concurrent_operations: 1,
        },
        execution_policy: RunExecutionPolicy {
            connectors: None,
            vision: None,
            vibe: None,
            process: Some(ProcessToolPolicy {
                profile: ProcessProfile::LinuxSystemdV1,
                account: ProcessAccount {
                    uid: 1000,
                    gid: 1000,
                },
                environment: BTreeMap::from([("LANG".into(), "C.UTF-8".into())]),
                limits: ProcessLimits {
                    runtime_seconds: 20,
                    output_bytes: 1024,
                    processes: 8,
                    memory_bytes: 64 * 1024 * 1024,
                },
            }),
            computer: Some(ComputerToolPolicy {
                profile: ComputerProfile::LinuxPlaywrightV1,
                account: ProcessAccount {
                    uid: 1000,
                    gid: 1000,
                },
                runtime: ComputerRuntime {
                    sha256: CertificateFingerprint::from_der(b"fixture runtime"),
                },
                viewport: Viewport {
                    width: 800,
                    height: 600,
                },
                limits: ProcessLimits {
                    runtime_seconds: 30,
                    output_bytes: 1024,
                    processes: 64,
                    memory_bytes: 512 * 1024 * 1024,
                },
                outputs: vec![OutputDeclaration {
                    slot: "text".into(),
                    name: "shared.txt".into(),
                    media_type: MediaType::TextPlain,
                    max_bytes: 8192,
                }],
            }),
        },
    };
    Fixture {
        _root: root,
        state,
        input,
        lease,
        journal,
        outbound,
    }
}
fn claim(state: &mut State) -> RunCallPermit {
    match state
        .claim_run_work(&HashSet::new(), true)
        .unwrap()
        .unwrap()
    {
        RunJob::Call(permit) => *permit,
        _ => panic!("expected provider call"),
    }
}
fn response(call: Option<RunToolCall>) -> ModelOutcome {
    ModelOutcome::Billed {
        usage: InferenceUsage {
            prompt_tokens: 12,
            completion_tokens: 8,
            total_tokens: 20,
        },
        request_id: "recorded-response".into(),
        response: Ok(RunMessage::Assistant {
            content: "Recorded response".into(),
            tool_call: call,
        }),
        finish_reason: Some(InferenceFinishReason::Stop),
    }
}
fn process_call(id: &str) -> RunToolCall {
    RunToolCall {
        id: id.into(),
        proposal: ToolProposal::Process(ProcessProposal {
            executable: "/usr/bin/cat".into(),
            args: vec!["/srv/shared/outside-workspace.txt".into()],
            cwd: "/srv/shared".into(),
        }),
    }
}
impl Fixture {
    fn propose(&mut self) -> TaskView {
        self.state.submit_run(token(), self.input.clone()).unwrap();
        let permit = claim(&mut self.state);
        self.state
            .finish_run_call(
                self.input.id,
                permit.turn.id,
                permit.turn.attempt,
                response(Some(process_call("call00001"))),
            )
            .unwrap();
        let view = self.state.load_run(self.input.id).unwrap();
        self.state
            .load_task(view.turns[0].intent.as_ref().unwrap().task)
            .unwrap()
    }
    fn drain(&mut self) -> Vec<ManagerMessage> {
        let mut messages = Vec::new();
        while let Ok(message) = self.outbound.try_recv() {
            messages.push(message.message);
        }
        messages
    }
    fn approve_and_claim(&mut self, task: TaskId) -> ManagerMessage {
        self.drain();
        let task = self.state.grant_approval(token(), task, 1400).unwrap();
        let fence = self
            .state
            .find_computer_session(self.input.session.unwrap())
            .unwrap()
            .fence(self.lease.generation);
        self.state
            .claim(
                self.lease,
                self.journal,
                fence,
                task.approval.binding.unwrap(),
            )
            .unwrap();
        self.drain()
            .into_iter()
            .find(|m| matches!(m, ManagerMessage::Permit { .. }))
            .unwrap()
    }
    fn finish(&mut self, task: TaskId) {
        let task = self.state.load_task(task).unwrap();
        let scope = ProcessScope {
            journal: self.journal,
            execution: task.command.execution,
            host: CertificateFingerprint::from_der(b"fixture host"),
            boot: RequestId::new().to_string(),
            invocation: "1".repeat(32),
            unit: format!(
                "botstral-{}-{}.service",
                self.journal, task.command.execution
            ),
            cgroup: format!(
                "/fixture/botstral-{}-{}.service",
                self.journal, task.command.execution
            ),
        };
        self.state
            .durable_reports(
                self.lease,
                self.journal,
                2,
                vec![
                    DurableReport {
                        sequence: 1,
                        command: task.command.id,
                        execution: task.command.execution,
                        event: ExecutionEvent::ProcessResult {
                            evidence: ProcessEvidence {
                                effect: ProcessEffect::Observed,
                                result: Some(ProcessResult {
                                    exit: ProcessExit::Exited { code: 0 },
                                    stdout: CapturedOutput {
                                        hex: "68656c6c6f".into(),
                                        complete: true,
                                        truncated: false,
                                    },
                                    stderr: CapturedOutput {
                                        hex: String::new(),
                                        complete: true,
                                        truncated: false,
                                    },
                                }),
                            },
                        },
                    },
                    DurableReport {
                        sequence: 2,
                        command: task.command.id,
                        execution: task.command.execution,
                        event: ExecutionEvent::ProcessFinished {
                            success: true,
                            scope,
                            reason: ProcessCompletionReason::Result,
                        },
                    },
                ],
            )
            .unwrap();
    }
}

#[test]
fn run_intent_task_command_accounting_and_events_commit_together() {
    let mut f = fixture();
    f.state.submit_run(token(), f.input.clone()).unwrap();
    let permit = claim(&mut f.state);
    f.state.connection.execute_batch("CREATE TRIGGER reject_child BEFORE INSERT ON run_children BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert_eq!(
        f.state.finish_run_call(
            f.input.id,
            permit.turn.id,
            permit.turn.attempt,
            response(Some(process_call("call00001")))
        ),
        Err(OperationError::Internal)
    );
    for table in ["tasks", "commands", "run_children"] {
        assert_eq!(
            f.state
                .connection
                .query_row::<i64, _, _>(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap(),
            0
        );
    }
    assert_eq!(
        f.state
            .load_run(f.input.id)
            .unwrap()
            .summary
            .accounting
            .settled_tokens,
        0
    );
    f.state
        .connection
        .execute_batch("DROP TRIGGER reject_child")
        .unwrap();
    f.state
        .finish_run_call(
            f.input.id,
            permit.turn.id,
            permit.turn.attempt,
            response(Some(process_call("call00001"))),
        )
        .unwrap();
    let view = f.state.load_run(f.input.id).unwrap();
    assert_eq!(view.summary.accounting.settled_cost_microusd, 28);
    assert_eq!(view.summary.accounting.held_tokens, 0);
    assert_eq!(
        f.state.finish_run_call(
            f.input.id,
            permit.turn.id,
            permit.turn.attempt,
            response(Some(process_call("replacement")))
        ),
        Err(OperationError::Conflict)
    );
    assert_eq!(f.state.load_run(f.input.id).unwrap(), view);
    assert!(
        f.state
            .connection
            .execute("UPDATE run_turns SET input='[]'", [])
            .is_err()
    );
    assert!(
        f.state
            .connection
            .execute("UPDATE runs SET request='{}'", [])
            .is_err()
    );
    assert!(
        f.state
            .connection
            .execute("UPDATE run_children SET call_id='replacement'", [])
            .is_err()
    );
    let task = f
        .state
        .load_task(view.turns[0].intent.as_ref().unwrap().task)
        .unwrap();
    assert!(!task.approval.consumed && task.approval.binding.is_none());
    assert_eq!(
        task.command.work,
        view.turns[0].intent.as_ref().unwrap().work
    );
}

#[test]
fn cumulative_limits_stop_tools_after_settling_the_model_turn() {
    for reason in [
        RunStopReason::TurnLimit,
        RunStopReason::TokenLimit,
        RunStopReason::CostLimit,
        RunStopReason::OperationLimit,
    ] {
        let mut f = fixture();
        match reason {
            RunStopReason::TurnLimit => f.input.limits.max_turns = 1,
            RunStopReason::TokenLimit => f.input.limits.max_total_tokens = 32832,
            RunStopReason::CostLimit => f.input.limits.max_cost_microusd = 32896,
            RunStopReason::OperationLimit => f.input.limits.max_operations = 1,
            _ => unreachable!(),
        }
        f.state.submit_run(token(), f.input.clone()).unwrap();
        let permit = claim(&mut f.state);
        let call = if reason == RunStopReason::OperationLimit {
            RunToolCall {
                id: "call00001".into(),
                proposal: ToolProposal::Computer(ComputerProposal {
                    operations: vec![
                        ComputerOperation::Navigate {
                            url: "https://example.com".into(),
                        },
                        ComputerOperation::WriteFile {
                            path: "/srv/shared/a.txt".into(),
                            text: "a".into(),
                        },
                    ],
                    outputs: vec![],
                }),
            }
        } else {
            process_call("call00001")
        };
        f.state
            .finish_run_call(
                f.input.id,
                permit.turn.id,
                permit.turn.attempt,
                response(Some(call)),
            )
            .unwrap();
        let view = f.state.load_run(f.input.id).unwrap();
        assert_eq!(view.summary.phase, RunPhase::Stopped { reason });
        assert_eq!(view.summary.accounting.settled_tokens, 20);
        assert_eq!(view.summary.accounting.settled_cost_microusd, 28);
        assert!(view.turns[0].intent.is_none());
        assert!(
            f.state
                .claim_run_work(&HashSet::new(), true)
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn child_claim_and_publication_recheck_parent_without_consuming_ineligible_approval() {
    for action in [
        "cancel",
        "deadline",
        "revoke",
        "permission",
        "expiry",
        "arguments",
    ] {
        let mut f = fixture();
        let task = f.propose();
        let approved = f.state.grant_approval(token(), task.id, 1400).unwrap();
        let mut binding = approved.approval.binding.unwrap();
        match action {
            "cancel" => {
                f.state.cancel_run(token(), f.input.id).unwrap();
            }
            "deadline" => f.state.test_time = Some(1500),
            "revoke" => {
                f.state.revoke_approval(token(), task.id).unwrap();
            }
            "permission" => {
                f.state
                    .set_permission(token(), f.input.bot, 1, false)
                    .unwrap();
            }
            "expiry" => f.state.test_time = Some(1400),
            "arguments" => {
                let mut changed = task.command.clone();
                changed.work = WorkSpec::Unavailable;
                binding.arguments = changed.argument_digest();
            }
            _ => unreachable!(),
        }
        f.drain();
        let fence = f
            .state
            .find_computer_session(f.input.session.unwrap())
            .unwrap()
            .fence(f.lease.generation);
        f.state.claim(f.lease, f.journal, fence, binding).unwrap();
        assert!(
            !f.state.load_task(task.id).unwrap().approval.consumed,
            "{action}"
        );
        assert!(
            f.drain()
                .iter()
                .any(|m| matches!(m, ManagerMessage::ClaimDenied { .. })),
            "{action}"
        );
    }
    for cancel in [true, false] {
        let mut f = fixture();
        let task = f.propose();
        let permit = f.approve_and_claim(task.id);
        if cancel {
            f.state.cancel_run(token(), f.input.id).unwrap();
        } else {
            f.state.test_time = Some(1500);
        }
        assert!(f.state.admit_delivery(f.lease, &permit).is_err());
    }
}

#[test]
fn results_are_durable_and_reconciled_without_replacement_effects() {
    let mut f = fixture();
    let task = f.propose();
    f.state.recover_runs(false).unwrap();
    assert_eq!(
        f.state.load_run(f.input.id).unwrap().turns[0]
            .intent
            .as_ref()
            .unwrap()
            .task,
        task.id
    );
    f.approve_and_claim(task.id);
    f.finish(task.id);
    let collection = match f
        .state
        .claim_run_work(&HashSet::new(), true)
        .unwrap()
        .unwrap()
    {
        RunJob::Collect(c) => c,
        _ => panic!(),
    };
    let turn = collection.turn;
    let snapshot = collection.task.clone();
    let result = crate::manager::runs::results::collect(*collection).unwrap();
    assert!(result.contains("hello"));
    f.state
        .finish_run_result(f.input.id, turn, snapshot.clone(), Ok(result.clone()))
        .unwrap();
    f.state.recover_runs(false).unwrap();
    f.state
        .finish_run_result(f.input.id, turn, snapshot, Ok("replacement".into()))
        .unwrap();
    let next = claim(&mut f.state);
    assert_eq!(next.turn.messages.len(), 3);
    assert_eq!(
        next.turn.messages[2],
        RunMessage::Tool {
            tool_call_id: "call00001".into(),
            content: result
        }
    );
    f.state
        .finish_run_call(f.input.id, next.turn.id, next.turn.attempt, response(None))
        .unwrap();
    let view = f.state.load_run(f.input.id).unwrap();
    assert_eq!(view.summary.phase, RunPhase::Completed);
    assert_eq!(view.summary.accounting.settled_tokens, 40);
    assert_eq!(view.summary.accounting.admitted_turns, 2);
    assert_eq!(view.turns[0].intent.as_ref().unwrap().task, task.id);
}

#[test]
fn provider_uncertainty_retains_reservation_and_late_cancelled_usage_settles_once() {
    let mut f = fixture();
    f.state.submit_run(token(), f.input.clone()).unwrap();
    let permit = claim(&mut f.state);
    f.state.cancel_run(token(), f.input.id).unwrap();
    f.state
        .finish_run_call(
            f.input.id,
            permit.turn.id,
            permit.turn.attempt,
            response(Some(process_call("call00001"))),
        )
        .unwrap();
    let view = f.state.load_run(f.input.id).unwrap();
    assert_eq!(view.summary.phase, RunPhase::Cancelled);
    assert_eq!(view.summary.accounting.settled_tokens, 20);
    assert!(view.turns[0].intent.is_none());
    let mut f = fixture();
    f.state.submit_run(token(), f.input.clone()).unwrap();
    let permit = claim(&mut f.state);
    f.state.recover_runs(false).unwrap();
    let view = f.state.load_run(f.input.id).unwrap();
    assert!(matches!(
        view.summary.phase,
        RunPhase::AwaitingAttention {
            reason: RunAttentionReason::Restart
        }
    ));
    assert_eq!(view.summary.accounting.held_tokens, 32832);
    f.state
        .finish_run_call(
            f.input.id,
            permit.turn.id,
            permit.turn.attempt,
            response(None),
        )
        .unwrap();
    assert_eq!(f.state.load_run(f.input.id).unwrap(), view);
    assert!(
        f.state
            .claim_run_work(&HashSet::new(), true)
            .unwrap()
            .is_none()
    );
}

#[test]
fn registry_preserves_owner_policy_and_rejects_persistent_tab_assumptions() {
    let f = fixture();
    let work =
        tools::translate(&f.input.execution_policy, &process_call("a").proposal, 1500).unwrap();
    let WorkSpec::Process { spec } = work else {
        panic!()
    };
    assert_eq!(spec.account.uid, 1000);
    assert_eq!(spec.environment["LANG"], "C.UTF-8");
    assert_eq!(spec.args, ["/srv/shared/outside-workspace.txt"]);
    let proposal = ToolProposal::Computer(ComputerProposal {
        operations: vec![ComputerOperation::Click {
            selector: "button".into(),
        }],
        outputs: vec![],
    });
    assert!(tools::translate(&f.input.execution_policy, &proposal, 1500).is_err());
    assert!(
        tools::decode(
            "process",
            r#"{"executable":"/bin/sh","args":[],"cwd":"/","account":{"uid":0,"gid":0}}"#
        )
        .is_err()
    );
    assert!(tools::decode("unknown", "{}").is_err());
}

#[path = "artifact_tests.rs"]
mod artifact_tests;
#[path = "coding_tests.rs"]
mod coding_tests;
#[path = "recovery_tests.rs"]
mod recovery_tests;
#[path = "restart_tests.rs"]
mod restart_tests;
#[path = "routine_tests.rs"]
mod routine_tests;
#[path = "vision_tests.rs"]
mod vision_tests;

#[path = "memory_tests.rs"]
mod memory_tests;

#[path = "tests/publication_tests.rs"]
mod publication_tests;
