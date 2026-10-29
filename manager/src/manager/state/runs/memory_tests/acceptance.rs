use super::*;

#[test]
fn protected_run_requests_refuse_access_before_and_after_vault_lock() {
    for in_environment in [false, true] {
        let mut f = fixture();
        let secret = "future-protected-run-request-value";
        if in_environment {
            f.input
                .execution_policy
                .process
                .as_mut()
                .unwrap()
                .environment
                .insert("REVIEW_VALUE".into(), secret.into());
        } else {
            f.input.prompt = secret.into();
        }
        f.state.submit_run(token(), f.input.clone()).unwrap();
        let p = claim(&mut f.state);
        f.state
            .finish_run_call(p.run, p.turn.id, p.turn.attempt, response(None))
            .unwrap();
        f.state
            .replace_provider_key(token(), ProviderSecret::new(secret.into()).unwrap())
            .unwrap();
        for locked in [false, true] {
            if locked {
                f.state.lock_vault(token()).unwrap();
            }
            let workspace = f.state.workspace_run(f.input.id).unwrap();
            assert!(workspace.request.is_none());
            assert!(workspace.turns.is_empty());
            assert_eq!(workspace.summary.id, f.input.id);
            assert!(!json(&workspace).unwrap().contains(secret));
            for result in [
                f.state.run(token(), f.input.id),
                f.state.submit_run(token(), f.input.clone()),
                f.state.cancel_run(token(), f.input.id),
            ] {
                match result {
                    Ok(view) => assert!(
                        !json(&view).unwrap().contains(secret),
                        "Protected run content was exposed; locked={locked}, environment={in_environment}"
                    ),
                    Err(error) => assert!(matches!(
                        error,
                        OperationError::Locked | OperationError::InvalidInput
                    )),
                }
            }
        }
    }
}

#[test]
fn protected_run_configuration_is_withheld_after_secret_rotation() {
    let mut f = fixture();
    manager_only(&mut f);
    f.state.submit_run(token(), f.input.clone()).unwrap();
    let p = claim(&mut f.state);
    f.state
        .finish_run_call(p.run, p.turn.id, p.turn.attempt, response(None))
        .unwrap();
    let configuration = f.state.load_run(f.input.id).unwrap().configuration;
    let secret = configuration.policy.acknowledgment.model_developer;
    f.state
        .replace_provider_key(token(), ProviderSecret::new(secret).unwrap())
        .unwrap();
    assert_eq!(
        f.state.run(token(), f.input.id).err(),
        Some(OperationError::InvalidInput)
    );
}

#[test]
fn secret_bearing_response_identifiers_are_not_saved_in_success_or_error_journals() {
    for billed in [false, true] {
        let mut f = fixture();
        manager_only(&mut f);
        f.state.submit_run(token(), f.input.clone()).unwrap();
        let p = claim(&mut f.state);
        let secret = "synthetic-state-inference-secret";
        let result = if billed {
            let mut result = response(Some(RunToolCall {
                id: "identifier-check".into(),
                proposal: ToolProposal::Memory(MemoryProposal::List {
                    scope: MemoryScopeSelection::Shared,
                }),
            }));
            if let ModelOutcome::Billed { request_id, .. } = &mut result {
                *request_id = secret.into();
            }
            result
        } else {
            ModelOutcome::Unknown {
                reason: InferenceAttentionReason::InvalidResponse,
                request_id: Some(secret.into()),
            }
        };
        f.state
            .finish_run_call(p.run, p.turn.id, p.turn.attempt, result)
            .unwrap();
        let view = f.state.load_run(p.run).unwrap();
        assert!(!json(&view).unwrap().contains(secret));
        assert!(view.turns[0].provider_request_id.is_none());
        assert_eq!(
            view.summary.accounting.settled_tokens,
            if billed { 20 } else { 0 }
        );
    }
}

#[test]
fn rotation_guards_dispatch_list_and_every_public_run_response() {
    let mut f = fixture();
    manager_only(&mut f);
    let secret = "future-provider-secret";
    create(&mut f, MemoryScopeSelection::Shared, secret);
    let committed = f.input.clone();
    f.input.id = RunId::new();
    f.state.submit_run(token(), f.input.clone()).unwrap();
    let p = claim(&mut f.state);
    let body = serde_json::from_str(&p.prepared.encoded()).unwrap();
    assert!(
        f.state
            .authorize_run_dispatch(p.run, p.turn.id, &body)
            .unwrap()
    );
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
        !f.state
            .authorize_run_dispatch(p.run, p.turn.id, &body)
            .unwrap()
    );
    assert_eq!(
        f.state.memory_context(f.input.bot),
        Err(OperationError::InvalidInput)
    );
    for result in [
        f.state.run(token(), committed.id),
        f.state.submit_run(token(), committed.clone()),
        f.state.cancel_run(token(), committed.id),
        f.state.cancel_run(token(), p.run),
    ] {
        assert_eq!(result.err(), Some(OperationError::InvalidInput));
    }
}

#[test]
fn entry_limits_and_private_file_checks_preserve_existing_content() {
    let mut f = fixture();
    manager_only(&mut f);
    let entry = create(
        &mut f,
        MemoryScopeSelection::Individual,
        "Keep this exact fact.",
    );
    for markdown in [" ".into(), "x".repeat(MAX_MEMORY_ENTRY_BYTES + 1)] {
        assert_eq!(
            call(
                &mut f,
                MemoryProposal::Update {
                    scope: MemoryScopeSelection::Individual,
                    id: entry.id,
                    expected_revision: 1,
                    markdown
                }
            ),
            MemoryOutcome::Refused {
                reason: MemoryRefusal::InvalidInput
            }
        );
        assert_eq!(
            f.state.memory_context(f.input.bot).unwrap(),
            vec![entry.clone()]
        );
    }
    for _ in 1..MAX_MEMORY_ENTRIES_PER_SCOPE {
        let mut next = entry.clone();
        next.id = MemoryId::new();
        write_entry(&f.state, &next);
    }
    assert_eq!(
        call(
            &mut f,
            MemoryProposal::Create {
                scope: MemoryScopeSelection::Individual,
                markdown: "Above the count limit.".into()
            }
        ),
        MemoryOutcome::Refused {
            reason: MemoryRefusal::Capacity
        }
    );
    assert_eq!(
        f.state.memory_context(f.input.bot).unwrap().len(),
        MAX_MEMORY_ENTRIES_PER_SCOPE
    );
    let path = f.state.directory.join(format!("entry-{}.md", entry.id));
    let original = fs::read(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(f.state.memory_context(f.input.bot).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let target = f.state.directory.join("private-memory-test");
    fs::rename(&path, &target).unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();
    assert!(f.state.memory_context(f.input.bot).is_err());
    fs::remove_file(&path).unwrap();
    fs::hard_link(&target, &path).unwrap();
    assert!(f.state.memory_context(f.input.bot).is_err());
    fs::remove_file(&path).unwrap();
    fs::rename(&target, &path).unwrap();
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(
        f.state
            .memory_context(f.input.bot)
            .unwrap()
            .contains(&entry)
    );
}

#[test]
fn legacy_import_keeps_deleted_history_provenance_without_exposing_a_document_editor() {
    let mut f = fixture();
    let mut doc = legacy(&f, "Imported fact with retained provenance.");
    doc.sources = vec![SourceReference::Message {
        id: MessageId::new(),
    }];
    let mut metadata = doc.clone();
    metadata.markdown.clear();
    let path = f.state.directory.join(format!("memory-{}.md", doc.bot));
    fs::write(
        path,
        format!(
            "<!-- botstral-document {} -->\n{}",
            json(&metadata).unwrap(),
            doc.markdown
        ),
    )
    .unwrap();
    assert!(f.state.rebuild_documents(token()).unwrap().is_empty());
    let entries = f.state.memory_context(f.input.bot).unwrap();
    assert_eq!(
        entries[0].source,
        MemorySource::Imported {
            sources: doc.sources
        }
    );
    assert_eq!(entries[0].markdown, doc.markdown);
    assert!(
        f.state
            .search_documents(token(), f.input.bot, "Imported".into())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn tombstones_count_toward_the_root_file_bound_before_creation_or_import() {
    let mut f = fixture();
    manager_only(&mut f);
    for _ in 0..MAX_RECORDS {
        let id = MemoryId::new();
        let path = f.state.directory.join(format!("entry-{id}.md"));
        fs::write(&path, format!("<!-- botstral-memory-entry {} -->\n", serde_json::json!({"type":"tombstone","id":id,"scope":{"type":"shared"},"revision":2}))).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    assert_eq!(
        call(
            &mut f,
            MemoryProposal::Create {
                scope: MemoryScopeSelection::Shared,
                markdown: "No space for another entry.".into()
            }
        ),
        MemoryOutcome::Refused {
            reason: MemoryRefusal::Capacity
        }
    );
    legacy(&f, "No space for legacy import.");
    assert_eq!(
        f.state.memory_context(f.input.bot),
        Err(OperationError::Busy)
    );
    assert_eq!(
        f.state
            .connection
            .query_row::<i64, _, _>("SELECT count(*) FROM memory_imports", [], |r| r.get(0))
            .unwrap(),
        0
    );
}

#[test]
fn followup_uses_frozen_context_and_archived_notices_keep_the_exact_outcome() {
    let mut f = fixture();
    manager_only(&mut f);
    let original = remember(&mut f, 0, "Original context.");
    f.state.submit_run(token(), f.input.clone()).unwrap();
    let first = f.input.id;
    let p = claim(&mut f.state);
    f.state
        .finish_run_call(
            p.run,
            p.turn.id,
            p.turn.attempt,
            response(Some(RunToolCall {
                id: "future-archived-tool-call-secret".into(),
                proposal: ToolProposal::Memory(MemoryProposal::Update {
                    scope: MemoryScopeSelection::Individual,
                    id: original.id,
                    expected_revision: original.revision,
                    markdown: "Corrected context.".into(),
                }),
            })),
        )
        .unwrap();
    let outcome = f.state.load_run(first).unwrap().turns[0]
        .manager_result
        .as_ref()
        .unwrap()
        .outcome
        .clone();
    let followup = claim(&mut f.state);
    assert_eq!(context(&followup), vec![original]);
    f.state
        .finish_run_call(
            followup.run,
            followup.turn.id,
            followup.turn.attempt,
            response(None),
        )
        .unwrap();
    f.input.id = RunId::new();
    let vault_run = f.input.id;
    f.state.submit_run(token(), f.input.clone()).unwrap();
    let vault = claim(&mut f.state);
    f.state
        .finish_run_call(
            vault.run,
            vault.turn.id,
            vault.turn.attempt,
            response(Some(RunToolCall {
                id: "archived-vault".into(),
                proposal: ToolProposal::RequestVaultEntry(RequestVaultEntry {
                    purpose: VaultPurpose::ProviderKey,
                }),
            })),
        )
        .unwrap();
    f.state.cancel_run(token(), vault_run).unwrap();
    for _ in 0..17 {
        f.input.id = RunId::new();
        f.state.submit_run(token(), f.input.clone()).unwrap();
        let next = claim(&mut f.state);
        f.state
            .finish_run_call(next.run, next.turn.id, next.turn.attempt, response(None))
            .unwrap();
        f.state.reconcile_routines().unwrap();
    }
    let archived = f.state.run(token(), first).unwrap();
    assert!(archived.turns.is_empty());
    assert_eq!(archived.archived_turns[0].manager_outcome, outcome);
    assert_eq!(
        WorkspaceRun::project(archived, false).turns[0].manager_outcome,
        outcome
    );
    let archived_vault = WorkspaceRun::project(f.state.run(token(), vault_run).unwrap(), false);
    assert_eq!(
        archived_vault.turns[0].result.as_deref(),
        Some(VaultOutcome::ProviderKeySetup.notice())
    );
    f.state.recover_memory_publications().unwrap();
    assert_eq!(f.state.memory_context(f.input.bot).unwrap()[0].revision, 2);
    f.state
        .replace_provider_key(
            token(),
            ProviderSecret::new("future-archived-tool-call-secret".into()).unwrap(),
        )
        .unwrap();
    assert_eq!(
        f.state.run(token(), first).err(),
        Some(OperationError::InvalidInput)
    );
}
