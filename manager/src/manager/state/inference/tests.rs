use super::*;

pub(in crate::manager::state) fn token() -> SessionToken {
    SessionToken::new("x".repeat(43)).unwrap()
}
pub(in crate::manager::state) fn fixture() -> (tempfile::TempDir, State, SubmitInference) {
    let root = tempfile::tempdir().unwrap();
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    db.execute_batch(
        "INSERT INTO owner VALUES(1,'Owner','unused'); INSERT INTO vault VALUES(1,1,X'00');",
    )
    .unwrap();
    let bot = BotId::new();
    db.execute("INSERT INTO bots VALUES(?1,'Bot',1)", [bot.to_string()])
        .unwrap();
    let (configured, _) = watch::channel(false);
    let mut state = State::from_connection(db, root.path().into(), configured).unwrap();
    state.test_time = Some(1000);
    state.installation = Installation::Configured {
        vault: VaultState::Unlocked {
            identity: age::x25519::Identity::generate(),
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
        .replace_provider_key(
            token(),
            ProviderSecret::new("synthetic-state-inference-secret".into()).unwrap(),
        )
        .unwrap();
    let input = ConfigureInference {
        bot,
        expected_revision: 0,
        policy: InferencePolicy {
            endpoint: InferenceEndpoint::Eu,
            model_id: "mistral-small-2506".into(),
            context_tokens: 32768,
            acknowledgment: InferenceAcknowledgment {
                model_developer: "Mistral AI".into(),
                developer_terms_url: "https://mistral.ai/terms/".into(),
                privacy_terms_url: "https://mistral.ai/privacy/".into(),
                acknowledged_at: 1000,
                release: ModelReleaseAcknowledgment::Stable,
            },
            prices: InferencePrices {
                input_microusd_per_million_tokens: 1_000_000,
                output_microusd_per_million_tokens: 2_000_000,
                effective_at: 999,
                expires_at: 10000,
                billing: InferenceBilling::TextTokensOnlyAtAcknowledgedCeilings,
            },
        },
    };
    state.advertise_for_test();
    state.configure_inference(token(), input).unwrap();
    let permit = state
        .prepare_inference_verification(token(), VerifyInference { bot, revision: 1 })
        .unwrap();
    state
        .finish_inference_verification(permit.configuration, permit.epoch, Some((true, false)))
        .unwrap();
    let request = SubmitInference {
        id: InferenceId::new(),
        bot,
        configuration_revision: 1,
        prompt: "One turn.".into(),
        deadline: 1100,
        max_output_tokens: 64,
        max_total_tokens: 32832,
        max_cost_microusd: 32896,
    };
    (root, state, request)
}
fn completion() -> ProviderOutcome {
    ProviderOutcome::Completed {
        usage: InferenceUsage {
            prompt_tokens: 12,
            completion_tokens: 8,
            total_tokens: 20,
        },
        output: "Completed.".into(),
        request_id: "cmpl-state-fixture".into(),
        finish_reason: InferenceFinishReason::Stop,
    }
}

#[test]
fn m04_ledger_transitions_and_events_commit_together_and_settle_once() {
    let (_root, mut state, request) = fixture();
    state.connection.execute_batch("CREATE TRIGGER reject_admission BEFORE INSERT ON events WHEN NEW.kind='inference_admitted' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert_eq!(
        state.submit_inference(token(), request.clone()),
        Err(OperationError::Internal)
    );
    assert_eq!(
        state
            .connection
            .query_row::<i64, _, _>("SELECT count(*) FROM inferences", [], |r| r.get(0))
            .unwrap(),
        0
    );
    state
        .connection
        .execute_batch("DROP TRIGGER reject_admission")
        .unwrap();
    let admitted = state.submit_inference(token(), request.clone()).unwrap();
    state.connection.execute_batch("CREATE TRIGGER reject_transition BEFORE INSERT ON events WHEN NEW.kind='inference_changed' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(matches!(
        state.claim_inference(),
        Err(OperationError::Internal)
    ));
    assert_eq!(state.load_inference(request.id).unwrap(), admitted);
    assert_eq!(
        state.cancel_inference(token(), request.id),
        Err(OperationError::Internal)
    );
    assert_eq!(state.load_inference(request.id).unwrap(), admitted);
    state
        .connection
        .execute_batch("DROP TRIGGER reject_transition")
        .unwrap();
    let permit = state.claim_inference().unwrap().unwrap();
    let sending = state.load_inference(request.id).unwrap();
    state.connection.execute_batch("CREATE TRIGGER reject_transition BEFORE INSERT ON events BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert_eq!(
        state.finish_inference(request.id, permit.attempt, completion()),
        Err(OperationError::Internal)
    );
    assert_eq!(state.load_inference(request.id).unwrap(), sending);
    state
        .connection
        .execute_batch("DROP TRIGGER reject_transition")
        .unwrap();
    state
        .finish_inference(request.id, InferenceAttemptId::new(), completion())
        .unwrap();
    assert_eq!(state.load_inference(request.id).unwrap(), sending);
    state
        .finish_inference(request.id, permit.attempt, completion())
        .unwrap();
    let completed = state.load_inference(request.id).unwrap();
    assert_eq!(
        completed.accounting,
        InferenceAccounting::Settled {
            charged_microusd: 28
        }
    );
    state
        .finish_inference(
            request.id,
            permit.attempt,
            ProviderOutcome::Unknown {
                reason: InferenceAttentionReason::Interrupted,
                request_id: None,
            },
        )
        .unwrap();
    assert_eq!(state.load_inference(request.id).unwrap(), completed);
    state.test_time = Some(2000);
    assert_eq!(
        state.submit_inference(token(), request.clone()).unwrap(),
        completed
    );
    assert!(
        state
            .connection
            .execute("UPDATE inferences SET request='{}'", [])
            .is_err()
    );
    assert!(
        state
            .connection
            .execute("UPDATE inference_configurations SET policy='{}'", [])
            .is_err()
    );
}

#[test]
fn m04_price_rounding_overflow_and_verification_authority() {
    let (_root, mut state, request) = fixture();
    let mut prices = state
        .load_inference_configuration(request.bot, 1)
        .unwrap()
        .policy
        .prices;
    prices.input_microusd_per_million_tokens = 1;
    prices.output_microusd_per_million_tokens = 1;
    assert_eq!(cost(&prices, 1, 1), Ok(2));
    assert_eq!(cost(&prices, 0, 0), Ok(0));
    prices.input_microusd_per_million_tokens = MAX_COUNTER;
    assert_eq!(
        cost(&prices, MAX_INFERENCE_CONTEXT_TOKENS.into(), 1),
        Err(OperationError::InvalidInput)
    );
    let permit = state
        .prepare_inference_verification(
            token(),
            VerifyInference {
                bot: request.bot,
                revision: 1,
            },
        )
        .unwrap();
    state
        .replace_provider_key(
            token(),
            ProviderSecret::new("synthetic-replacement-secret".into()).unwrap(),
        )
        .unwrap();
    assert_eq!(
        state.finish_inference_verification(
            permit.configuration,
            permit.epoch,
            Some((true, false))
        ),
        Err(OperationError::Conflict)
    );
    assert_eq!(
        state.submit_inference(token(), request.clone()),
        Err(OperationError::InferenceUnverified)
    );
    let permit = state
        .prepare_inference_verification(
            token(),
            VerifyInference {
                bot: request.bot,
                revision: 1,
            },
        )
        .unwrap();
    state.lock_vault(token()).unwrap();
    assert_eq!(
        state.finish_inference_verification(
            permit.configuration,
            permit.epoch,
            Some((true, false))
        ),
        Err(OperationError::Locked)
    );
}
