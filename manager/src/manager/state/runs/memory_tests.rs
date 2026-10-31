use super::*;
use std::os::unix::fs::PermissionsExt;

fn write_entry(state: &State, entry: &MemoryEntry) {
    let mut metadata = entry.clone();
    metadata.markdown.clear();
    let path = state.directory.join(format!("entry-{}.md", entry.id));
    fs::write(
        &path,
        format!(
            "<!-- botstral-memory-entry {} -->\n{}",
            serde_json::json!({"type":"live","entry":metadata}),
            entry.markdown
        ),
    )
    .unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

pub(super) fn remember(f: &mut Fixture, revision: u64, markdown: &str) -> MemoryEntry {
    let entry = MemoryEntry {
        id: MemoryId(f.input.bot.0),
        scope: MemoryScope::Individual { bot: f.input.bot },
        revision: revision + 1,
        markdown: markdown.into(),
        source: MemorySource::Imported {
            sources: vec![SourceReference::External {
                reference: "https://example.com/preference".into(),
            }],
        },
    };
    write_entry(&f.state, &entry);
    entry
}
fn legacy(f: &Fixture, markdown: &str) -> Document {
    let document = Document {
        bot: f.input.bot,
        kind: DocumentKind::Memory,
        revision: 7,
        sources: vec![SourceReference::External {
            reference: "https://example.com/original".into(),
        }],
        markdown: markdown.into(),
    };
    let mut metadata = document.clone();
    metadata.markdown.clear();
    let path = f
        .state
        .directory
        .join(format!("memory-{}.md", document.bot));
    fs::write(
        &path,
        format!(
            "<!-- botstral-document {} -->\n{}",
            json(&metadata).unwrap(),
            markdown
        ),
    )
    .unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    document
}
fn stored(f: &Fixture, run: RunId) -> Option<Vec<MemoryEntry>> {
    let value: Option<String> = f
        .state
        .connection
        .query_row(
            "SELECT memory_entries_snapshot FROM runs WHERE id=?1",
            [run.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    value.map(|body| serde_json::from_str(&body).unwrap())
}
fn context(p: &RunCallPermit) -> Vec<MemoryEntry> {
    let RunMessage::User { content } = &p.turn.messages[0] else {
        panic!()
    };
    serde_json::from_str(content.split_once('\n').unwrap().1).unwrap()
}
fn manager_only(f: &mut Fixture) {
    f.input.session = None;
    f.input.execution_policy = RunExecutionPolicy {
        computer: None,
        process: None,
        connectors: None,
        vision: None,
        vibe: None,
    };
}
fn call(f: &mut Fixture, proposal: MemoryProposal) -> MemoryOutcome {
    f.input.id = RunId::new();
    f.state.submit_run(token(), f.input.clone()).unwrap();
    let p = claim(&mut f.state);
    let proposal = RunToolCall {
        id: "memory-call".into(),
        proposal: ToolProposal::Memory(proposal),
    };
    f.state
        .finish_run_call(
            p.run,
            p.turn.id,
            p.turn.attempt,
            response(Some(proposal.clone())),
        )
        .unwrap();
    let view = f.state.load_run(p.run).unwrap();
    let result = view.turns[0].manager_result.as_ref().unwrap();
    let Some(ManagerOutcome::Memory { outcome }) = &result.outcome else {
        panic!()
    };
    let outcome = outcome.clone();
    f.state
        .finish_run_call(p.run, p.turn.id, p.turn.attempt, response(Some(proposal)))
        .unwrap();
    assert_eq!(view, f.state.load_run(p.run).unwrap());
    f.state.cancel_run(token(), p.run).unwrap();
    outcome
}
fn create(f: &mut Fixture, scope: MemoryScopeSelection, text: &str) -> MemoryEntry {
    let MemoryOutcome::Created { entry } = call(
        f,
        MemoryProposal::Create {
            scope,
            markdown: text.into(),
        },
    ) else {
        panic!()
    };
    entry
}

#[test]
fn both_scopes_crud_preserve_one_fact_and_derive_source_from_the_actual_run() {
    let mut f = fixture();
    manager_only(&mut f);
    for scope in [
        MemoryScopeSelection::Individual,
        MemoryScopeSelection::Shared,
    ] {
        let first = create(&mut f, scope, "Use concise answers.");
        let second = create(&mut f, scope, "Prefer morning meetings.");
        let original = f.state.memory_context(f.input.bot).unwrap();
        assert!(matches!(first.source, MemorySource::Run { bot, .. } if bot == f.input.bot));
        assert_eq!(first.scope, scope.resolve(f.input.bot));
        assert_eq!(
            call(
                &mut f,
                MemoryProposal::Update {
                    scope,
                    id: first.id,
                    expected_revision: 0,
                    markdown: "wrong revision".into()
                }
            ),
            MemoryOutcome::Refused {
                reason: MemoryRefusal::RevisionConflict
            }
        );
        assert_eq!(f.state.memory_context(f.input.bot).unwrap(), original);
        let MemoryOutcome::Updated { entry } = call(
            &mut f,
            MemoryProposal::Update {
                scope,
                id: first.id,
                expected_revision: 1,
                markdown: "Use complete sentences.".into(),
            },
        ) else {
            panic!()
        };
        assert_eq!(entry.revision, 2);
        assert!(
            f.state
                .memory_context(f.input.bot)
                .unwrap()
                .contains(&second)
        );
        let MemoryOutcome::Listed {
            entries,
            scope: owner,
        } = call(&mut f, MemoryProposal::List { scope })
        else {
            panic!()
        };
        assert_eq!(owner, scope.resolve(f.input.bot));
        assert!(entries.contains(&entry) && entries.contains(&second));
        assert_eq!(
            call(
                &mut f,
                MemoryProposal::Delete {
                    scope,
                    id: first.id,
                    expected_revision: 2
                }
            ),
            MemoryOutcome::Deleted {
                id: first.id,
                scope: owner,
                revision: 3
            }
        );
        let bytes =
            fs::read_to_string(f.state.directory.join(format!("entry-{}.md", first.id))).unwrap();
        assert!(bytes.contains("tombstone") && !bytes.contains("Use complete sentences."));
        assert_eq!(
            call(
                &mut f,
                MemoryProposal::Update {
                    scope,
                    id: first.id,
                    expected_revision: 3,
                    markdown: "resurrect".into()
                }
            ),
            MemoryOutcome::Refused {
                reason: MemoryRefusal::NotFound
            }
        );
    }
    assert_eq!(
        f.state
            .connection
            .query_row::<i64, _, _>(
                "SELECT count(*) FROM memory_publications WHERE state='applied'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
        8
    );
    assert_eq!(
        f.state
            .connection
            .query_row::<i64, _, _>("SELECT count(*) FROM tasks", [], |r| r.get(0))
            .unwrap(),
        0
    );
}

#[test]
fn individual_identity_cannot_cross_bots_and_shared_entries_survive_restart() {
    let mut f = fixture();
    manager_only(&mut f);
    let individual = create(&mut f, MemoryScopeSelection::Individual, "Only this bot.");
    let shared = create(&mut f, MemoryScopeSelection::Shared, "All bots.");
    let other = BotId::new();
    assert_eq!(f.state.memory_context(other).unwrap(), vec![shared.clone()]);
    assert_eq!(
        f.state
            .prepare_memory_change(
                RunId::new(),
                other,
                RunTurnId::new(),
                &MemoryProposal::Update {
                    scope: MemoryScopeSelection::Individual,
                    id: individual.id,
                    expected_revision: 1,
                    markdown: "foreign".into()
                }
            )
            .err(),
        Some(OperationError::NotFound)
    );
    assert_eq!(
        call(
            &mut f,
            MemoryProposal::Delete {
                scope: MemoryScopeSelection::Shared,
                id: individual.id,
                expected_revision: 1
            }
        ),
        MemoryOutcome::Refused {
            reason: MemoryRefusal::NotFound
        }
    );
    restart_tests::reopen(&mut f);
    assert_eq!(f.state.memory_context(other).unwrap(), vec![shared]);
    assert!(
        f.state
            .memory_context(f.input.bot)
            .unwrap()
            .contains(&individual)
    );
}

#[test]
fn new_snapshots_are_explicit_and_immutable_through_correction_and_restart() {
    let mut f = fixture();
    let original = remember(&mut f, 0, "Preference A.");
    f.state.submit_run(token(), f.input.clone()).unwrap();
    remember(&mut f, 1, "Preference B.");
    assert_eq!(stored(&f, f.input.id), Some(vec![original.clone()]));
    for column in ["memory_snapshot", "memory_entries_snapshot"] {
        assert!(
            f.state
                .connection
                .execute(&format!("UPDATE runs SET {column}=NULL"), [])
                .is_err()
        );
    }
    restart_tests::reopen(&mut f);
    let p = claim(&mut f.state);
    assert_eq!(context(&p), vec![original]);
    f.state
        .finish_run_call(p.run, p.turn.id, p.turn.attempt, response(None))
        .unwrap();
    fs::remove_file(
        f.state
            .directory
            .join(format!("entry-{}.md", MemoryId(f.input.bot.0))),
    )
    .unwrap();
    f.input.id = RunId::new();
    f.state.submit_run(token(), f.input.clone()).unwrap();
    assert_eq!(stored(&f, f.input.id), Some(vec![]));
    remember(&mut f, 2, "Must not enter empty snapshot.");
    let p = claim(&mut f.state);
    assert_eq!(
        p.turn.messages,
        vec![RunMessage::User {
            content: f.input.prompt.clone()
        }]
    );
}

#[test]
fn migration_preserves_old_frozen_document_and_null_no_context() {
    for old in [None, Some("Frozen legacy preference.")] {
        let mut f = fixture();
        f.state.submit_run(token(), f.input.clone()).unwrap();
        let doc = legacy(&f, "Present-day memory.");
        f.state.connection.execute_batch("DROP TRIGGER immutable_run; ALTER TABLE runs DROP COLUMN memory_entries_snapshot; DROP TABLE memory_publications; DROP TABLE memory_imports; DROP TABLE credential_prepare_attempts; DROP TABLE credential_authorities; DROP TABLE credential_codes; DROP TABLE credential_rules; DROP TABLE credential_uses; ALTER TABLE execution_leases RENAME TO connector_leases; DROP TABLE credential_epochs; DROP TABLE credential_items; DROP TABLE credential_writes; DROP TABLE run_files; DROP TABLE staged_run_files; ALTER TABLE workers DROP COLUMN second_key; ALTER TABLE owner_devices DROP COLUMN second_key; DROP TABLE join_invitations; PRAGMA user_version=11;").unwrap();
        if let Some(markdown) = old {
            let mut frozen = doc.clone();
            frozen.markdown = markdown.into();
            f.state
                .connection
                .execute(
                    "UPDATE runs SET memory_snapshot=?1",
                    [json(&frozen).unwrap()],
                )
                .unwrap();
        }
        f.state.connection.execute_batch("CREATE TRIGGER immutable_run BEFORE UPDATE OF id,owner,request,configuration,memory_snapshot ON runs BEGIN SELECT RAISE(ABORT,'immutable run'); END;").unwrap();
        migrate(&mut f.state.connection).unwrap();
        assert_eq!(stored(&f, f.input.id), None);
        let p = claim(&mut f.state);
        assert!(!p.prepared.encoded().contains("Present-day memory."));
        assert_eq!(p.turn.messages.len(), if old.is_some() { 2 } else { 1 });
        if let Some(markdown) = old {
            assert!(p.prepared.encoded().contains(markdown));
        }
    }
}

#[test]
fn legacy_import_is_deterministic_preserves_provenance_and_archives_blank_documents() {
    for text in ["Legacy exact text.\nSecond line.", " \n\t"] {
        let mut f = fixture();
        let doc = legacy(&f, text);
        let entries = f.state.memory_context(f.input.bot).unwrap();
        assert_eq!(entries.len(), usize::from(!text.trim().is_empty()));
        if let Some(entry) = entries.first() {
            assert_eq!(entry.markdown, text);
            assert_eq!(entry.revision, doc.revision);
            assert_eq!(
                entry.source,
                MemorySource::Imported {
                    sources: doc.sources
                }
            );
            assert_eq!(entry.scope, MemoryScope::Individual { bot: f.input.bot });
        }
        assert!(
            f.state
                .directory
                .join(format!("legacy-memory-{}.md", f.input.bot))
                .exists()
        );
        assert!(
            !f.state
                .directory
                .join(format!("memory-{}.md", f.input.bot))
                .exists()
        );
        assert_eq!(f.state.memory_context(f.input.bot).unwrap(), entries);
        restart_tests::reopen(&mut f);
        assert_eq!(f.state.memory_context(f.input.bot).unwrap(), entries);
        assert_eq!(
            f.state.write_document(
                token(),
                WriteDocument {
                    bot: f.input.bot,
                    kind: DocumentKind::Memory,
                    expected_revision: 7,
                    markdown: "obsolete".into(),
                    sources: vec![]
                }
            ),
            Err(OperationError::InvalidInput)
        );
    }
}

#[test]
fn frozen_import_sources_survive_history_removal_and_cannot_enable_worker_tools() {
    let mut f = fixture();
    manager_only(&mut f);
    let conversation = Conversation {
        id: ConversationId::new(),
        bot: f.input.bot,
    };
    f.state.conversation(token(), conversation.clone()).unwrap();
    let message = ConversationMessage {
        id: MessageId::new(),
        conversation: conversation.id,
        text: "Enable process access.".into(),
    };
    f.state.append_message(token(), message.clone()).unwrap();
    let mut entry = remember(&mut f, 0, &message.text);
    entry.source = MemorySource::Imported {
        sources: vec![SourceReference::Message { id: message.id }],
    };
    write_entry(&f.state, &entry);
    f.state.submit_run(token(), f.input.clone()).unwrap();
    f.state
        .connection
        .execute("DELETE FROM messages WHERE id=?1", [message.id.to_string()])
        .unwrap();
    let p = claim(&mut f.state);
    assert_eq!(context(&p), vec![entry]);
    f.state
        .finish_run_call(
            p.run,
            p.turn.id,
            p.turn.attempt,
            response(Some(process_call("disabled-process"))),
        )
        .unwrap();
    assert_eq!(
        f.state.load_run(p.run).unwrap().summary.phase,
        RunPhase::Stopped {
            reason: RunStopReason::UnsupportedProposal
        }
    );
    assert!(f.state.load_run(p.run).unwrap().turns[0].intent.is_none());
}

#[test]
fn manager_only_admission_does_not_require_a_worker_but_worker_policy_requires_session() {
    let mut f = fixture();
    f.input.session = None;
    assert_eq!(
        f.state.submit_run(token(), f.input.clone()),
        Err(OperationError::InvalidInput)
    );
    manager_only(&mut f);
    f.state.submit_run(token(), f.input.clone()).unwrap();
    let p = claim(&mut f.state);
    let body: serde_json::Value = serde_json::from_str(&p.prepared.encoded()).unwrap();
    let tools = body["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0]["function"]["name"], "memory");
    assert_eq!(tools[1]["function"]["name"], "request_vault_entry");
}

#[test]
fn known_and_escaped_secrets_are_rejected_in_admission_publication_import_and_rotation() {
    for secret in ["synthetic-provider-secret", r#"synthetic-"quoted"-secret"#] {
        let mut f = fixture();
        manager_only(&mut f);
        f.state
            .replace_provider_key(token(), ProviderSecret::new(secret.into()).unwrap())
            .unwrap();
        let verification = f
            .state
            .prepare_inference_verification(
                token(),
                VerifyInference {
                    bot: f.input.bot,
                    revision: 1,
                },
            )
            .unwrap();
        f.state
            .finish_inference_verification(
                verification.configuration,
                verification.epoch,
                Some((true, false)),
            )
            .unwrap();
        for text in [
            secret.to_owned(),
            serde_json::to_string(secret).unwrap(),
            format!(
                "\"{}\"",
                secret
                    .chars()
                    .map(|c| format!("\\u{:04x}", c as u32))
                    .collect::<String>()
            ),
        ] {
            f.input.prompt = text.clone();
            assert_eq!(
                f.state.submit_run(token(), f.input.clone()),
                Err(OperationError::InvalidInput)
            );
            f.input.prompt = "Remember a preference.".into();
            f.state.submit_run(token(), f.input.clone()).unwrap();
            let p = claim(&mut f.state);
            f.state
                .finish_run_call(
                    p.run,
                    p.turn.id,
                    p.turn.attempt,
                    response(Some(RunToolCall {
                        id: "secret".into(),
                        proposal: ToolProposal::Memory(MemoryProposal::Create {
                            scope: MemoryScopeSelection::Individual,
                            markdown: text,
                        }),
                    })),
                )
                .unwrap();
            let run = f.state.load_run(p.run).unwrap();
            assert!(run.turns[0].response.is_none());
            assert_eq!(f.state.memory_context(f.input.bot).unwrap(), vec![]);
            f.input.id = RunId::new();
        }
        legacy(&f, secret);
        assert_eq!(
            f.state.memory_context(f.input.bot),
            Err(OperationError::InvalidInput)
        );
        assert_eq!(
            f.state
                .connection
                .query_row::<i64, _, _>("SELECT count(*) FROM memory_imports", [], |r| r.get(0))
                .unwrap(),
            0
        );
    }
    let mut f = fixture();
    let secret = "future-rotated-secret";
    let entry = remember(&mut f, 0, secret);
    f.state.submit_run(token(), f.input.clone()).unwrap();
    f.state
        .replace_provider_key(token(), ProviderSecret::new(secret.into()).unwrap())
        .unwrap();
    let verification = f
        .state
        .prepare_inference_verification(
            token(),
            VerifyInference {
                bot: f.input.bot,
                revision: 1,
            },
        )
        .unwrap();
    f.state
        .finish_inference_verification(
            verification.configuration,
            verification.epoch,
            Some((true, false)),
        )
        .unwrap();
    assert!(
        f.state
            .claim_run_work(&HashSet::new(), true)
            .unwrap()
            .is_none()
    );
    assert_eq!(stored(&f, f.input.id), Some(vec![entry]));
    assert_eq!(
        f.state.load_run(f.input.id).unwrap().summary.phase,
        RunPhase::AwaitingAttention {
            reason: RunAttentionReason::ResultInvalid
        }
    );
}

#[test]
fn limits_and_cancellation_precede_publication_and_vault_handoff_has_no_value() {
    for stop in [
        "cancel",
        "deadline",
        "operations",
        "turns",
        "tokens",
        "cost",
    ] {
        let mut f = fixture();
        manager_only(&mut f);
        if stop == "turns" {
            f.input.limits.max_turns = 1;
        }
        f.state.submit_run(token(), f.input.clone()).unwrap();
        let p = claim(&mut f.state);
        match stop {
            "cancel" => {
                f.state.cancel_run(token(), p.run).unwrap();
            }
            "deadline" => f.state.test_time = Some(1501),
            "operations" | "tokens" | "cost" => {
                let mut view = f.state.load_run(p.run).unwrap();
                match stop {
                    "operations" => {
                        view.summary.accounting.admitted_operations = f.input.limits.max_operations
                    }
                    "tokens" => {
                        view.summary.accounting.settled_tokens = f.input.limits.max_total_tokens
                    }
                    _ => {
                        view.summary.accounting.settled_cost_microusd =
                            f.input.limits.max_cost_microusd
                    }
                }
                save_summary(&f.state.connection, &view.summary).unwrap();
            }
            _ => {}
        }
        f.state
            .finish_run_call(
                p.run,
                p.turn.id,
                p.turn.attempt,
                response(Some(RunToolCall {
                    id: "too-late".into(),
                    proposal: ToolProposal::Memory(MemoryProposal::Create {
                        scope: MemoryScopeSelection::Individual,
                        markdown: "Must not be saved.".into(),
                    }),
                })),
            )
            .unwrap();
        assert!(
            f.state.memory_context(f.input.bot).unwrap().is_empty(),
            "{stop}"
        );
    }
    for purpose in [VaultPurpose::ProviderKey, VaultPurpose::ReusableLogin] {
        let mut f = fixture();
        manager_only(&mut f);
        f.state.submit_run(token(), f.input.clone()).unwrap();
        let p = claim(&mut f.state);
        f.state
            .finish_run_call(
                p.run,
                p.turn.id,
                p.turn.attempt,
                response(Some(RunToolCall {
                    id: "handoff".into(),
                    proposal: ToolProposal::RequestVaultEntry(RequestVaultEntry { purpose }),
                })),
            )
            .unwrap();
        let view = f.state.load_run(p.run).unwrap();
        let result = view.turns[0].manager_result.as_ref().unwrap();
        assert!(
            result
                .result
                .contains(if purpose == VaultPurpose::ProviderKey {
                    "protected local administration"
                } else {
                    "protected native login entry"
                })
        );
        assert!(f.state.memory_context(f.input.bot).unwrap().is_empty());
    }
    for invalid in [
        r#"{"purpose":"provider_key","value":"secret"}"#,
        r#"{"purpose":"store my secret"}"#,
    ] {
        assert!(tools::decode("request_vault_entry", invalid).is_err());
    }
    for invalid in [
        r#"{"action":"create","scope":"individual","markdown":"x","bot":"foreign"}"#,
        r#"{"action":"create","scope":"shared","markdown":"x","source":{}}"#,
    ] {
        assert!(tools::decode("memory", invalid).is_err());
    }
}

#[cfg(feature = "nonproduction-test-worker")]
#[path = "memory_tests/crash.rs"]
mod crash;

#[path = "memory_tests/acceptance.rs"]
mod acceptance;
