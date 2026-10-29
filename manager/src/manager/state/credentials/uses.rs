use super::*;
use crate::worker_http::VerifiedIdentity;
use botstral_link::protected::{
    BrowserAuthority, FillValues, ProtectedBrowserOperation, ProtectedBrowserReply,
    ProtectedBrowserRequest,
};

pub(super) fn contract(destination: &LiveDestination) -> Result<String, OperationError> {
    json(&(
        &destination.top_origin,
        &destination.frame_origin,
        &destination.plan,
        destination
            .fields
            .iter()
            .map(|f| (&f.role, &f.name, &f.input_type))
            .collect::<Vec<_>>(),
    ))
}
fn save(db: &Connection, review: &CredentialUseReview) -> Result<(), OperationError> {
    db.execute("INSERT INTO credential_uses VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET view=excluded.view", params![review.id.to_string(), review.release.to_string(), json(review)?])?;
    event(db, "credential_use_changed", review.id)
}
fn active(state: UseState) -> bool {
    matches!(
        state,
        UseState::AwaitingApproval | UseState::Authorized | UseState::Released
    )
}
pub(super) fn invalidate(
    db: &Connection,
    item: Option<CredentialId>,
) -> Result<(), OperationError> {
    let reviews = db
        .prepare("SELECT view FROM credential_uses")?
        .query_map([], |r| r.get::<_, String>(0))?
        .map(|r| parse::<CredentialUseReview>(r?))
        .collect::<Result<Vec<_>, _>>()?;
    for mut review in reviews
        .into_iter()
        .filter(|r| item.is_none_or(|id| r.item.id == id) && active(r.state))
    {
        if review.state == UseState::Released {
            review.state = UseState::Uncertain;
            let body: String = db.query_row(
                "SELECT view FROM tasks WHERE id=?1",
                [review.binding.task.to_string()],
                |r| r.get(0),
            )?;
            let mut task: TaskView = parse(body)?;
            if tasks::execution_pending(&task) {
                task.state = TaskState::Cancelling;
                db.execute(
                    "INSERT OR IGNORE INTO cancellations VALUES(?1,?2)",
                    params![task.id.to_string(), CommandId::new().to_string()],
                )?;
                tasks::save(db, &task)?;
            }
        } else {
            review.state = UseState::Revoked;
        }
        save(db, &review)?;
    }
    Ok(())
}

impl State {
    pub(in crate::manager::state) fn recover_credential_uses(
        &mut self,
    ) -> Result<(), OperationError> {
        let tx = self.connection.transaction()?;
        invalidate(&tx, None)?;
        tx.commit()?;
        Ok(())
    }
    fn load_credential_use(
        &self,
        id: CredentialUseId,
    ) -> Result<CredentialUseReview, OperationError> {
        parse(
            self.connection
                .query_row(
                    "SELECT view FROM credential_uses WHERE id=?1",
                    [id.to_string()],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or(OperationError::NotFound)?,
        )
    }
    fn browser_parent(&self, authority: &BrowserAuthority) -> Result<TaskView, OperationError> {
        let task = self.load_task(authority.task)?;
        self.task_gateway_authority(&task, authority.fence.generation)?;
        self.check_input_fence(task.command.worker, authority.fence)?;
        let journal: String = self.connection.query_row(
            "SELECT incarnation FROM worker_journals WHERE worker=?1",
            [task.command.worker.to_string()],
            |r| r.get(0),
        )?;
        if task.command.execution != authority.execution
            || task.command.session != authority.fence.session_id
            || authority.fence.owner != InputOwner::Bot
            || journal != authority.journal.to_string()
            || !matches!(task.command.work, WorkSpec::Computer { .. })
        {
            return Err(OperationError::Unauthorized);
        }
        Ok(task)
    }
    fn use_authority(&self, review: &CredentialUseReview) -> Result<TaskView, OperationError> {
        let task = self.browser_parent(&BrowserAuthority {
            task: review.binding.task,
            execution: review.binding.execution,
            fence: review.fence,
            journal: review.incarnation,
        })?;
        self.credential_identity()?;
        let item = self.load_login(review.item.id)?;
        if task.approval.binding.as_ref() != Some(&review.binding)
            || item != review.item
            || item.state != CredentialState::Active
            || !item.metadata.bots.permits(task.command.bot)
            || !item
                .metadata
                .allowed_origins
                .contains(&review.destination.top_origin)
            || !item
                .metadata
                .allowed_origins
                .contains(&review.destination.frame_origin)
            || review.expires_at <= self.now()
            || (review.purpose == CredentialFields::Totp && !item.has_totp)
        {
            return Err(OperationError::Unauthorized);
        }
        if self.rule(review)?.is_some_and(|(rule, _)| rule == "deny") {
            return Err(OperationError::Unauthorized);
        }
        Ok(task)
    }
    fn rule(
        &self,
        review: &CredentialUseReview,
    ) -> Result<Option<(String, String)>, OperationError> {
        Ok(self.connection.query_row("SELECT decision,cause FROM credential_rules WHERE item=?1 AND bot=?2 AND contract=?3", params![review.item.id.to_string(), review.binding.bot.to_string(), contract(&review.destination)?], |r| Ok((r.get(0)?, r.get(1)?))).optional()?)
    }
    pub(in crate::manager) fn credential_uses(
        &mut self,
        authority: impl Into<OwnerAuthority>,
    ) -> Result<Vec<CredentialUseReview>, OperationError> {
        self.authorize_owner(&authority.into())?;
        let mut reviews = self
            .connection
            .prepare("SELECT view FROM credential_uses ORDER BY rowid DESC LIMIT 128")?
            .query_map([], |r| r.get::<_, String>(0))?
            .map(|r| parse::<CredentialUseReview>(r?))
            .collect::<Result<Vec<_>, _>>()?;
        for review in &mut reviews {
            self.reconcile_credential_use(review)?;
        }
        self.reject_known_secrets(&reviews)?;
        Ok(reviews)
    }
    fn reconcile_credential_use(
        &self,
        review: &mut CredentialUseReview,
    ) -> Result<(), OperationError> {
        if active(review.state) && self.use_authority(review).is_err() {
            review.state = if review.state == UseState::Released {
                UseState::Uncertain
            } else if review.expires_at <= self.now() {
                UseState::Expired
            } else {
                UseState::Revoked
            };
            save(&self.connection, review)?;
        }
        Ok(())
    }
    pub(in crate::manager) fn decide_credential_use(
        &mut self,
        authority: impl Into<OwnerAuthority>,
        expected: CredentialUseReview,
        decision: UseDecision,
    ) -> Result<CredentialUseReview, OperationError> {
        self.authorize_owner(&authority.into())?;
        let mut review = self.load_credential_use(expected.id)?;
        if review != expected || review.state != UseState::AwaitingApproval {
            return Err(OperationError::Conflict);
        }
        self.use_authority(&review)?;
        if decision == UseDecision::RememberPassword
            && (review.purpose != CredentialFields::Password
                || review.item.metadata.password_policy != PasswordPolicy::RememberedOrigins
                || !review.destination.certain)
        {
            return Err(OperationError::InvalidInput);
        }
        if decision != UseDecision::AllowOnce {
            self.credential_capacity("credential_rules", 256)?;
        }
        let tx = self.connection.transaction()?;
        if decision == UseDecision::AllowOnce {
            review.state = UseState::Authorized;
        } else {
            let mut item = review.item.clone();
            item.revision = advance(item.revision)?;
            invalidate(&tx, Some(item.id))?;
            tx.execute(
                "UPDATE credential_items SET view=?2 WHERE id=?1",
                params![item.id.to_string(), json(&item)?],
            )?;
            tx.execute("INSERT INTO credential_rules VALUES(?1,?2,?3,?4,?5) ON CONFLICT(item,bot,contract) DO UPDATE SET decision=excluded.decision,cause=excluded.cause", params![item.id.to_string(), review.binding.bot.to_string(), contract(&review.destination)?, if decision == UseDecision::Deny { "deny" } else { "allow" }, review.id.to_string()])?;
            review.state = if decision == UseDecision::Deny {
                UseState::Denied
            } else {
                UseState::Revoked
            };
        }
        save(&tx, &review)?;
        tx.commit()?;
        Ok(review)
    }
    fn prepare_credential_use(
        &mut self,
        authority: &BrowserAuthority,
        destination: LiveDestination,
        previous: Option<CredentialUseId>,
    ) -> Result<CredentialUseReview, OperationError> {
        let task = self.browser_parent(authority)?;
        self.credential_identity()?;
        self.reject_known_secrets(&destination)?;
        destination
            .plan
            .validate()
            .map_err(OperationError::Protocol)?;
        if canonical_origin(&destination.top_origin)? != destination.plan.origin
            || destination.frame_origin != destination.top_origin
            || destination.fields.is_empty()
            || destination.fields.len() > 2
            || [&destination.form, &destination.submitter]
                .into_iter()
                .any(|id| id.parse::<RequestId>().is_err())
            || destination.fields.iter().any(|field| {
                field.name.is_empty()
                    || field.name.len() > 128
                    || field.name.chars().any(char::is_control)
            })
        {
            return Err(OperationError::InvalidInput);
        }
        let WorkSpec::Computer { spec } = &task.command.work else {
            return Err(OperationError::InvalidInput);
        };
        let [
            ComputerOperation::ProtectedLogin {
                item,
                revision,
                purpose,
                plan,
            },
        ] = spec.operations.as_slice()
        else {
            return Err(OperationError::InvalidInput);
        };
        if **plan != destination.plan
            || destination.fields.len()
                != if *purpose == CredentialFields::Password {
                    2
                } else {
                    1
                }
        {
            return Err(OperationError::InvalidInput);
        }
        let item = self.load_login(*item)?;
        let shape = match purpose {
            CredentialFields::Password => {
                destination.fields.len() == 2
                    && destination.fields[0].role == LoginFieldRole::Username
                    && matches!(
                        destination.fields[0].input_type.as_str(),
                        "text" | "email" | "tel"
                    )
                    && destination.fields[1].role == LoginFieldRole::Password
                    && destination.fields[1].input_type == "password"
            }
            CredentialFields::Totp => {
                destination.fields.len() == 1
                    && destination.fields[0].role == LoginFieldRole::Totp
                    && destination.fields[0].input_type == "text"
            }
        };
        if !shape {
            return Err(OperationError::InvalidInput);
        }
        let binding = task
            .approval
            .binding
            .clone()
            .ok_or(OperationError::Unauthorized)?;
        let expires_at = (self.now() + 30).min(binding.expires_at).min(spec.deadline);
        let mut review = CredentialUseReview {
            id: CredentialUseId::new(),
            release: ReleaseId::new(),
            item,
            purpose: *purpose,
            binding,
            fence: authority.fence,
            incarnation: authority.journal,
            destination,
            expires_at,
            state: UseState::AwaitingApproval,
        };
        if review.item.revision != *revision {
            let prior = self.load_credential_use(previous.ok_or(OperationError::Conflict)?)?;
            if prior.state != UseState::Revoked
                || prior.destination != review.destination
                || prior.binding != review.binding
                || prior.fence != review.fence
                || prior.incarnation != review.incarnation
                || prior.purpose != review.purpose
                || prior.item.id != review.item.id
                || prior.item.epoch != review.item.epoch
                || advance(prior.item.revision)? != review.item.revision
                || self.rule(&review)? != Some(("allow".into(), prior.id.to_string()))
            {
                return Err(OperationError::Conflict);
            }
        }
        self.use_authority(&review)?;
        if review.purpose == CredentialFields::Password
            && review.item.metadata.password_policy == PasswordPolicy::RememberedOrigins
            && review.destination.certain
            && self
                .rule(&review)?
                .is_some_and(|(decision, _)| decision == "allow")
        {
            review.state = UseState::Authorized;
        }
        self.credential_capacity("credential_uses", MAX_CREDENTIAL_USES)?;
        save(&self.connection, &review)?;
        Ok(review)
    }
    fn release_credential(
        &mut self,
        mut review: CredentialUseReview,
        destination: LiveDestination,
        protection: ProfileProtection,
    ) -> Result<ProtectedBrowserReply, OperationError> {
        if review.state != UseState::Authorized || review.destination != destination {
            return Err(OperationError::Conflict);
        }
        self.use_authority(&review)?;
        let ProfileProtection::Tracked {
            profile, releases, ..
        } = protection
        else {
            return Err(OperationError::Conflict);
        };
        if profile != destination.profile
            || !releases.contains(&review.release)
            || releases.len() > MAX_CREDENTIAL_USES
        {
            return Err(OperationError::Conflict);
        }
        let cipher: Vec<u8> = self.connection.query_row(
            "SELECT ciphertext FROM credential_epochs WHERE item=?1 AND epoch=?2",
            params![review.item.id.to_string(), review.item.epoch.get() as i64],
            |r| r.get(0),
        )?;
        let values = self.login_values(review.item.id, review.item.epoch, &cipher)?;
        let mut issued = None;
        let (values, valid_until) = match review.purpose {
            CredentialFields::Password => (
                FillValues::Password {
                    username: Zeroizing::new(values.username.clone()),
                    password: Zeroizing::new(values.password.clone()),
                },
                review.expires_at,
            ),
            CredentialFields::Totp => {
                self.credential_capacity("credential_codes", 512)?;
                let (code, valid_until) = totp::current(&values.totp_seed, self.now())?;
                let until = valid_until.min(review.expires_at);
                if until - self.now() < 5 {
                    return Err(OperationError::Conflict);
                }
                issued = Some(crypto::encrypt_credential(
                    self.credential_identity()?,
                    &self.code_context(&review)?,
                    code.as_bytes(),
                )?);
                (FillValues::Totp { code }, until)
            }
        };
        review.state = UseState::Released;
        let tx = self.connection.transaction()?;
        if let Some(ciphertext) = issued {
            tx.execute(
                "INSERT INTO credential_codes VALUES(?1,?2)",
                params![review.release.to_string(), ciphertext],
            )?;
        }
        save(&tx, &review)?;
        tx.commit()?;
        Ok(ProtectedBrowserReply::Released {
            release: review.release,
            values,
            valid_until,
        })
    }
    fn code_context(&self, review: &CredentialUseReview) -> Result<String, OperationError> {
        Ok(format!(
            "{}\0{}\0issued-code",
            self.credential_context(review.item.id, review.item.epoch)?,
            review.release
        ))
    }
    pub(super) fn known_issued_codes(&self) -> Result<Vec<SecretString>, OperationError> {
        let rows = self
            .connection
            .prepare(
                "SELECT view,ciphertext FROM credential_codes JOIN credential_uses USING(release)",
            )?
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(view, cipher)| {
                let review: CredentialUseReview = parse(view)?;
                let bytes = crypto::decrypt_credential(
                    self.credential_identity()?,
                    &self.code_context(&review)?,
                    &cipher,
                )?;
                Ok(SecretString::from(
                    std::str::from_utf8(&bytes)
                        .map_err(|_| OperationError::Internal)?
                        .to_owned(),
                ))
            })
            .collect()
    }
    fn screen_profile(
        &self,
        protection: ProfileProtection,
        text: Zeroizing<String>,
    ) -> Result<ProtectedBrowserReply, OperationError> {
        let ProfileProtection::Tracked {
            profile,
            releases,
            complete: true,
        } = protection
        else {
            return Err(OperationError::Conflict);
        };
        if releases.len() > MAX_CREDENTIAL_USES || text.len() > 32768 {
            return Err(OperationError::InvalidInput);
        }
        for release in releases {
            let view: String = self
                .connection
                .query_row(
                    "SELECT view FROM credential_uses WHERE release=?1",
                    [release.to_string()],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or(OperationError::Conflict)?;
            let review: CredentialUseReview = parse(view)?;
            if review.destination.profile != profile {
                return Err(OperationError::Conflict);
            }
            let cipher: Vec<u8> = self.connection.query_row(
                "SELECT ciphertext FROM credential_epochs WHERE item=?1 AND epoch=?2",
                params![review.item.id.to_string(), review.item.epoch.get() as i64],
                |r| r.get(0),
            )?;
            self.login_values(review.item.id, review.item.epoch, &cipher)?;
            if review.purpose == CredentialFields::Totp {
                let exists: bool = self.connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM credential_codes WHERE release=?1)",
                    [release.to_string()],
                    |r| r.get(0),
                )?;
                if !exists {
                    return Err(OperationError::Conflict);
                }
            }
        }
        self.reject_known_secrets(&*text)?;
        Ok(ProtectedBrowserReply::Screened { text })
    }
    pub(in crate::manager) fn protected_browser(
        &mut self,
        peer: VerifiedIdentity,
        request: ProtectedBrowserRequest,
    ) -> Result<ProtectedBrowserReply, OperationError> {
        let task = self.browser_parent(&request.authority)?;
        let valid: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM workers WHERE id=?1 AND fingerprint=?2 AND revoked=0)",
            params![task.command.worker.to_string(), peer.fingerprint().as_str()],
            |r| r.get(0),
        )?;
        if !valid {
            return Err(OperationError::Unauthorized);
        }
        match request.operation {
            ProtectedBrowserOperation::Prepare {
                destination,
                previous,
            } => Ok(ProtectedBrowserReply::Review {
                review: Box::new(self.prepare_credential_use(
                    &request.authority,
                    destination,
                    previous,
                )?),
            }),
            ProtectedBrowserOperation::Screen { protection, text } => {
                self.screen_profile(protection, text)
            }
            operation => {
                let id = match &operation {
                    ProtectedBrowserOperation::Poll { id }
                    | ProtectedBrowserOperation::Release { id, .. }
                    | ProtectedBrowserOperation::Settle { id, .. } => *id,
                    _ => unreachable!(),
                };
                let mut review = self.load_credential_use(id)?;
                if review.binding.task != task.id
                    || review.binding.execution != request.authority.execution
                    || review.fence != request.authority.fence
                    || review.incarnation != request.authority.journal
                {
                    return Err(OperationError::Unauthorized);
                }
                match operation {
                    ProtectedBrowserOperation::Release {
                        destination,
                        protection,
                        ..
                    } => self.release_credential(review, destination, protection),
                    ProtectedBrowserOperation::Poll { .. } => {
                        self.reconcile_credential_use(&mut review)?;
                        Ok(ProtectedBrowserReply::Review {
                            review: Box::new(review),
                        })
                    }
                    ProtectedBrowserOperation::Settle { applied, .. } => {
                        let next = if applied {
                            UseState::Applied
                        } else {
                            UseState::Uncertain
                        };
                        if review.state == UseState::Released {
                            review.state = next;
                            save(&self.connection, &review)?;
                        } else if review.state != next {
                            return Err(OperationError::Conflict);
                        }
                        Ok(ProtectedBrowserReply::Review {
                            review: Box::new(review),
                        })
                    }
                    _ => unreachable!(),
                }
            }
        }
    }
}
