use super::*;
use crate::worker_http::VerifiedIdentity;
use serde_json::json;

struct Fixture {
    _root: tempfile::TempDir,
    state: State,
    bot: BotId,
    session: ComputerSessionId,
    lease: ConnectionLease,
    journal: JournalId,
    outbound: mpsc::Receiver<Envelope<ManagerMessage>>,
}
fn token() -> SessionToken {
    inference::tests::token()
}
fn peer() -> VerifiedIdentity {
    VerifiedIdentity::for_test(b"M06 connector worker")
}
fn runner() -> ConnectorRunner {
    ConnectorRunner {
        runtime: CertificateFingerprint::from_der(b"pinned connector"),
        account: ProcessAccount {
            uid: 1000,
            gid: 1000,
        },
        limits: ProcessLimits {
            runtime_seconds: 60,
            output_bytes: 1024,
            processes: 8,
            memory_bytes: 64 * 1024 * 1024,
        },
    }
}
fn fixture() -> Fixture {
    let (root, mut state, input) = inference::tests::fixture();
    let invitation = state
        .invite_worker(token(), peer().fingerprint().clone(), 600)
        .unwrap();
    let worker = state.enroll_worker(peer(), invitation.secret).unwrap();
    let session = state
        .open_computer_session(token(), input.bot, worker)
        .unwrap()
        .id;
    let (out, outbound) = mpsc::channel(32);
    let (stop, _) = watch::channel(false);
    let mut hello = Hello::current(false);
    hello.capabilities.extend([
        Capability::ProcessSystemdV1,
        Capability::ConnectorV1,
        Capability::VibeV1,
    ]);
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
    Fixture {
        _root: root,
        state,
        bot: input.bot,
        session,
        lease,
        journal,
        outbound,
    }
}
fn tool(name: &str) -> McpTool {
    McpTool {
        name: name.into(),
        description: "Synthetic tool".into(),
        input_schema: json!({"type":"object","properties":{"value":{"type":"string"}},"required":["value"],"additionalProperties":false}),
    }
}
fn outcome() -> ConnectorOutcome {
    ConnectorOutcome::Finished {
        result: ConnectorResult::Tool {
            value: json!({"content":[{"type":"text","text":"recorded result"}]}),
        },
    }
}
impl Fixture {
    fn install(&mut self, local: bool) -> ConnectionView {
        self.state
            .install_connection(
                token(),
                InstallConnection {
                    id: ConnectionId::new(),
                    label: "Synthetic account".into(),
                    transport: if local {
                        ConnectionTransport::WorkerStdio {
                            worker: self.lease.worker,
                            account: runner().account,
                            executable: "/usr/bin/python3".into(),
                            arguments: vec!["/srv/fixture.py".into()],
                            cwd: "/srv".into(),
                            environment: Default::default(),
                        }
                    } else {
                        ConnectionTransport::RemoteHttp {
                            endpoint: "https://example.com/mcp".into(),
                        }
                    },
                },
            )
            .unwrap()
    }
    fn approve(&mut self, id: TaskId) -> TaskView {
        while self.outbound.try_recv().is_ok() {}
        let task = self.state.grant_approval(token(), id, 1400).unwrap();
        let fence = self
            .state
            .find_computer_session(task.command.session)
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
        let mut permit = None;
        while let Ok(message) = self.outbound.try_recv() {
            if matches!(message.message, ManagerMessage::Permit { .. }) {
                permit = Some(message.message);
            }
        }
        self.state
            .admit_delivery(self.lease, &permit.expect("normal task permit"))
            .unwrap();
        self.state.load_task(id).unwrap()
    }
    fn task(&mut self, operation: ConnectorOperation) -> TaskView {
        let input = SubmitTask {
            id: TaskId::new(),
            session_id: self.session,
            conversation_id: None,
            settings: TaskSettings {
                model: "connector".into(),
                max_turns: 1,
                max_tokens: 1,
                deadline: 1500,
                max_cost_micros: 0,
            },
            work: WorkSpec::Connector {
                spec: Box::new(ConnectorSpec {
                    operation,
                    runner: runner(),
                    deadline: 1500,
                    result: connector_output(8192),
                }),
            },
        };
        self.state.submit_task(token(), input).unwrap()
    }
    fn reserve(&mut self, task: &TaskView, id: ConnectorCallId) -> ConnectorAdmission {
        let WorkSpec::Connector { spec } = &task.command.work else {
            panic!()
        };
        self.state
            .admit_connector(
                peer(),
                task.id,
                task.command.execution,
                id,
                spec.operation.clone(),
            )
            .unwrap()
    }
    fn ready(&mut self, local: bool) -> ConnectionView {
        let connection = self.install(local);
        let task = self.task(ConnectorOperation::Discover {
            connection: connection.version,
        });
        let task = self.approve(task.id);
        let id = ConnectorCallId::new();
        assert!(matches!(
            self.reserve(&task, id),
            ConnectorAdmission::Reserved(_)
        ));
        assert!(self.state.dispatch_connector(id).unwrap().is_some());
        self.state
            .finish_connector(
                id,
                ConnectorOutcome::Finished {
                    result: ConnectorResult::Catalog {
                        tools: vec![tool("read"), tool("write")],
                    },
                },
            )
            .unwrap();
        self.finish_task(task.id);
        self.state.load_connection(connection.id).unwrap()
    }
    fn finish_task(&mut self, id: TaskId) {
        let mut task = self.state.load_task(id).unwrap();
        task.state = TaskState::Completed;
        tasks::save(&self.state.connection, &task).unwrap();
    }
    fn grant(
        &mut self,
        connection: &ConnectionView,
        bot: BotId,
        names: &[&str],
    ) -> BotConnectorGrants {
        let old = self.state.load_grants(bot, connection.id).unwrap();
        self.state
            .set_connector_grants(
                token(),
                bot,
                SetConnectorGrants {
                    connection: connection.version.clone(),
                    catalog_revision: connection.catalog_revision,
                    expected_revision: old.revision,
                    tools: names.iter().map(|s| (*s).into()).collect(),
                },
            )
            .unwrap()
    }
    fn call(&mut self, connection: &ConnectionView) -> TaskView {
        let binding = self.grant(connection, self.bot, &["read"]).tools.remove(0);
        let task = self.task(ConnectorOperation::Call {
            proposal: ConnectorProposal {
                tool: binding,
                arguments: json!({"value":"input"}),
            },
        });
        self.approve(task.id)
    }
}
#[test]
fn owner_installation_does_not_grant_tools_and_bots_have_independent_revisions() {
    let mut f = fixture();
    let c = f.ready(false);
    assert!(f.state.connector_tools(token(), f.bot).unwrap().is_empty());
    let other = BotId::new();
    f.state
        .create_bot(
            token(),
            NewBot {
                id: other,
                name: BotName::new("Other".into()).unwrap(),
            },
        )
        .unwrap();
    let first = f.grant(&c, f.bot, &["read"]);
    let second = f.grant(&c, other, &["write"]);
    assert_eq!(
        f.state.connector_tools(token(), f.bot).unwrap()[0]
            .binding
            .name,
        "read"
    );
    assert_eq!(
        f.state.connector_tools(token(), other).unwrap()[0]
            .binding
            .name,
        "write"
    );
    f.grant(&c, f.bot, &[]);
    assert!(
        f.state
            .validate_connector_binding(f.bot, &first.tools[0])
            .is_err()
    );
    assert!(
        f.state
            .validate_connector_binding(other, &second.tools[0])
            .is_ok()
    );
    f.grant(&c, f.bot, &["read"]);
    assert!(
        f.state
            .validate_connector_binding(f.bot, &first.tools[0])
            .is_err()
    );
}
#[test]
fn revoke_and_regrant_cannot_revive_an_approved_task_or_release() {
    let mut f = fixture();
    let c = f.ready(true);
    let task = f.call(&c);
    let id = ConnectorCallId::new();
    f.reserve(&task, id);
    f.grant(&c, f.bot, &[]);
    f.grant(&c, f.bot, &["read"]);
    assert!(f.state.dispatch_connector(id).unwrap().is_none());
    assert!(
        f.state
            .validate_connector_work(f.bot, f.lease.worker, &task.command.work)
            .is_err()
    );
    assert!(matches!(
        f.state.load_connector_call(id).unwrap().state,
        ConnectorCallState::Resolved {
            outcome: ConnectorOutcome::NotSent { .. }
        }
    ));
}
#[test]
fn local_permit_is_one_shot_including_lost_reply_and_duplicate_settlement() {
    let mut f = fixture();
    let c = f.ready(true);
    let task = f.call(&c);
    let id = ConnectorCallId::new();
    f.reserve(&task, id);
    assert!(f.state.dispatch_connector(id).unwrap().is_some());
    assert!(matches!(
        f.reserve(&task, id),
        ConnectorAdmission::Existing(_)
    ));
    assert!(f.state.dispatch_connector(id).unwrap().is_none());
    let first = f
        .state
        .settle_local_connector(peer(), task.id, task.command.execution, id, outcome())
        .unwrap();
    let second = f
        .state
        .settle_local_connector(peer(), task.id, task.command.execution, id, outcome())
        .unwrap();
    assert_eq!(first, second);
    assert!(f.state.dispatch_connector(id).unwrap().is_none());
}
#[test]
fn restart_retains_possible_effects_and_late_results_do_not_reopen_authority() {
    let mut f = fixture();
    let c = f.ready(true);
    let task = f.call(&c);
    let id = ConnectorCallId::new();
    f.reserve(&task, id);
    f.state.dispatch_connector(id).unwrap().unwrap();
    f.state.recover_connectors().unwrap();
    assert!(gateway::unknown(&f.state.load_connector_call(id).unwrap()));
    let late = f
        .state
        .settle_local_connector(peer(), task.id, task.command.execution, id, outcome())
        .unwrap();
    assert!(gateway::unknown(&late));
    assert!(late.late_result.is_some());
    assert!(f.state.dispatch_connector(id).unwrap().is_none());
    assert_eq!(
        f.state.load_task(task.id).unwrap().state,
        TaskState::Cancelling
    );
}

fn stop_reports(f: &Fixture, task: &TaskView) -> Vec<DurableReport> {
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
    vec![
        DurableReport {
            sequence: 1,
            command: task.command.id,
            execution: task.command.execution,
            event: ExecutionEvent::Uncertain {
                reason: "process_interrupted".into(),
            },
        },
        DurableReport {
            sequence: 2,
            command: task.command.id,
            execution: task.command.execution,
            event: ExecutionEvent::Stopped {
                applied: 0,
                evidence: StopEvidence::SystemdScopeEmpty { scope },
            },
        },
    ]
}

fn assert_unknown_connector_stop(unknown_before_stop: bool) {
    let mut f = fixture();
    let c = f.ready(true);
    let task = f.call(&c);
    let id = ConnectorCallId::new();
    f.reserve(&task, id);
    assert!(f.state.dispatch_connector(id).unwrap().is_some());
    if unknown_before_stop {
        f.state.recover_connectors().unwrap();
    } else {
        f.state.cancel_task(token(), task.id).unwrap();
    }
    let reports = stop_reports(&f, &task);
    f.state
        .durable_reports(f.lease, f.journal, 1, vec![reports[0].clone()])
        .unwrap();
    assert_eq!(
        f.state.load_task(task.id).unwrap().state,
        TaskState::Cancelling
    );
    assert!(
        std::iter::from_fn(|| f.outbound.try_recv().ok()).any(|message| {
            matches!(message.message, ManagerMessage::Delivery(delivery)
            if delivery.body.task == task.id && delivery.cancel)
        })
    );
    f.state
        .durable_reports(f.lease, f.journal, 2, vec![reports[1].clone()])
        .unwrap();
    if !unknown_before_stop {
        assert_eq!(
            f.state.load_task(task.id).unwrap().state,
            TaskState::Cancelled
        );
        f.state.recover_connectors().unwrap();
    }
    let stopped = f.state.load_task(task.id).unwrap();
    assert_eq!(stopped.state, TaskState::AwaitingAttention);
    assert_eq!(stopped.outcome, Some(reports[1].event.clone()));
    assert_eq!(stopped.effect_evidence, vec![reports[0].event.clone()]);
    assert!(stopped.approval.consumed);
    assert_eq!(stopped.approval, task.approval);
    assert!(f.state.grant_approval(token(), task.id, 1400).is_err());
    assert!(f.state.dispatch_connector(id).unwrap().is_none());
    let late = f
        .state
        .settle_local_connector(peer(), task.id, task.command.execution, id, outcome())
        .unwrap();
    assert!(gateway::unknown(&late));
    assert!(late.late_result.is_some());
    f.state
        .durable_reports(f.lease, f.journal, 2, reports.clone())
        .unwrap();
    assert_eq!(f.state.load_task(task.id).unwrap(), stopped);
    let mut repeated = reports;
    for report in &mut repeated {
        report.sequence += 2;
        f.state
            .durable_reports(f.lease, f.journal, report.sequence, vec![report.clone()])
            .unwrap();
        assert_eq!(f.state.load_task(task.id).unwrap(), stopped);
    }
}

#[test]
fn unknown_connector_before_stop_requires_attention() {
    assert_unknown_connector_stop(true);
}

#[test]
fn unknown_connector_after_stop_requires_attention() {
    assert_unknown_connector_stop(false);
}

#[test]
fn reopening_cancelled_unknown_connector_requires_attention() {
    use std::{fs, os::unix::fs::PermissionsExt};
    let mut f = fixture();
    let c = f.ready(true);
    let task = f.call(&c);
    let id = ConnectorCallId::new();
    f.reserve(&task, id);
    assert!(f.state.dispatch_connector(id).unwrap().is_some());
    f.state.recover_connectors().unwrap();
    let reports = stop_reports(&f, &task);
    f.state
        .durable_reports(f.lease, f.journal, 2, reports)
        .unwrap();
    let mut legacy = f.state.load_task(task.id).unwrap();
    legacy.state = TaskState::Cancelled;
    tasks::save(&f.state.connection, &legacy).unwrap();
    let path = f.state.directory.join("manager.sqlite3");
    f.state
        .connection
        .execute("VACUUM INTO ?1", [path.to_str().unwrap()])
        .unwrap();
    fs::set_permissions(
        f.state.directory.join("memory-index.sqlite3"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let (configured, _) = watch::channel(false);
    let mut reopened = State::open(f.state.directory.clone(), configured).unwrap();
    let repaired = reopened.load_task(task.id).unwrap();
    assert_eq!(repaired.state, TaskState::AwaitingAttention);
    legacy.state = TaskState::AwaitingAttention;
    assert_eq!(repaired, legacy);
    assert!(gateway::unknown(&reopened.load_connector_call(id).unwrap()));
    assert!(reopened.dispatch_connector(id).unwrap().is_none());
    assert!(reopened.grant_approval(token(), task.id, 1400).is_err());
    let sequence = reopened.change_sequence();
    reopened.recover_connectors().unwrap();
    assert_eq!(reopened.change_sequence(), sequence);
    assert_eq!(reopened.load_task(task.id).unwrap(), repaired);
}

#[test]
fn confirmed_connector_cancellation_remains_cancelled() {
    for dispatched in [false, true] {
        let mut f = fixture();
        let c = f.ready(true);
        let task = f.call(&c);
        let id = ConnectorCallId::new();
        f.reserve(&task, id);
        if dispatched {
            assert!(f.state.dispatch_connector(id).unwrap().is_some());
            f.state.finish_connector(id, outcome()).unwrap();
        } else {
            f.state.recover_connectors().unwrap();
        }
        f.state.cancel_task(token(), task.id).unwrap();
        let reports = stop_reports(&f, &task);
        f.state
            .durable_reports(f.lease, f.journal, 2, reports.clone())
            .unwrap();
        let stopped = f.state.load_task(task.id).unwrap();
        assert_eq!(stopped.state, TaskState::Cancelled);
        assert_eq!(stopped.outcome, Some(reports[1].event.clone()));
        assert_eq!(stopped.effect_evidence, vec![reports[0].event.clone()]);
        assert!(stopped.approval.consumed);
        assert!(!gateway::unknown(&f.state.load_connector_call(id).unwrap()));
        assert!(f.state.dispatch_connector(id).unwrap().is_none());
    }
}

#[test]
fn in_flight_revoke_keeps_unknown_and_audits_late_evidence() {
    let mut f = fixture();
    let c = f.ready(false);
    let task = f.call(&c);
    let id = ConnectorCallId::new();
    f.reserve(&task, id);
    f.state.dispatch_connector(id).unwrap().unwrap();
    f.state
        .change_connection(token(), c.id, c.version.revision, true)
        .unwrap();
    let call = f.state.load_connector_call(id).unwrap();
    assert!(gateway::unknown(&call));
    assert_eq!(call.revoked_during_call_at, Some(1000));
    f.state.finish_connector(id, outcome()).unwrap();
    assert!(f.state.connector_tools(token(), f.bot).unwrap().is_empty());
    assert!(
        f.state
            .connector_audit(token(), c.id, 0)
            .unwrap()
            .iter()
            .any(|a| a.code == "late_evidence")
    );
}
#[test]
fn rotation_and_ciphertext_binding_keep_secrets_out_of_views_audit_and_results() {
    let mut f = fixture();
    let c = f.ready(false);
    let old = f.grant(&c, f.bot, &["read"]);
    let secret = "synthetic-connector-bearer";
    let c = f
        .state
        .replace_connection_secret(
            token(),
            c.id,
            ReplaceConnectionSecret {
                expected_revision: c.version.revision,
                secret: ConnectionSecretInput::Bearer {
                    value: ConnectorSecretEntry(secret.into()),
                },
            },
        )
        .unwrap();
    assert_eq!(c.version.credential_epoch, 2);
    assert!(
        f.state
            .validate_connector_binding(f.bot, &old.tools[0])
            .is_err()
    );
    let other = f.install(false);
    let ciphertext: Vec<u8> = f
        .state
        .connection
        .query_row(
            "SELECT ciphertext FROM connection_secrets WHERE connection=?1 AND epoch=2",
            [c.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        crypto::decrypt_connector(
            f.state.connector_identity().unwrap(),
            &context(&other),
            &ciphertext
        )
        .is_err()
    );
    assert!(
        f.state
            .connection
            .execute(
                "UPDATE connection_secrets SET ciphertext=?1 WHERE connection=?2",
                params![ciphertext, other.id.to_string()]
            )
            .is_err()
    );
    let task = f.task(ConnectorOperation::Discover {
        connection: c.version.clone(),
    });
    let task = f.approve(task.id);
    let id = ConnectorCallId::new();
    f.reserve(&task, id);
    let dispatch = f.state.dispatch_connector(id).unwrap().unwrap();
    assert!(matches!(
        dispatch.secret,
        ConnectionSecretInput::Bearer { .. }
    ));
    let mut t = tool("echo");
    t.description = secret.into();
    let view = f
        .state
        .finish_connector(
            id,
            ConnectorOutcome::Finished {
                result: ConnectorResult::Catalog { tools: vec![t] },
            },
        )
        .unwrap();
    assert!(!json(&view).unwrap().contains(secret));
    assert!(
        !json(&f.state.connections(token()).unwrap())
            .unwrap()
            .contains(secret)
    );
    assert!(
        !json(&f.state.connector_audit(token(), c.id, 0).unwrap())
            .unwrap()
            .contains(secret)
    );
    let ciphertext: Vec<u8> = f
        .state
        .connection
        .query_row(
            "SELECT ciphertext FROM connection_secrets WHERE connection=?1 AND epoch=2",
            [c.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        !ciphertext
            .windows(secret.len())
            .any(|b| b == secret.as_bytes())
    );
}
#[test]
fn every_release_rechecks_permission_fence_approval_and_worker_generation() {
    for cause in ["permission", "fence", "approval", "worker", "deadline"] {
        let mut f = fixture();
        let c = f.ready(true);
        let task = f.call(&c);
        let id = ConnectorCallId::new();
        f.reserve(&task, id);
        match cause {
            "permission" => {
                f.state.set_permission(token(), f.bot, 1, false).unwrap();
            }
            "fence" => {
                let s = f.state.find_computer_session(f.session).unwrap();
                f.state
                    .set_input_owner(token(), f.session, s.revision, InputOwner::Owner)
                    .unwrap();
            }
            "approval" => {
                f.state.revoke_approval(token(), task.id).unwrap();
            }
            "worker" => {
                f.state
                    .worker_connections
                    .get_mut(&f.lease.worker)
                    .unwrap()
                    .generation = f.lease.generation.advance().unwrap();
            }
            "deadline" => {
                f.state.test_time = Some(1501);
            }
            _ => unreachable!(),
        }
        assert!(f.state.dispatch_connector(id).unwrap().is_none(), "{cause}");
    }
}
#[test]
fn discovery_arguments_and_schema_are_bounded_and_denials_are_audited() {
    let mut f = fixture();
    let c = f.ready(false);
    let task = f.call(&c);
    let id = ConnectorCallId::new();
    let WorkSpec::Connector { spec } = task.command.work.clone() else {
        panic!()
    };
    let ConnectorOperation::Call { mut proposal } = spec.operation else {
        panic!()
    };
    proposal.arguments = json!({"unexpected":true});
    assert!(
        f.state
            .admit_connector(
                peer(),
                task.id,
                task.command.execution,
                id,
                ConnectorOperation::Call { proposal }
            )
            .is_err()
    );
    assert!(
        f.state
            .connector_audit(token(), c.id, 0)
            .unwrap()
            .iter()
            .any(|a| a.code == "admission_denied")
    );
    let mut external = tool("bad");
    external.input_schema = json!({"type":"object","$ref":"https://example.com/schema"});
    assert!(validate_catalog(&[external]).is_err());
}
#[test]
fn possible_marker_and_audit_commit_atomically_before_any_permit() {
    let mut f = fixture();
    let c = f.ready(true);
    let task = f.call(&c);
    let id = ConnectorCallId::new();
    f.reserve(&task, id);
    f.state.connection.execute_batch("CREATE TRIGGER reject_connector_marker BEFORE INSERT ON connector_audit WHEN NEW.code='dispatch_possible' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(f.state.dispatch_connector(id).is_err());
    assert_eq!(
        f.state.load_connector_call(id).unwrap().state,
        ConnectorCallState::Reserved
    );
    f.state
        .connection
        .execute_batch("DROP TRIGGER reject_connector_marker")
        .unwrap();
    assert!(f.state.dispatch_connector(id).unwrap().is_some());
}

mod runs;

#[test]
fn reopening_sqlite_preserves_effect_identity_and_never_releases_again() {
    use std::{fs, os::unix::fs::PermissionsExt};
    for phase in ["reserved", "possible", "finished"] {
        let mut f = fixture();
        let c = f.ready(true);
        let task = f.call(&c);
        let id = ConnectorCallId::new();
        f.reserve(&task, id);
        if phase != "reserved" {
            assert!(f.state.dispatch_connector(id).unwrap().is_some());
        }
        if phase == "finished" {
            f.state.finish_connector(id, outcome()).unwrap();
        }
        let prior = f.state.load_connector_call(id).unwrap();
        let path = f.state.directory.join("manager.sqlite3");
        f.state
            .connection
            .execute("VACUUM INTO ?1", [path.to_str().unwrap()])
            .unwrap();
        fs::set_permissions(
            f.state.directory.join("memory-index.sqlite3"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        let (configured, _) = watch::channel(false);
        let mut reopened = State::open(f.state.directory.clone(), configured).unwrap();
        let after = reopened.load_connector_call(id).unwrap();
        assert_eq!(after.id, prior.id);
        assert_eq!(after.execution, prior.execution);
        assert_eq!(after.tool, prior.tool);
        assert_eq!(reopened.load_connection(c.id).unwrap(), c);
        assert_eq!(
            reopened.load_grants(f.bot, c.id).unwrap(),
            f.state.load_grants(f.bot, c.id).unwrap()
        );
        match phase {
            "reserved" => assert!(matches!(
                after.state,
                ConnectorCallState::Resolved {
                    outcome: ConnectorOutcome::NotSent { .. }
                }
            )),
            "possible" => assert!(matches!(
                after.state,
                ConnectorCallState::Resolved {
                    outcome: ConnectorOutcome::Unknown { .. }
                }
            )),
            "finished" => assert_eq!(after, prior),
            _ => unreachable!(),
        }
        assert!(reopened.dispatch_connector(id).unwrap().is_none());
    }
}

#[test]
fn vibe_catalog_coding_requests_and_cached_calls_share_current_grants() {
    use crate::manager::coding::CodingRequest;
    let mut f = fixture();
    let c = f.ready(true);
    let binding = f.grant(&c, f.bot, &["read"]).tools.remove(0);
    let model = f
        .state
        .load_inference_configuration(f.bot, 1)
        .unwrap()
        .policy
        .model_id;
    let work = WorkSpec::Vibe {
        spec: Box::new(VibeSpec {
            policy: VibePolicy {
                workspace: CodingWorkspace {
                    id: CodingWorkspaceId::new(),
                    path: "/srv/coding/m06".into(),
                },
                runtime: CertificateFingerprint::from_der(b"pinned Vibe"),
                account: runner().account,
                resources: runner().limits,
                configuration_revision: 1,
                model_id: model.clone(),
                access: VibeAccess::EditFilesAsGuestAccount,
                outputs: vec![],
                connectors: Some(ConnectorToolPolicy {
                    tools: vec![binding.clone()],
                    runner: runner(),
                    max_calls: 2,
                    max_result_bytes: 4096,
                }),
            },
            session: VibeSession::New,
            prompt: "Use the authorized local tool.".into(),
            limits: RunLimits {
                deadline: 1500,
                max_turns: 4,
                max_total_tokens: 200_000,
                max_cost_microusd: 200_000,
                max_output_tokens: 64,
                max_operations: 1,
                max_concurrent_operations: 1,
            },
        }),
    };
    let task = f
        .state
        .submit_task(
            token(),
            SubmitTask {
                id: TaskId::new(),
                session_id: f.session,
                conversation_id: None,
                settings: TaskSettings {
                    model: model.clone(),
                    max_turns: 4,
                    max_tokens: 64,
                    deadline: 1500,
                    max_cost_micros: 200_000,
                },
                work,
            },
        )
        .unwrap();
    let task = f.approve(task.id);
    let catalog = f
        .state
        .connector_gateway_catalog(peer(), task.id, task.command.execution)
        .unwrap();
    assert_eq!(catalog.len(), 1);
    assert_eq!(catalog[0].binding, binding);
    let request = |schema| {
        serde_json::from_value::<CodingRequest>(json!({"model":model,"messages":[{"role":"user","content":"Use tool"}],"stream":true,
        "tools":[{"type":"function","function":{"name":binding.vibe_name(),"description":catalog[0].description,"parameters":schema}}]})).unwrap()
    };
    let WorkSpec::Vibe { spec } = &task.command.work else {
        unreachable!()
    };
    assert!(
        request(catalog[0].input_schema.clone())
            .prepare(spec, &catalog)
            .is_ok()
    );
    assert!(
        request(json!({"type":"object"}))
            .prepare(spec, &catalog)
            .is_err()
    );
    f.grant(&c, f.bot, &[]);
    f.grant(&c, f.bot, &["read"]);
    assert!(
        f.state
            .connector_gateway_catalog(peer(), task.id, task.command.execution)
            .unwrap()
            .is_empty()
    );
    assert!(
        f.state
            .admit_connector(
                peer(),
                task.id,
                task.command.execution,
                ConnectorCallId::new(),
                ConnectorOperation::Call {
                    proposal: ConnectorProposal {
                        tool: binding.clone(),
                        arguments: json!({"value":"cached"})
                    }
                }
            )
            .is_err()
    );
    assert!(
        f.state
            .admit_coding(
                peer(),
                task.id,
                task.command.execution,
                InferenceAttemptId::new(),
                request(catalog[0].input_schema.clone())
            )
            .is_err()
    );
}

#[test]
fn authentication_failure_hides_tools_and_large_secret_entry_is_atomic() {
    let mut f = fixture();
    let c = f.ready(true);
    let task = f.call(&c);
    let id = ConnectorCallId::new();
    f.reserve(&task, id);
    f.state.dispatch_connector(id).unwrap().unwrap();
    let before = f.state.load_connection(c.id).unwrap();
    let values = (0..3)
        .map(|i| (format!("TOKEN_{i}"), ConnectorSecretEntry("s".repeat(8192))))
        .collect();
    assert!(
        f.state
            .replace_connection_secret(
                token(),
                c.id,
                ReplaceConnectionSecret {
                    expected_revision: before.version.revision,
                    secret: ConnectionSecretInput::LocalEnvironment { values },
                }
            )
            .is_err()
    );
    assert_eq!(f.state.load_connection(c.id).unwrap(), before);
    f.state
        .finish_connector(
            id,
            ConnectorOutcome::Unknown {
                reason: ConnectorFailure::Authentication,
            },
        )
        .unwrap();
    let after = f.state.load_connection(c.id).unwrap();
    assert_eq!(
        after.availability,
        ConnectionAvailability::AuthenticationRequired
    );
    assert!(after.tools.is_empty());
    assert!(f.state.connector_tools(token(), f.bot).unwrap().is_empty());
    assert_eq!(
        after.version.credential_epoch,
        before.version.credential_epoch
    );
    assert!(after.version.revision > before.version.revision);
}

mod schema_bounds;

#[test]
fn credential_words_cannot_corrupt_discovery_or_tool_settlement_envelopes() {
    let mut f = fixture();
    let connection = f.install(false);
    let connection = f
        .state
        .replace_connection_secret(
            token(),
            connection.id,
            ReplaceConnectionSecret {
                expected_revision: connection.version.revision,
                secret: ConnectionSecretInput::Bearer {
                    value: ConnectorSecretEntry("tool".into()),
                },
            },
        )
        .unwrap();
    let discovery = f.task(ConnectorOperation::Discover {
        connection: connection.version,
    });
    let discovery = f.approve(discovery.id);
    let id = ConnectorCallId::new();
    f.reserve(&discovery, id);
    f.state.dispatch_connector(id).unwrap().unwrap();
    let discovered = f
        .state
        .finish_connector(
            id,
            ConnectorOutcome::Finished {
                result: ConnectorResult::Catalog {
                    tools: vec![tool("read")],
                },
            },
        )
        .expect("redaction must preserve the typed catalog envelope");
    assert!(matches!(
        discovered.state,
        ConnectorCallState::Resolved {
            outcome: ConnectorOutcome::Finished { .. }
        }
    ));
    f.finish_task(discovery.id);
    let connection = f.state.load_connection(connection.id).unwrap();
    let task = f.call(&connection);
    let id = ConnectorCallId::new();
    f.reserve(&task, id);
    f.state.dispatch_connector(id).unwrap().unwrap();
    let call = f
        .state
        .finish_connector(
            id,
            ConnectorOutcome::Finished {
                result: ConnectorResult::Tool {
                    value: json!({"content":[{"type":"text","text":"tool"}], "tool":"tool"}),
                },
            },
        )
        .expect("redaction must preserve the typed tool envelope");
    let ConnectorCallState::Resolved {
        outcome:
            ConnectorOutcome::Finished {
                result: ConnectorResult::Tool { value },
            },
    } = call.state
    else {
        panic!("finished typed result")
    };
    assert_eq!(value["content"][0]["text"], "[redacted]");
    assert_eq!(value["[redacted]"], "[redacted]");
}

fn stopped_unknown() -> (Fixture, TaskView) {
    let mut f = fixture();
    let c = f.ready(true);
    let task = f.call(&c);
    let id = ConnectorCallId::new();
    f.reserve(&task, id);
    assert!(f.state.dispatch_connector(id).unwrap().is_some());
    f.state.recover_connectors().unwrap();
    let reports = stop_reports(&f, &task);
    f.state
        .durable_reports(f.lease, f.journal, 2, reports)
        .unwrap();
    let stopped = f.state.load_task(task.id).unwrap();
    assert_eq!(stopped.state, TaskState::AwaitingAttention);
    (f, stopped)
}
#[test]
fn journal_replacement_preserves_stopped_proof() {
    let (mut f, stopped) = stopped_unknown();
    f.state
        .recovery_hello(f.lease, JournalId::new(), 0)
        .unwrap();
    assert_eq!(
        f.state.load_task(stopped.id).unwrap().outcome,
        stopped.outcome
    );
}
#[test]
fn missing_inventory_preserves_stopped_proof() {
    let (mut f, stopped) = stopped_unknown();
    f.state
        .worker_connections
        .get_mut(&f.lease.worker)
        .unwrap()
        .inventory
        .clear();
    f.state
        .reconcile_inventory(f.lease, f.journal, 0, 0, vec![])
        .unwrap();
    assert_eq!(
        f.state.load_task(stopped.id).unwrap().outcome,
        stopped.outcome
    );
}
#[test]
fn cancel_stopped_unknown_is_idempotent() {
    let (mut f, stopped) = stopped_unknown();
    assert_eq!(f.state.cancel_task(token(), stopped.id).unwrap(), stopped);
}
#[test]
fn revocation_preserves_attention() {
    let (mut f, stopped) = stopped_unknown();
    f.state.revoke_approval(token(), stopped.id).unwrap();
    let receipt: String = f
        .state
        .connection
        .query_row(
            "SELECT body FROM report_receipts WHERE sequence=2",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let duplicate: DurableReport = parse(receipt).unwrap();
    f.state
        .durable_reports(f.lease, f.journal, 2, vec![duplicate])
        .unwrap();
    assert_eq!(
        f.state.load_task(stopped.id).unwrap().state,
        TaskState::AwaitingAttention
    );
}

#[test]
fn stopped_unknown_does_not_occupy_supervised_execution() {
    let (mut f, stopped) = stopped_unknown();
    assert!(
        f.state
            .process_control_status(f.lease.worker)
            .unwrap()
            .is_empty()
    );
    let connection = f.ready(true);
    let next = f.call(&connection);
    assert!(next.approval.consumed);
    assert_eq!(f.state.load_task(stopped.id).unwrap(), stopped);
}
#[test]
fn permission_revocation_preserves_stopped_unknown_attention() {
    let (mut f, stopped) = stopped_unknown();
    let policy = f.state.permission(f.bot).unwrap();
    f.state
        .set_permission(token(), f.bot, policy.revision, false)
        .unwrap();
    let current = f.state.load_task(stopped.id).unwrap();
    assert_eq!(current.state, TaskState::AwaitingAttention);
    assert_eq!(current.outcome, stopped.outcome);
}
