use super::{
    tasks::{event, json, parse},
    *,
};
use crate::manager::inference::{CallPermit, ProviderOutcome, VerificationPermit};
use botstral_link::*;

#[cfg(test)]
mod tests;

fn sql_revision(revision: u64) -> Result<i64, OperationError> {
    i64::try_from(revision).map_err(|_| OperationError::InvalidInput)
}

fn https_reference(value: &str) -> bool {
    value.len() <= 1024
        && reqwest::Url::parse(value).is_ok_and(|url| {
            url.scheme() == "https"
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
        })
}

fn validate_policy(policy: &InferencePolicy, now: i64) -> Result<(), OperationError> {
    let ack = &policy.acknowledgment;
    let prices = &policy.prices;
    if policy.model_id.is_empty()
        || policy.model_id.len() > 128
        || !policy
            .model_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        || policy.model_id.to_ascii_lowercase().contains("latest")
        || (policy.model_id.to_ascii_lowercase().contains("preview")
            && !matches!(ack.release, ModelReleaseAcknowledgment::Preview { .. }))
        || !(1..=MAX_INFERENCE_CONTEXT_TOKENS).contains(&policy.context_tokens)
        || ack.model_developer.trim().is_empty()
        || ack.model_developer.len() > 200
        || ack.model_developer.chars().any(char::is_control)
        || !https_reference(&ack.developer_terms_url)
        || !https_reference(&ack.privacy_terms_url)
        || ack.acknowledged_at <= 0
        || ack.acknowledged_at > now
        || matches!(&ack.release, ModelReleaseAcknowledgment::Preview { terms_url } if !https_reference(terms_url))
        || prices.effective_at <= 0
        || prices.effective_at > now
        || prices.expires_at <= now
        || prices.input_microusd_per_million_tokens > MAX_COUNTER
        || prices.output_microusd_per_million_tokens > MAX_COUNTER
    {
        return Err(OperationError::InvalidInput);
    }
    Ok(())
}

pub(in crate::manager) fn cost(
    prices: &InferencePrices,
    input: u64,
    output: u64,
) -> Result<u64, OperationError> {
    let priced = |tokens: u64, rate: u64| {
        tokens
            .checked_mul(rate)?
            .checked_add(999_999)
            .map(|n| n / 1_000_000)
    };
    priced(input, prices.input_microusd_per_million_tokens)
        .and_then(|input| {
            priced(output, prices.output_microusd_per_million_tokens)
                .and_then(|output| input.checked_add(output))
        })
        .filter(|n| *n <= MAX_COUNTER)
        .ok_or(OperationError::InvalidInput)
}

fn save(db: &Connection, view: &InferenceView) -> Result<(), OperationError> {
    let (phase, attempt) = match view.phase {
        InferencePhase::Admitted => ("admitted", None),
        InferencePhase::Sending { attempt, .. } => ("sending", Some(attempt)),
        InferencePhase::Completed { attempt, .. } => ("completed", Some(attempt)),
        InferencePhase::Cancelled => ("cancelled", None),
        InferencePhase::NotSent { .. } => ("not_sent", None),
        InferencePhase::AwaitingAttention { attempt, .. } => ("awaiting_attention", Some(attempt)),
    };
    db.execute(
        "UPDATE inferences SET view=?1,phase=?2,attempt=?3 WHERE id=?4",
        params![
            json(view)?,
            phase,
            attempt.map(|a| a.to_string()),
            view.request.id.to_string()
        ],
    )?;
    event(db, "inference_changed", view.request.id)
}

impl State {
    fn provider_epoch(&self) -> Result<i64, OperationError> {
        Ok(self.connection.query_row(
            "SELECT provider_epoch FROM inference_authority",
            [],
            |r| r.get(0),
        )?)
    }

    fn provider_secret(&self) -> Result<SecretString, OperationError> {
        let Installation::Configured {
            vault: VaultState::Unlocked { identity },
        } = &self.installation
        else {
            return Err(OperationError::Locked);
        };
        let encrypted: Vec<u8> = self
            .connection
            .query_row(
                "SELECT ciphertext FROM provider_keys WHERE provider='mistral'",
                [],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(OperationError::NotFound)?;
        let plaintext = crypto::decrypt_provider(identity, &encrypted)?;
        let secret = std::str::from_utf8(&plaintext).map_err(|_| OperationError::Internal)?;
        Ok(SecretString::from(secret.to_owned()))
    }

    fn latest_inference_revision(&self, bot: BotId) -> Result<u64, OperationError> {
        Ok(self.connection.query_row(
            "SELECT coalesce(max(revision),0) FROM inference_configurations WHERE bot=?1",
            [bot.to_string()],
            |r| r.get::<_, i64>(0),
        )? as u64)
    }

    fn load_inference_configuration(
        &self,
        bot: BotId,
        revision: u64,
    ) -> Result<InferenceConfiguration, OperationError> {
        let (policy, checked, until): (String, Option<i64>, Option<i64>) = self.connection.query_row(
            "SELECT policy, verified_at, verified_until FROM inference_configurations WHERE bot=?1 AND revision=?2",
            params![bot.to_string(), sql_revision(revision)?], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?.ok_or(OperationError::NotFound)?;
        Ok(InferenceConfiguration {
            bot,
            revision,
            policy: parse(policy)?,
            verification: match (checked, until) {
                (Some(checked_at), Some(expires_at)) => InferenceVerification::Verified {
                    checked_at,
                    expires_at,
                },
                _ => InferenceVerification::Unverified,
            },
        })
    }

    pub(in crate::manager) fn inference_configuration(
        &self,
        token: SessionToken,
        bot: BotId,
    ) -> Result<InferenceConfiguration, OperationError> {
        self.authorize(&token)?;
        self.load_inference_configuration(bot, self.latest_inference_revision(bot)?)
    }

    pub(in crate::manager) fn configure_inference(
        &mut self,
        token: SessionToken,
        input: ConfigureInference,
    ) -> Result<InferenceConfiguration, OperationError> {
        self.authorize(&token)?;
        validate_policy(&input.policy, self.now())?;
        if !self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM bots WHERE id=?1)",
            [input.bot.to_string()],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(OperationError::NotFound);
        }
        let current = self.latest_inference_revision(input.bot)?;
        if current != input.expected_revision {
            return Err(OperationError::Conflict);
        }
        let count: i64 = self.connection.query_row(
            "SELECT count(*) FROM inference_configurations",
            [],
            |r| r.get(0),
        )?;
        if count >= MAX_INFERENCE_CONFIGURATIONS as i64 {
            return Err(OperationError::Busy);
        }
        let revision = current
            .checked_add(1)
            .filter(|n| *n <= MAX_COUNTER)
            .ok_or(OperationError::Conflict)?;
        let config = InferenceConfiguration {
            bot: input.bot,
            revision,
            policy: input.policy,
            verification: InferenceVerification::Unverified,
        };
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO inference_configurations(bot,revision,policy) VALUES(?1,?2,?3)",
            params![
                config.bot.to_string(),
                sql_revision(revision)?,
                json(&config.policy)?
            ],
        )?;
        event(&tx, "inference_configured", config.bot)?;
        tx.commit()?;
        Ok(config)
    }

    pub(in crate::manager) fn prepare_inference_verification(
        &mut self,
        token: SessionToken,
        input: VerifyInference,
    ) -> Result<VerificationPermit, OperationError> {
        self.authorize(&token)?;
        if self.latest_inference_revision(input.bot)? != input.revision {
            return Err(OperationError::Conflict);
        }
        let config = self.load_inference_configuration(input.bot, input.revision)?;
        validate_policy(&config.policy, self.now())?;
        let secret = self.provider_secret()?;
        let epoch = self.provider_epoch()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("UPDATE inference_configurations SET verified_at=NULL,verified_until=NULL,provider_epoch=NULL WHERE bot=?1 AND revision=?2", params![input.bot.to_string(), sql_revision(input.revision)?])?;
        event(&tx, "inference_verification_started", input.bot)?;
        tx.commit()?;
        Ok(VerificationPermit {
            configuration: config,
            secret,
            epoch,
        })
    }

    pub(in crate::manager) fn finish_inference_verification(
        &mut self,
        config: InferenceConfiguration,
        epoch: i64,
        verified: bool,
    ) -> Result<InferenceConfiguration, OperationError> {
        if self.latest_inference_revision(config.bot)? != config.revision
            || self.provider_epoch()? != epoch
        {
            return Err(OperationError::Conflict);
        }
        if !matches!(
            self.installation,
            Installation::Configured {
                vault: VaultState::Unlocked { .. }
            }
        ) {
            return Err(OperationError::Locked);
        }
        let now = self.now();
        validate_policy(&config.policy, now)?;
        if !verified {
            return Err(OperationError::InferenceUnverified);
        }
        let until = (now + 3600).min(config.policy.prices.expires_at);
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("UPDATE inference_configurations SET verified_at=?1,verified_until=?2,provider_epoch=?3 WHERE bot=?4 AND revision=?5", params![now, until, epoch, config.bot.to_string(), sql_revision(config.revision)?])?;
        event(&tx, "inference_verified", config.bot)?;
        tx.commit()?;
        self.load_inference_configuration(config.bot, config.revision)
    }

    fn configuration_authorized(
        &self,
        config: &InferenceConfiguration,
    ) -> Result<bool, OperationError> {
        if self.latest_inference_revision(config.bot)? != config.revision {
            return Ok(false);
        }
        let current = self.load_inference_configuration(config.bot, config.revision)?;
        let epoch: Option<i64> = self.connection.query_row(
            "SELECT provider_epoch FROM inference_configurations WHERE bot=?1 AND revision=?2",
            params![config.bot.to_string(), sql_revision(config.revision)?],
            |r| r.get(0),
        )?;
        Ok(current.policy == config.policy
            && epoch == Some(self.provider_epoch()?)
            && matches!(current.verification, InferenceVerification::Verified { expires_at, .. } if expires_at > self.now())
            && current.policy.prices.expires_at > self.now())
    }

    fn load_inference(&self, id: InferenceId) -> Result<InferenceView, OperationError> {
        parse(
            self.connection
                .query_row(
                    "SELECT view FROM inferences WHERE id=?1",
                    [id.to_string()],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or(OperationError::NotFound)?,
        )
    }

    pub(in crate::manager) fn inference(
        &self,
        token: SessionToken,
        id: InferenceId,
    ) -> Result<InferenceView, OperationError> {
        self.authorize(&token)?;
        self.load_inference(id)
    }

    pub(in crate::manager) fn submit_inference(
        &mut self,
        token: SessionToken,
        input: SubmitInference,
    ) -> Result<InferenceView, OperationError> {
        self.authorize(&token)?;
        match self.load_inference(input.id) {
            Ok(previous) => {
                return if previous.request == input {
                    Ok(previous)
                } else {
                    Err(OperationError::Conflict)
                };
            }
            Err(OperationError::NotFound) => {}
            Err(error) => return Err(error),
        }
        let now = self.now();
        if input.prompt.is_empty()
            || input.prompt.len() > MAX_INFERENCE_PROMPT_BYTES
            || input.deadline <= now
            || input.deadline > now + MAX_INFERENCE_SECONDS
            || !(1..=MAX_INFERENCE_OUTPUT_TOKENS).contains(&input.max_output_tokens)
            || input.max_total_tokens > MAX_COUNTER
            || input.max_cost_microusd > MAX_COUNTER
        {
            return Err(OperationError::InvalidInput);
        }
        let config = self.load_inference_configuration(input.bot, input.configuration_revision)?;
        if !self.configuration_authorized(&config)?
            || input.deadline > config.policy.prices.expires_at
        {
            return Err(OperationError::InferenceUnverified);
        }
        let total_tokens = u64::from(config.policy.context_tokens)
            .checked_add(u64::from(input.max_output_tokens))
            .ok_or(OperationError::InvalidInput)?;
        let reserved_cost = cost(
            &config.policy.prices,
            config.policy.context_tokens.into(),
            input.max_output_tokens.into(),
        )?;
        if input.max_output_tokens >= config.policy.context_tokens
            || input.max_total_tokens < total_tokens
            || input.max_cost_microusd < reserved_cost
        {
            return Err(OperationError::InferenceBudget);
        }
        self.provider_secret()?;
        let (count, active): (i64, i64) = self.connection.query_row(
            "SELECT count(*),coalesce(sum(phase IN ('admitted','sending')),0) FROM inferences",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if count >= MAX_INFERENCES as i64 || active >= MAX_INFERENCE_CALLS as i64 {
            return Err(OperationError::Busy);
        }
        let view = InferenceView {
            request: input,
            configuration: config,
            phase: InferencePhase::Admitted,
            reservation: InferenceReservation {
                total_tokens,
                cost_microusd: reserved_cost,
            },
            accounting: InferenceAccounting::Held,
            created_at: now,
            updated_at: now,
            cancellation_requested_at: None,
            usage: None,
            provider_request_id: None,
            output: None,
        };
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("INSERT INTO inferences(id,bot,configuration_revision,request,view,phase) VALUES(?1,?2,?3,?4,?5,'admitted')", params![view.request.id.to_string(), view.request.bot.to_string(), sql_revision(view.request.configuration_revision)?, json(&view.request)?,json(&view)?])?;
        event(&tx, "inference_admitted", view.request.id)?;
        tx.commit()?;
        Ok(view)
    }

    pub(in crate::manager) fn cancel_inference(
        &mut self,
        token: SessionToken,
        id: InferenceId,
    ) -> Result<InferenceView, OperationError> {
        self.authorize(&token)?;
        let mut view = self.load_inference(id)?;
        if view.cancellation_requested_at.is_some()
            || !matches!(
                view.phase,
                InferencePhase::Admitted | InferencePhase::Sending { .. }
            )
        {
            return Ok(view);
        }
        view.cancellation_requested_at = Some(self.now());
        view.updated_at = self.now();
        if matches!(view.phase, InferencePhase::Admitted) {
            view.phase = InferencePhase::Cancelled;
            view.accounting = InferenceAccounting::Released;
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        save(&tx, &view)?;
        tx.commit()?;
        Ok(view)
    }

    pub(in crate::manager) fn claim_inference(
        &mut self,
    ) -> Result<Option<CallPermit>, OperationError> {
        let sending: i64 = self.connection.query_row(
            "SELECT count(*) FROM inferences WHERE phase='sending'",
            [],
            |r| r.get(0),
        )?;
        if sending >= MAX_INFERENCE_CALLS as i64 {
            return Ok(None);
        }
        let pending = self.inference_views(Some("admitted"))?;
        for mut view in pending {
            let reason = if view.request.deadline <= self.now() {
                Some(InferenceNotSentReason::Deadline)
            } else if !self.configuration_authorized(&view.configuration)? {
                Some(InferenceNotSentReason::ConfigurationChanged)
            } else if !matches!(
                self.installation,
                Installation::Configured {
                    vault: VaultState::Unlocked { .. }
                }
            ) {
                Some(InferenceNotSentReason::VaultLocked)
            } else {
                None
            };
            view.updated_at = self.now();
            if let Some(reason) = reason {
                view.phase = InferencePhase::NotSent { reason };
                view.accounting = InferenceAccounting::Released;
                let tx = self
                    .connection
                    .transaction_with_behavior(TransactionBehavior::Immediate)?;
                save(&tx, &view)?;
                tx.commit()?;
                continue;
            }
            let secret = self.provider_secret()?;
            let attempt = InferenceAttemptId::new();
            view.phase = InferencePhase::Sending {
                attempt,
                started_at: self.now(),
            };
            let tx = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            save(&tx, &view)?;
            tx.commit()?;
            return Ok(Some(CallPermit {
                view,
                attempt,
                secret,
            }));
        }
        Ok(None)
    }

    pub(in crate::manager) fn finish_inference(
        &mut self,
        id: InferenceId,
        attempt: InferenceAttemptId,
        outcome: ProviderOutcome,
    ) -> Result<(), OperationError> {
        let mut view = self.load_inference(id)?;
        if !matches!(view.phase, InferencePhase::Sending { attempt: current, .. } if current == attempt)
        {
            return Ok(());
        }
        match outcome {
            ProviderOutcome::NotSent { reason } => {
                view.phase = InferencePhase::NotSent { reason };
                view.accounting = InferenceAccounting::Released;
            }
            ProviderOutcome::Completed {
                usage,
                output,
                request_id,
                finish_reason,
            } => {
                let charged = cost(
                    &view.configuration.policy.prices,
                    usage.prompt_tokens,
                    usage.completion_tokens,
                )?;
                view.accounting = InferenceAccounting::Settled {
                    charged_microusd: charged,
                };
                view.usage = Some(usage);
                view.output = Some(output);
                view.provider_request_id = Some(request_id);
                view.phase = InferencePhase::Completed {
                    attempt,
                    finish_reason,
                };
            }
            ProviderOutcome::Unknown { reason, request_id } => {
                view.provider_request_id = request_id;
                view.phase = InferencePhase::AwaitingAttention { attempt, reason };
            }
        }
        view.updated_at = self.now();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        save(&tx, &view)?;
        tx.commit()?;
        Ok(())
    }

    pub(super) fn inference_views(
        &self,
        phase: Option<&str>,
    ) -> Result<Vec<InferenceView>, OperationError> {
        self.connection
            .prepare("SELECT view FROM inferences WHERE (?1 IS NULL OR phase=?1) ORDER BY rowid")?
            .query_map([phase], |r| r.get::<_, String>(0))?
            .map(|r| parse(r?))
            .collect()
    }

    pub(super) fn inference_configurations(
        &self,
    ) -> Result<Vec<InferenceConfiguration>, OperationError> {
        self.connection
            .prepare("SELECT bot,revision FROM inference_configurations ORDER BY bot,revision")?
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
            })?
            .map(|r| {
                let (bot, rev) = r?;
                self.load_inference_configuration(
                    bot.parse().map_err(|_| OperationError::Internal)?,
                    rev,
                )
            })
            .collect()
    }

    pub(in crate::manager) fn recover_inferences(
        &mut self,
        shutdown: bool,
    ) -> Result<(), OperationError> {
        let now = self.now();
        let views = self.inference_views(None)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for mut view in views {
            match view.phase {
                InferencePhase::Sending { attempt, .. } => {
                    view.phase = InferencePhase::AwaitingAttention {
                        attempt,
                        reason: if shutdown {
                            InferenceAttentionReason::Shutdown
                        } else {
                            InferenceAttentionReason::Restart
                        },
                    }
                }
                InferencePhase::Admitted => {
                    view.phase = InferencePhase::NotSent {
                        reason: if shutdown {
                            InferenceNotSentReason::Shutdown
                        } else {
                            InferenceNotSentReason::Restart
                        },
                    };
                    view.accounting = InferenceAccounting::Released;
                }
                _ => continue,
            }
            view.updated_at = now;
            save(&tx, &view)?;
        }
        tx.commit()?;
        Ok(())
    }
}
