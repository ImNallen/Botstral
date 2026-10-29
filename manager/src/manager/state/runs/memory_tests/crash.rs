use super::*;
use std::process::Command;

fn persistent(f: &Fixture) {
    fs::set_permissions(&f.state.directory, fs::Permissions::from_mode(0o700)).unwrap();
    let Installation::Configured {
        vault: VaultState::Unlocked { identity },
    } = &f.state.installation
    else {
        panic!()
    };
    let path = f.state.directory.join("fixture-identity");
    fs::write(&path, identity.to_string().expose_secret()).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    let path = f.state.directory.join("manager.sqlite3");
    f.state
        .connection
        .execute("VACUUM INTO ?1", [path.to_str().unwrap()])
        .unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    let lock = f.state.directory.join("manager.lock");
    fs::write(&lock, b"").unwrap();
    fs::set_permissions(lock, fs::Permissions::from_mode(0o600)).unwrap();
}
fn open(root: &std::path::Path) -> State {
    let (configured, _) = watch::channel(false);
    let mut state = State::open(root.into(), configured).unwrap();
    state.test_time = Some(1000);
    state.installation = Installation::Configured {
        vault: VaultState::Unlocked {
            identity: fs::read_to_string(root.join("fixture-identity"))
                .unwrap()
                .parse()
                .unwrap(),
        },
    };
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
    state
}
#[test]
fn child() {
    let Ok(root) = std::env::var("BOTSTRAL_MEMORY_CRASH_ROOT") else {
        return;
    };
    let mut state = open(std::path::Path::new(&root));
    if std::env::var_os("BOTSTRAL_MEMORY_IMPORT").is_some() {
        let bot = state.list_bots(token()).unwrap()[0].id;
        state.memory_context(bot).unwrap();
    } else {
        let p = claim(&mut state);
        let proposal = std::env::var("BOTSTRAL_MEMORY_MUTATION")
            .ok()
            .map(|value| serde_json::from_str(&value).unwrap())
            .unwrap_or(MemoryProposal::Create {
                scope: MemoryScopeSelection::Shared,
                markdown: "Saved exactly once.".into(),
            });
        state
            .finish_run_call(
                p.run,
                p.turn.id,
                p.turn.attempt,
                response(Some(RunToolCall {
                    id: "crash-call".into(),
                    proposal: ToolProposal::Memory(proposal),
                })),
            )
            .unwrap();
    }
    panic!("fault was not reached");
}
fn crash(root: &std::path::Path, point: &str, import: bool) {
    crash_proposal(root, point, import, None);
}
fn crash_proposal(
    root: &std::path::Path,
    point: &str,
    import: bool,
    proposal: Option<&MemoryProposal>,
) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "manager::state::runs::tests::memory_tests::crash::child",
            "--nocapture",
        ])
        .env("BOTSTRAL_MEMORY_CRASH_ROOT", root)
        .env("BOTSTRAL_FAULT", point);
    if import {
        command.env("BOTSTRAL_MEMORY_IMPORT", "1");
    }
    if let Some(proposal) = proposal {
        command.env("BOTSTRAL_MEMORY_MUTATION", json(proposal).unwrap());
    }
    let output = command.output().unwrap();
    assert!(!output.status.success());
    assert!(
        root.join(format!("fault-{point}.reached")).exists(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn updates_and_tombstones_recover_the_same_revision_at_each_boundary() {
    for point in [
        "memory_intent_committed",
        "memory_before_rename",
        "memory_after_rename",
        "memory_before_receipt",
    ] {
        for delete in [false, true] {
            let mut f = fixture();
            manager_only(&mut f);
            let entry = create(&mut f, MemoryScopeSelection::Shared, "Before recovery.");
            f.input.id = RunId::new();
            f.state.submit_run(token(), f.input.clone()).unwrap();
            persistent(&f);
            let proposal = if delete {
                MemoryProposal::Delete {
                    scope: MemoryScopeSelection::Shared,
                    id: entry.id,
                    expected_revision: 1,
                }
            } else {
                MemoryProposal::Update {
                    scope: MemoryScopeSelection::Shared,
                    id: entry.id,
                    expected_revision: 1,
                    markdown: "After recovery.".into(),
                }
            };
            crash_proposal(&f.state.directory, point, false, Some(&proposal));
            let mut state = open(&f.state.directory);
            state.recover_memory_publications().unwrap();
            state.recover_memory_publications().unwrap();
            let entries = state.memory_context(f.input.bot).unwrap();
            let outcome = state.load_run(f.input.id).unwrap().turns[0]
                .manager_result
                .as_ref()
                .unwrap()
                .outcome
                .clone();
            if delete {
                assert!(entries.is_empty());
                assert_eq!(
                    outcome,
                    Some(ManagerOutcome::Memory {
                        outcome: MemoryOutcome::Deleted {
                            id: entry.id,
                            scope: entry.scope,
                            revision: 2
                        }
                    })
                );
            } else {
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].id, entry.id);
                assert_eq!(entries[0].revision, 2);
                assert_eq!(entries[0].markdown, "After recovery.");
                assert_eq!(
                    outcome,
                    Some(ManagerOutcome::Memory {
                        outcome: MemoryOutcome::Updated {
                            entry: entries[0].clone()
                        }
                    })
                );
            }
            assert_eq!(
                state
                    .connection
                    .query_row::<i64, _, _>(
                        "SELECT count(*) FROM events WHERE kind='memory_published'",
                        [],
                        |r| r.get(0)
                    )
                    .unwrap(),
                2
            );
        }
    }
}

#[test]
fn committed_intent_survives_every_publication_crash_without_provider_resend_or_extra_revision() {
    for point in [
        "memory_before_intent",
        "memory_intent_committed",
        "memory_before_rename",
        "memory_after_rename",
        "memory_before_receipt",
        "memory_receipt_committed",
    ] {
        let mut f = fixture();
        manager_only(&mut f);
        f.state.submit_run(token(), f.input.clone()).unwrap();
        persistent(&f);
        crash(&f.state.directory, point, false);
        let mut state = open(&f.state.directory);
        state.recover_memory_publications().unwrap();
        let entries = state.memory_context(f.input.bot).unwrap();
        let run = state.load_run(f.input.id).unwrap();
        assert_eq!(run.turns.len(), 1);
        if point == "memory_before_intent" {
            assert!(entries.is_empty());
            assert!(run.turns[0].manager_result.is_none());
            assert!(matches!(
                run.summary.phase,
                RunPhase::AwaitingAttention { .. }
            ));
            assert!(
                state
                    .claim_run_work(&HashSet::new(), true)
                    .unwrap()
                    .is_none()
            );
        } else {
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].revision, 1);
            assert_eq!(entries[0].markdown, "Saved exactly once.");
            assert!(run.turns[0].manager_result.is_some());
            assert_eq!(run.summary.accounting.settled_tokens, 20);
            assert_eq!(run.summary.accounting.admitted_operations, 1);
            let turn = &run.turns[0];
            let RunMessage::Assistant { tool_call, .. } = turn.response.as_ref().unwrap() else {
                panic!()
            };
            state
                .finish_run_call(
                    run.request.id,
                    turn.id,
                    turn.attempt,
                    response(tool_call.clone()),
                )
                .unwrap();
            state.recover_memory_publications().unwrap();
            assert_eq!(state.load_run(run.request.id).unwrap(), run);
            assert_eq!(
                state
                    .connection
                    .query_row::<i64, _, _>(
                        "SELECT count(*) FROM events WHERE kind='memory_published'",
                        [],
                        |r| r.get(0)
                    )
                    .unwrap(),
                1
            );
        }
    }
}

#[test]
fn admitted_publication_survives_cancellation_but_never_overwrites_an_unexpected_digest() {
    for cancel in [false, true] {
        let mut f = fixture();
        manager_only(&mut f);
        f.state.submit_run(token(), f.input.clone()).unwrap();
        persistent(&f);
        crash(&f.state.directory, "memory_intent_committed", false);
        let mut state = open(&f.state.directory);
        if cancel {
            state.cancel_run(token(), f.input.id).unwrap();
            state.recover_memory_publications().unwrap();
            let run = state.load_run(f.input.id).unwrap();
            assert_eq!(run.summary.phase, RunPhase::Cancelled);
            assert!(run.turns[0].manager_result.is_some());
            assert_eq!(state.memory_context(f.input.bot).unwrap().len(), 1);
        } else {
            let id: String = state
                .connection
                .query_row("SELECT entry FROM memory_publications", [], |r| r.get(0))
                .unwrap();
            let path = state.directory.join(format!("entry-{id}.md"));
            fs::write(&path, "Unexpected external edit.").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            assert_eq!(
                state.recover_memory_publications(),
                Err(OperationError::Conflict)
            );
            assert_eq!(
                fs::read_to_string(&path).unwrap(),
                "Unexpected external edit."
            );
            assert_eq!(
                state.memory_context(f.input.bot),
                Err(OperationError::Conflict)
            );
            let mut next = f.input.clone();
            next.id = RunId::new();
            assert_eq!(
                state.submit_run(token(), next),
                Err(OperationError::Conflict)
            );
            assert!(
                state
                    .claim_run_work(&HashSet::new(), true)
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                state.load_run(f.input.id).unwrap().summary.phase,
                RunPhase::AwaitingAttention {
                    reason: RunAttentionReason::MemoryPublication
                }
            );
        }
    }
}

#[test]
fn interrupted_import_never_duplicates_or_promotes_legacy_memory() {
    for point in [
        "memory_import_committed",
        "memory_import_published",
        "memory_import_archived",
    ] {
        let f = fixture();
        let doc = legacy(&f, "Exact legacy text.");
        persistent(&f);
        crash(&f.state.directory, point, true);
        let mut state = open(&f.state.directory);
        let entries = state.memory_context(f.input.bot).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].revision, 7);
        assert_eq!(entries[0].markdown, doc.markdown);
        assert_eq!(entries[0].scope, MemoryScope::Individual { bot: doc.bot });
        assert_eq!(
            entries[0].source,
            MemorySource::Imported {
                sources: doc.sources
            }
        );
        assert_eq!(state.memory_context(f.input.bot).unwrap(), entries);
        assert!(state.memory_context(BotId::new()).unwrap().is_empty());
    }
}

#[test]
fn backup_restores_entries_tombstones_archives_and_an_unfinished_publication() {
    let mut f = fixture();
    manager_only(&mut f);
    legacy(&f, "Legacy backup content.");
    let keep = create(&mut f, MemoryScopeSelection::Shared, "Retain this fact.");
    let remove = create(
        &mut f,
        MemoryScopeSelection::Individual,
        "Delete this fact.",
    );
    call(
        &mut f,
        MemoryProposal::Delete {
            scope: MemoryScopeSelection::Individual,
            id: remove.id,
            expected_revision: 1,
        },
    );
    f.input.id = RunId::new();
    f.state.submit_run(token(), f.input.clone()).unwrap();
    persistent(&f);
    crash(&f.state.directory, "memory_intent_committed", false);
    let backup_root = tempfile::tempdir().unwrap();
    fs::set_permissions(backup_root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let backup = backup_root.path().canonicalize().unwrap().join("backup");
    let restored = backup_root.path().canonicalize().unwrap().join("restored");
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("packaging/backup.py");
    for args in [
        vec![
            "create",
            "--state-dir",
            f.state.directory.to_str().unwrap(),
            "--destination",
            backup.to_str().unwrap(),
        ],
        vec![
            "restore",
            "--backup",
            backup.to_str().unwrap(),
            "--destination",
            restored.to_str().unwrap(),
        ],
    ] {
        let output = Command::new("python3")
            .arg(&script)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let mut state = open(&restored);
    state.recover_memory_publications().unwrap();
    let entries = state.memory_context(f.input.bot).unwrap();
    assert_eq!(entries.len(), 3);
    assert!(entries.contains(&keep));
    assert!(!entries.iter().any(|e| e.id == remove.id));
    assert!(
        fs::read_to_string(restored.join(format!("entry-{}.md", remove.id)))
            .unwrap()
            .contains("tombstone")
    );
    assert!(
        restored
            .join(format!("legacy-memory-{}.md", f.input.bot))
            .exists()
    );
    assert!(
        state.load_run(f.input.id).unwrap().turns[0]
            .manager_result
            .is_some()
    );
}
