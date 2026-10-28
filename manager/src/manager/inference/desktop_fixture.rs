use super::*;

#[tokio::test]
#[ignore = "Starts the isolated C02 graphical fixture until its stop file appears."]
async fn c02_desktop_fixture() {
    let root = PathBuf::from(std::env::var_os("BOTSTRAL_C02_FIXTURE_ROOT").expect("fixture root"));
    let local = crate::LocalState::open(&root.join("manager-state")).unwrap();
    let fixture = Fixture::start().await;
    fixture.state.desktop.store(true, Ordering::SeqCst);
    let app = App::start(local.directory().into(), &fixture).await;
    app.initialize().await;
    let mut worker = SyntheticWorker::start(&app).await;
    app.call(
        "PUT",
        &format!("/v1/bots/{}/permission", app.bot),
        json!({"expected_revision":1,"allow":true}),
        200,
    )
    .await;
    let listener = crate::public_http::PublicListener::bind(crate::PublicTls {
        listen: "127.0.0.1:0".parse().unwrap(),
        certificate: root.join("manager.pem"),
        key: root.join("manager.key"),
        client_ca: root.join("ca.pem"),
    })
    .await
    .unwrap();
    let public = listener.local_addr().unwrap();
    let (stop, stopped) = watch::channel(false);
    let manager = app.manager.clone();
    let public_task = tokio::spawn(async move {
        listener.serve(manager, stopped).await.unwrap();
    });
    let trust = app.call("POST", "/v1/device-trust", json!({"endpoint":public.to_string(),"server_name":"localhost","ca_pem":std::fs::read_to_string(root.join("ca.pem")).unwrap(),"certificate_pem":std::fs::read_to_string(root.join("manager.pem")).unwrap()}), 200).await;
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(
        root.join("trust.json"),
        serde_json::to_vec_pretty(&trust).unwrap(),
    )
    .unwrap();
    std::fs::set_permissions(
        root.join("trust.json"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    std::fs::write(root.join("fixture.json"), json!({"local":app.address.trim_start_matches("http://"),"public":public,"password":PASSWORD,"bot":app.bot,"session":worker.session.id,"runtime":CertificateFingerprint::from_der(b"fixture runtime"),"synthetic_worker":true}).to_string()).unwrap();
    let mut sequence = 0;
    let mut started = std::collections::HashMap::new();
    let mut finished = std::collections::HashSet::new();
    while !root.join("stop").exists() {
        for message in worker.drain() {
            match message {
                ManagerMessage::Delivery(delivery)
                    if !delivery.cancel && delivery.authority.is_some() =>
                {
                    worker
                        .report(WorkerMessage::Claim {
                            incarnation: worker.journal,
                            fence: worker.session.fence(worker.lease.generation),
                            binding: delivery.authority.unwrap(),
                        })
                        .await;
                }
                permit @ ManagerMessage::Permit { .. } => {
                    let _ = worker.gateway.admit_delivery(worker.lease, permit).await;
                }
                _ => {}
            }
        }
        let replay = app.manager.resume(app.session(), 0, 64).await.unwrap();
        let EventReplay::Snapshot { state, .. } = replay else {
            unreachable!()
        };
        for task in state.tasks {
            if finished.contains(&task.id) || task.state.terminal() {
                continue;
            }
            if task.state == TaskState::Cancelling {
                sequence += 1;
                worker
                    .report(WorkerMessage::Reports {
                        generation: worker.lease.generation,
                        incarnation: worker.journal,
                        high_water: sequence,
                        reports: vec![DurableReport {
                            sequence,
                            command: task.command.id,
                            execution: task.command.execution,
                            event: ExecutionEvent::Stopped {
                                applied: task.progress,
                                evidence: if task.approval.consumed {
                                    StopEvidence::SystemdScopeEmpty {
                                        scope: synthetic_scope(
                                            worker.journal,
                                            task.command.execution,
                                        ),
                                    }
                                } else {
                                    StopEvidence::NotLaunched
                                },
                            },
                        }],
                    })
                    .await;
                finished.insert(task.id);
            } else if task.approval.consumed {
                let since = started
                    .entry(task.id)
                    .or_insert_with(tokio::time::Instant::now);
                let seconds = if matches!(&task.command.work, WorkSpec::Process { spec } if spec.executable == "/usr/bin/sleep")
                {
                    30
                } else {
                    2
                };
                if since.elapsed() >= Duration::from_secs(seconds) {
                    sequence += 2;
                    worker
                        .report(WorkerMessage::Reports {
                            generation: worker.lease.generation,
                            incarnation: worker.journal,
                            high_water: sequence,
                            reports: vec![
                                DurableReport {
                                    sequence: sequence - 1,
                                    command: task.command.id,
                                    execution: task.command.execution,
                                    event: ExecutionEvent::ProcessResult {
                                        evidence: ProcessEvidence {
                                            effect: ProcessEffect::Observed,
                                            result: Some(ProcessResult {
                                                exit: ProcessExit::Exited { code: 0 },
                                                stdout: CapturedOutput {
                                                    hex: "646f6e65".into(),
                                                    truncated: false,
                                                    complete: true,
                                                },
                                                stderr: CapturedOutput {
                                                    hex: String::new(),
                                                    truncated: false,
                                                    complete: true,
                                                },
                                            }),
                                        },
                                    },
                                },
                                DurableReport {
                                    sequence,
                                    command: task.command.id,
                                    execution: task.command.execution,
                                    event: ExecutionEvent::ProcessFinished {
                                        scope: synthetic_scope(
                                            worker.journal,
                                            task.command.execution,
                                        ),
                                        reason: ProcessCompletionReason::Result,
                                        success: true,
                                    },
                                },
                            ],
                        })
                        .await;
                    for output in &task.outputs {
                        worker.artifact(&task, &output.declaration.slot, b"Synthetic C02 recorded worker output. No production process ran.\n").await;
                    }
                    finished.insert(task.id);
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    stop.send_replace(true);
    public_task.await.unwrap();
    drop(worker);
    app.shutdown().await;
}

fn synthetic_scope(journal: JournalId, execution: ExecutionId) -> ProcessScope {
    let unit = format!("botstral-{journal}-{execution}.service");
    ProcessScope {
        journal,
        execution,
        host: CertificateFingerprint::from_der(b"C02 synthetic host"),
        boot: RequestId::new().to_string(),
        invocation: "a".repeat(32),
        cgroup: format!("/synthetic/{unit}"),
        unit,
    }
}
