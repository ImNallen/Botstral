use super::*;

#[tokio::test]
#[ignore = "Starts the isolated C02 graphical fixture until its stop file appears."]
async fn c02_desktop_fixture() {
    let root = PathBuf::from(std::env::var_os("BOTSTRAL_C02_FIXTURE_ROOT").expect("fixture root"));
    let local = crate::LocalState::open(&root.join("manager-state")).unwrap();
    let fixture = Fixture::start().await;
    fixture.state.desktop.store(
        std::env::var_os("BOTSTRAL_MEMORY_PROOF").is_none(),
        Ordering::SeqCst,
    );
    let mut app = App::start(local.directory().into(), &fixture).await;
    app.initialize().await;
    app.configure(1).await;
    app.verify(2, 200).await;
    let mut worker = SyntheticWorker::start(&app).await;
    app.call(
        "PUT",
        &format!("/v1/bots/{}/permission", app.bot),
        json!({"expected_revision":1,"allow":true}),
        200,
    )
    .await;
    let (public, mut stop, mut public_task) =
        desktop_listener(&root, &app, "127.0.0.1:0".parse().unwrap()).await;
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
    let details = json!({"local":app.address.trim_start_matches("http://"),"public":public,"password":PASSWORD,"bot":app.bot,"session":worker.session.id,"runtime":CertificateFingerprint::from_der(b"fixture runtime"),"synthetic_worker":true});
    std::fs::write(root.join("fixture.json"), details.to_string()).unwrap();
    let mut sequence = 0;
    let mut started = std::collections::HashMap::new();
    let mut finished = std::collections::HashSet::new();
    while !root.join("stop").exists() {
        if std::env::var_os("BOTSTRAL_MEMORY_PROOF").is_some() {
            if root.join("provider-script.json").exists() {
                let bodies: Vec<String> = serde_json::from_slice(
                    &std::fs::read(root.join("provider-script.json")).unwrap(),
                )
                .unwrap();
                *fixture.state.script.lock().unwrap() = bodies
                    .into_iter()
                    .map(|body| Reply {
                        body,
                        ..Reply::default()
                    })
                    .collect();
                std::fs::remove_file(root.join("provider-script.json")).unwrap();
                std::fs::write(root.join("provider-script-loaded"), b"ready").unwrap();
            }
            std::fs::write(
                root.join("provider-requests.json"),
                serde_json::to_vec_pretty(&*fixture.state.requests.lock().unwrap()).unwrap(),
            )
            .unwrap();
            if root.join("restart-manager").exists() {
                let snapshots = desktop_memory_snapshots(local.directory());
                stop.send_replace(true);
                public_task.await.unwrap();
                let prior_session = worker.session.clone();
                drop(worker);
                let bot = app.bot;
                app.shutdown().await;
                app = App::start(local.directory().into(), &fixture).await;
                app.bot = bot;
                app.call(
                    "POST",
                    "/v1/vault/unlock",
                    json!({"passphrase":PASSPHRASE}),
                    204,
                )
                .await;
                assert_eq!(desktop_memory_snapshots(local.directory()), snapshots);
                worker = SyntheticWorker::reconnect(&app, prior_session).await;
                (_, stop, public_task) = desktop_listener(&root, &app, public).await;
                std::fs::write(root.join("restart-proof.json"), serde_json::to_vec_pretty(&json!({"historical_snapshots_before_and_after":snapshots,"unchanged":true})).unwrap()).unwrap();
                std::fs::write(
                    root.join("restart-session.json"),
                    serde_json::to_vec(&worker.session).unwrap(),
                )
                .unwrap();
                std::fs::remove_file(root.join("restart-manager")).unwrap();
                std::fs::write(root.join("manager-restarted"), b"ready").unwrap();
                sequence = 0;
                started.clear();
                finished.clear();
            }
        }
        if root.join("pause-worker").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
            continue;
        }
        for message in worker.drain() {
            match message {
                ManagerMessage::Control { session, .. }
                    if std::env::var_os("BOTSTRAL_P05_PROOF").is_some() =>
                {
                    worker
                        .report(WorkerMessage::ControlApplied {
                            fence: session.fence(worker.lease.generation),
                        })
                        .await;
                }
                ManagerMessage::Delivery(delivery)
                    if !delivery.cancel && delivery.authority.is_some() =>
                {
                    let session = if std::env::var_os("BOTSTRAL_P05_PROOF").is_some() {
                        app.manager
                            .computer_session(app.session(), delivery.body.session)
                            .await
                            .unwrap()
                    } else {
                        worker.session.clone()
                    };
                    worker
                        .report(WorkerMessage::Claim {
                            incarnation: worker.journal,
                            fence: session.fence(worker.lease.generation),
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

async fn desktop_listener(
    root: &std::path::Path,
    app: &App,
    address: std::net::SocketAddr,
) -> (std::net::SocketAddr, watch::Sender<bool>, JoinHandle<()>) {
    let listener = crate::public_http::PublicListener::bind(crate::PublicTls {
        listen: address,
        certificate: root.join("manager.pem"),
        key: root.join("manager.key"),
        client_ca: root.join("ca.pem"),
    })
    .await
    .unwrap();
    let public = listener.local_addr().unwrap();
    let (stop, stopped) = watch::channel(false);
    let manager = app.manager.clone();
    let task = tokio::spawn(async move {
        listener.serve(manager, stopped).await.unwrap();
    });
    (public, stop, task)
}

fn desktop_memory_snapshots(
    directory: &std::path::Path,
) -> std::collections::BTreeMap<String, (Option<String>, Option<String>)> {
    let connection = rusqlite::Connection::open(directory.join("manager.sqlite3")).unwrap();
    connection
        .prepare("SELECT id,memory_snapshot,memory_entries_snapshot FROM runs ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, (row.get(1)?, row.get(2)?))))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}
