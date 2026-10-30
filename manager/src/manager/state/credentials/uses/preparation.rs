use super::*;

fn same_use_context(a: &CredentialUseReview, b: &CredentialUseReview) -> bool {
    a.binding == b.binding
        && a.fence == b.fence
        && a.incarnation == b.incarnation
        && a.purpose == b.purpose
        && a.item.id == b.item.id
        && a.item.epoch == b.item.epoch
        && a.destination == b.destination
}

impl State {
    pub(super) fn prepare_credential_use(
        &mut self,
        authority: &BrowserAuthority,
        destination: LiveDestination,
        previous: Option<CredentialUseId>,
    ) -> Result<CredentialUseReview, OperationError> {
        let task = self.browser_parent(authority)?;
        self.credential_identity()?;
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
            item: self.load_login(*item)?,
            purpose: *purpose,
            binding,
            fence: authority.fence,
            incarnation: authority.journal,
            destination,
            expires_at,
            state: UseState::AwaitingApproval,
        };
        let _ = self.profile_history(task.command.worker, review.destination.profile)?;
        let history: Vec<_> = self
            .credential_public_rows::<CredentialUseReview>(
                "SELECT view FROM credential_uses ORDER BY rowid",
            )?
            .into_iter()
            .filter(|r| r.binding.task == task.id)
            .collect();
        let mut remembered = None;
        if let Some(root) = history.first() {
            if history.len() > 2
                || root.item.revision != *revision
                || !same_use_context(root, &review)
            {
                return Err(OperationError::Conflict);
            }
            if let Some(successor) = history.get(1)
                && (root.state != UseState::Revoked
                    || root.purpose != CredentialFields::Password
                    || !same_use_context(root, successor)
                    || successor.item.revision != advance(root.item.revision)?
                    || successor.expires_at > root.expires_at)
            {
                return Err(OperationError::Conflict);
            }
            if previous.is_none() {
                let mut cached = root.clone();
                self.reconcile_credential_use(&mut cached)?;
                return Ok(cached);
            }
            if previous != Some(root.id) {
                return Err(OperationError::Conflict);
            }
            if let Some(successor) = history.get(1) {
                let mut cached = successor.clone();
                self.reconcile_credential_use(&mut cached)?;
                return Ok(cached);
            }
            if root.state != UseState::Revoked
                || root.purpose != CredentialFields::Password
                || root.item.metadata.password_policy != PasswordPolicy::RememberedOrigins
                || !root.destination.certain
                || review.item.revision != advance(root.item.revision)?
                || root.expires_at <= self.now()
                || self.rule(&review)? != Some(("allow".into(), root.id.to_string()))
            {
                return Err(OperationError::Conflict);
            }
            let source = load_authority(&self.connection, &root.id.to_string())?
                .ok_or(OperationError::Unauthorized)?;
            if !source.valid(&self.connection)? {
                return Err(OperationError::Unauthorized);
            }
            review.expires_at = root
                .expires_at
                .min(spec.deadline)
                .min(review.binding.expires_at);
            remembered = Some(source);
        } else if previous.is_some() || review.item.revision != *revision {
            return Err(OperationError::Conflict);
        }
        self.use_authority(&review)?;
        self.credential_capacity("credential_uses", MAX_CREDENTIAL_USES)?;
        if history.is_empty() {
            let inserted = self.connection.execute(
                "INSERT OR IGNORE INTO credential_prepare_attempts VALUES(?1,?2)",
                params![task.id.to_string(), task.command.execution.to_string()],
            )?;
            if inserted != 1 {
                return Err(OperationError::Conflict);
            }
            self.reject_known_secrets(&review.destination)?;
        }
        let source = if let Some(source) = remembered {
            Some(source)
        } else if review.purpose == CredentialFields::Password
            && review.item.metadata.password_policy == PasswordPolicy::RememberedOrigins
            && review.destination.certain
        {
            match self.rule(&review)? {
                Some((decision, cause)) if decision == "allow" => {
                    load_authority(&self.connection, &cause)?
                }
                _ => None,
            }
        } else {
            None
        };
        let source = match source {
            Some(source) if source.valid(&self.connection)? => Some(source),
            _ => None,
        };
        if source.is_some() {
            review.state = UseState::Authorized;
        }
        let tx = self.connection.transaction()?;
        save(&tx, &review)?;
        if let Some(source) = source {
            save_authority(&tx, review.id, &source)?;
        }
        tx.commit()?;
        Ok(review)
    }
}
