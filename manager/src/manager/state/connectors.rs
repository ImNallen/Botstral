use super::{
    tasks::{event, json, parse},
    *,
};
use crate::manager::connectors::{
    ConnectorAdmission, ConnectorDispatch, redact_result, secret_bytes, secret_values,
    validate_arguments, validate_catalog,
};
use botstral_link::*;

mod gateway;
#[cfg(test)]
mod tests;

fn audit(
    db: &Connection,
    connection: ConnectionId,
    call: Option<ConnectorCallId>,
    code: &str,
    digest: Option<&CertificateFingerprint>,
    at: i64,
) -> Result<(), OperationError> {
    db.execute(
        "INSERT INTO connector_audit(connection,call,code,digest,at) VALUES(?1,?2,?3,?4,?5)",
        params![
            connection.to_string(),
            call.map(|id| id.to_string()),
            code,
            digest.map(|d| d.as_str()),
            at
        ],
    )?;
    event(db, "connector_changed", connection)
}
fn save_connection(db: &Connection, view: &ConnectionView) -> Result<(), OperationError> {
    db.execute(
        "INSERT INTO connections VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET view=excluded.view",
        params![view.id.to_string(), json(view)?],
    )?;
    db.execute(
        "INSERT INTO connection_versions VALUES(?1,?2,?3)",
        params![
            view.id.to_string(),
            view.version.revision as i64,
            json(view)?
        ],
    )?;
    Ok(())
}
fn advance(n: u64) -> Result<u64, OperationError> {
    n.checked_add(1)
        .filter(|n| *n <= MAX_COUNTER)
        .ok_or(OperationError::Conflict)
}
fn context(view: &ConnectionView) -> String {
    format!(
        "{}\0{}\0{}",
        view.id,
        view.transport.class(),
        view.version.credential_epoch
    )
}
fn valid_environment(values: impl Iterator<Item = (String, String)>) -> bool {
    values.enumerate().all(|(i, (k, v))| {
        i < 16
            && !k.is_empty()
            && k.len() <= 64
            && k.bytes()
                .enumerate()
                .all(|(i, b)| b.is_ascii_alphabetic() || b == b'_' || (i > 0 && b.is_ascii_digit()))
            && !matches!(
                k.as_str(),
                "PYTHONPATH"
                    | "PYTHONHOME"
                    | "LD_PRELOAD"
                    | "LD_LIBRARY_PATH"
                    | "DYLD_INSERT_LIBRARIES"
            )
            && !v.contains('\0')
            && v.len() <= 8192
    })
}
impl State {
    pub(super) fn known_connector_secrets(
        &self,
    ) -> Result<Vec<age::secrecy::SecretString>, OperationError> {
        let views = self
            .connection
            .prepare("SELECT view FROM connection_versions")?
            .query_map([], |r| r.get::<_, String>(0))?
            .map(|row| parse::<ConnectionView>(row?))
            .collect::<Result<Vec<_>, _>>()?;
        let mut seen = std::collections::HashSet::new();
        let mut secrets = Vec::new();
        for view in views {
            if seen.insert((view.id, view.version.credential_epoch)) {
                secrets.extend(
                    secret_values(&self.connection_secret(&view)?)
                        .into_iter()
                        .map(age::secrecy::SecretString::from),
                );
            }
        }
        Ok(secrets)
    }
    fn connector_identity(&self) -> Result<&age::x25519::Identity, OperationError> {
        match &self.installation {
            Installation::Configured {
                vault: VaultState::Unlocked { identity },
            } => Ok(identity),
            _ => Err(OperationError::Locked),
        }
    }
    pub(in crate::manager) fn load_connection(
        &self,
        id: ConnectionId,
    ) -> Result<ConnectionView, OperationError> {
        let value = self
            .connection
            .query_row(
                "SELECT view FROM connections WHERE id=?1",
                [id.to_string()],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .ok_or(OperationError::NotFound)?;
        parse(value)
    }
    pub(in crate::manager) fn connection_view(
        &self,
        owner: impl Into<OwnerAuthority>,
        id: ConnectionId,
    ) -> Result<ConnectionView, OperationError> {
        self.authorize_owner(&owner.into())?;
        self.load_connection(id)
    }
    pub(in crate::manager) fn connections(
        &self,
        owner: impl Into<OwnerAuthority>,
    ) -> Result<Vec<ConnectionView>, OperationError> {
        self.authorize_owner(&owner.into())?;
        self.connection
            .prepare("SELECT view FROM connections ORDER BY rowid")?
            .query_map([], |r| r.get::<_, String>(0))?
            .map(|r| parse(r?))
            .collect()
    }
    pub(in crate::manager) fn install_connection(
        &mut self,
        owner: impl Into<OwnerAuthority>,
        input: InstallConnection,
    ) -> Result<ConnectionView, OperationError> {
        self.authorize_owner(&owner.into())?;
        if input.label.is_empty()
            || input.label.len() > 100
            || input.label.chars().any(char::is_control)
        {
            return Err(OperationError::InvalidInput);
        }
        match &input.transport {
            ConnectionTransport::RemoteHttp { endpoint } => {
                let url =
                    reqwest::Url::parse(endpoint).map_err(|_| OperationError::InvalidInput)?;
                if endpoint.len() > 2048
                    || url.scheme() != "https"
                    || url.host_str().is_none()
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || url.fragment().is_some()
                    || url.query().is_some()
                {
                    return Err(OperationError::InvalidInput);
                }
            }
            ConnectionTransport::WorkerStdio {
                worker,
                account,
                executable,
                arguments,
                cwd,
                environment,
            } => {
                let exists: bool = self.connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM workers WHERE id=?1 AND revoked=0)",
                    [worker.to_string()],
                    |r| r.get(0),
                )?;
                if !exists
                    || account.uid == 0
                    || !canonical_guest_path(executable)
                    || !canonical_guest_path(cwd)
                    || arguments.len() > 32
                    || arguments.iter().any(|a| a.len() > 2048 || a.contains('\0'))
                    || !valid_environment(environment.iter().map(|(k, v)| (k.clone(), v.clone())))
                    || json(&input)?.len() > 8192
                {
                    return Err(OperationError::InvalidInput);
                }
            }
        }
        if let Ok(previous) = self.load_connection(input.id) {
            return if previous.transport == input.transport && previous.label == input.label {
                Ok(previous)
            } else {
                Err(OperationError::Conflict)
            };
        }
        let count: i64 =
            self.connection
                .query_row("SELECT count(*) FROM connections", [], |r| r.get(0))?;
        if count >= MAX_CONNECTIONS as i64 {
            return Err(OperationError::Busy);
        }
        let view = ConnectionView {
            id: input.id,
            label: input.label,
            transport: input.transport,
            version: ConnectionVersion {
                connection: input.id,
                revision: 1,
                credential_epoch: 1,
            },
            availability: ConnectionAvailability::NeedsDiscovery,
            catalog_revision: 0,
            tools: vec![],
        };
        let encrypted = crypto::encrypt_connector(
            self.connector_identity()?,
            &context(&view),
            &secret_bytes(&ConnectionSecretInput::None)?,
        )?;
        let now = self.now();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        save_connection(&tx, &view)?;
        tx.execute(
            "INSERT INTO connection_secrets VALUES(?1,1,?2)",
            params![view.id.to_string(), encrypted],
        )?;
        audit(&tx, view.id, None, "installed", None, now)?;
        tx.commit()?;
        Ok(view)
    }
    fn connection_secret(
        &self,
        view: &ConnectionView,
    ) -> Result<ConnectionSecretInput, OperationError> {
        let ciphertext: Vec<u8> = self.connection.query_row(
            "SELECT ciphertext FROM connection_secrets WHERE connection=?1 AND epoch=?2",
            params![view.id.to_string(), view.version.credential_epoch as i64],
            |r| r.get(0),
        )?;
        let bytes =
            crypto::decrypt_connector(self.connector_identity()?, &context(view), &ciphertext)?;
        serde_json::from_slice(&bytes).map_err(|_| OperationError::Internal)
    }
    pub(in crate::manager) fn replace_connection_secret(
        &mut self,
        owner: impl Into<OwnerAuthority>,
        id: ConnectionId,
        input: ReplaceConnectionSecret,
    ) -> Result<ConnectionView, OperationError> {
        self.authorize_owner(&owner.into())?;
        let mut view = self.load_connection(id)?;
        if view.version.revision != input.expected_revision
            || view.availability == ConnectionAvailability::Revoked
        {
            return Err(OperationError::Conflict);
        }
        match (&view.transport, &input.secret) {
            (_, ConnectionSecretInput::None) => {}
            (ConnectionTransport::RemoteHttp { .. }, ConnectionSecretInput::Bearer { value })
                if !value.0.is_empty()
                    && value.0.len() <= 8192
                    && value.0.bytes().all(|b| b.is_ascii_graphic()) => {}
            (
                ConnectionTransport::WorkerStdio { environment, .. },
                ConnectionSecretInput::LocalEnvironment { values },
            ) if !values.is_empty()
                && values.values().all(|v| !v.0.is_empty())
                && values.keys().all(|k| !environment.contains_key(k))
                && valid_environment(values.iter().map(|(k, v)| (k.clone(), v.0.clone()))) => {}
            _ => return Err(OperationError::InvalidInput),
        }
        view.version.revision = advance(view.version.revision)?;
        view.version.credential_epoch = advance(view.version.credential_epoch)?;
        view.availability = ConnectionAvailability::NeedsDiscovery;
        view.tools.clear();
        let encrypted = crypto::encrypt_connector(
            self.connector_identity()?,
            &context(&view),
            &secret_bytes(&input.secret)?,
        )?;
        let now = self.now();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        save_connection(&tx, &view)?;
        tx.execute(
            "INSERT INTO connection_secrets VALUES(?1,?2,?3)",
            params![
                id.to_string(),
                view.version.credential_epoch as i64,
                encrypted
            ],
        )?;
        audit(&tx, id, None, "credential_rotated", None, now)?;
        tx.commit()?;
        self.invalidate_connector_calls(id, None)?;
        Ok(view)
    }
    pub(in crate::manager) fn change_connection(
        &mut self,
        owner: impl Into<OwnerAuthority>,
        id: ConnectionId,
        expected: u64,
        revoke: bool,
    ) -> Result<ConnectionView, OperationError> {
        self.authorize_owner(&owner.into())?;
        let mut view = self.load_connection(id)?;
        if view.version.revision != expected || view.availability == ConnectionAvailability::Revoked
        {
            return Err(OperationError::Conflict);
        }
        let secret = if revoke {
            ConnectionSecretInput::None
        } else {
            self.connection_secret(&view)?
        };
        view.version.revision = advance(view.version.revision)?;
        view.version.credential_epoch = advance(view.version.credential_epoch)?;
        view.availability = if revoke {
            ConnectionAvailability::Revoked
        } else {
            ConnectionAvailability::NeedsDiscovery
        };
        view.tools.clear();
        let encrypted = if revoke {
            None
        } else {
            Some(crypto::encrypt_connector(
                self.connector_identity()?,
                &context(&view),
                &secret_bytes(&secret)?,
            )?)
        };
        let now = self.now();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        save_connection(&tx, &view)?;
        if let Some(ciphertext) = encrypted {
            tx.execute(
                "INSERT INTO connection_secrets VALUES(?1,?2,?3)",
                params![
                    id.to_string(),
                    view.version.credential_epoch as i64,
                    ciphertext
                ],
            )?;
        }
        audit(
            &tx,
            id,
            None,
            if revoke { "revoked" } else { "reconnected" },
            None,
            now,
        )?;
        tx.commit()?;
        self.invalidate_connector_calls(id, None)?;
        Ok(view)
    }
    pub(in crate::manager) fn discover_connection(
        &mut self,
        owner: impl Into<OwnerAuthority>,
        input: DiscoverConnection,
    ) -> Result<TaskView, OperationError> {
        let owner = owner.into();
        self.authorize_owner(&owner)?;
        let view = self.load_connection(input.expected.connection)?;
        if view.version != input.expected || view.availability == ConnectionAvailability::Revoked {
            return Err(OperationError::Conflict);
        }
        self.submit_task(
            owner,
            SubmitTask {
                id: input.task,
                session_id: input.session,
                conversation_id: None,
                settings: TaskSettings {
                    model: "connector-discovery".into(),
                    max_turns: 1,
                    max_tokens: 1,
                    deadline: input.deadline,
                    max_cost_micros: 0,
                },
                work: WorkSpec::Connector {
                    spec: Box::new(ConnectorSpec {
                        operation: ConnectorOperation::Discover {
                            connection: input.expected,
                        },
                        runner: input.runner,
                        deadline: input.deadline,
                        result: connector_output(MAX_CONNECTOR_RESULT),
                    }),
                },
            },
        )
    }
    fn load_grants(
        &self,
        bot: BotId,
        id: ConnectionId,
    ) -> Result<BotConnectorGrants, OperationError> {
        let raw: Option<String> = self.connection.query_row("SELECT view FROM connector_grants WHERE bot=?1 AND connection=?2 ORDER BY revision DESC LIMIT 1", params![bot.to_string(), id.to_string()], |r| r.get(0)).optional()?;
        raw.map(parse).unwrap_or_else(|| {
            Ok(BotConnectorGrants {
                bot,
                connection: id,
                revision: 0,
                tools: vec![],
            })
        })
    }
    pub(in crate::manager) fn connector_grants(
        &self,
        owner: impl Into<OwnerAuthority>,
        bot: BotId,
        id: ConnectionId,
    ) -> Result<BotConnectorGrants, OperationError> {
        self.authorize_owner(&owner.into())?;
        self.load_connection(id)?;
        self.load_grants(bot, id)
    }
    pub(in crate::manager) fn set_connector_grants(
        &mut self,
        owner: impl Into<OwnerAuthority>,
        bot: BotId,
        input: SetConnectorGrants,
    ) -> Result<BotConnectorGrants, OperationError> {
        self.authorize_owner(&owner.into())?;
        let view = self.load_connection(input.connection.connection)?;
        let old = self.load_grants(bot, view.id)?;
        if old.revision != input.expected_revision
            || view.version != input.connection
            || view.catalog_revision != input.catalog_revision
            || (!input.tools.is_empty() && view.availability != ConnectionAvailability::Ready)
            || input.tools.len() > MAX_CONNECTOR_TOOLS
        {
            return Err(OperationError::Conflict);
        }
        let revision = advance(old.revision)?;
        let mut names = std::collections::HashSet::new();
        let tools = input
            .tools
            .iter()
            .map(|name| {
                let tool = view
                    .tools
                    .iter()
                    .find(|t| &t.name == name)
                    .ok_or(OperationError::InvalidInput)?;
                if !names.insert(name) {
                    return Err(OperationError::InvalidInput);
                }
                Ok(ConnectorToolBinding {
                    connection: view.version.clone(),
                    catalog_revision: view.catalog_revision,
                    grant_revision: revision,
                    name: name.clone(),
                    schema_digest: tool.schema_digest(),
                })
            })
            .collect::<Result<Vec<_>, OperationError>>()?;
        let grants = BotConnectorGrants {
            bot,
            connection: view.id,
            revision,
            tools,
        };
        let now = self.now();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO connector_grants VALUES(?1,?2,?3,?4)",
            params![
                bot.to_string(),
                view.id.to_string(),
                revision as i64,
                json(&grants)?
            ],
        )?;
        audit(
            &tx,
            view.id,
            None,
            "grants_changed",
            Some(&CertificateFingerprint::from_der(json(&grants)?.as_bytes())),
            now,
        )?;
        tx.commit()?;
        self.invalidate_connector_calls(view.id, Some(bot))?;
        Ok(grants)
    }
    pub(in crate::manager) fn connector_tools(
        &self,
        owner: impl Into<OwnerAuthority>,
        bot: BotId,
    ) -> Result<Vec<ConnectorToolDefinition>, OperationError> {
        self.authorize_owner(&owner.into())?;
        self.current_connector_tools(bot)
    }
    fn current_connector_tools(
        &self,
        bot: BotId,
    ) -> Result<Vec<ConnectorToolDefinition>, OperationError> {
        let ids = self
            .connection
            .prepare("SELECT id FROM connections ORDER BY rowid")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut tools = vec![];
        if !self.permission(bot)?.allow {
            return Ok(tools);
        }
        for id in ids {
            let id = id.parse().map_err(|_| OperationError::Internal)?;
            for binding in self.load_grants(bot, id)?.tools {
                if let Ok(definition) = self.validate_connector_binding(bot, &binding) {
                    tools.push(definition);
                }
            }
        }
        if tools.len() > MAX_CONNECTIONS * MAX_CONNECTOR_TOOLS {
            return Err(OperationError::Internal);
        }
        Ok(tools)
    }
    pub(super) fn current_connector_catalogs(
        &self,
    ) -> Result<Vec<ConnectorToolDefinition>, OperationError> {
        let bots = self
            .connection
            .prepare("SELECT DISTINCT bot FROM connector_grants WHERE bot IN (SELECT id FROM bots WHERE id NOT IN (SELECT bot FROM bot_tombstones)) ORDER BY bot")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut catalogs = Vec::new();
        for bot in bots {
            catalogs.extend(
                self.current_connector_tools(bot.parse().map_err(|_| OperationError::Internal)?)?,
            );
        }
        Ok(catalogs)
    }
    pub(in crate::manager) fn permitted_connector_catalog(
        &self,
        bot: BotId,
        policy: &ConnectorToolPolicy,
    ) -> Result<Vec<ConnectorToolDefinition>, OperationError> {
        policy.validate()?;
        policy
            .tools
            .iter()
            .map(|binding| self.validate_connector_binding(bot, binding))
            .collect()
    }
    fn validate_connector_binding(
        &self,
        bot: BotId,
        binding: &ConnectorToolBinding,
    ) -> Result<ConnectorToolDefinition, OperationError> {
        let view = self.load_connection(binding.connection.connection)?;
        let grants = self.load_grants(bot, view.id)?;
        if view.availability != ConnectionAvailability::Ready
            || view.version != binding.connection
            || view.catalog_revision != binding.catalog_revision
            || grants.revision != binding.grant_revision
            || !grants.tools.contains(binding)
            || !self.permission(bot)?.allow
        {
            return Err(OperationError::Unauthorized);
        }
        let tool = view
            .tools
            .iter()
            .find(|t| t.name == binding.name && t.schema_digest() == binding.schema_digest)
            .ok_or(OperationError::Unauthorized)?;
        Ok(ConnectorToolDefinition {
            binding: binding.clone(),
            description: tool.description.clone(),
            input_schema: tool.input_schema.clone(),
        })
    }
    pub(in crate::manager) fn validate_connector_work(
        &self,
        bot: BotId,
        worker: WorkerId,
        work: &WorkSpec,
    ) -> Result<(), OperationError> {
        match work {
            WorkSpec::Connector { spec } => {
                spec.validate()?;
                let view = self.load_connection(spec.operation.connection().connection)?;
                if view.version != *spec.operation.connection()
                    || view.availability == ConnectionAvailability::Revoked
                {
                    return Err(OperationError::Unauthorized);
                }
                if let ConnectionTransport::WorkerStdio {
                    worker: target,
                    account,
                    ..
                } = view.transport
                    && (target != worker || account != spec.runner.account)
                {
                    return Err(OperationError::Unauthorized);
                }
                if let ConnectorOperation::Call { proposal } = &spec.operation {
                    let definition = self.validate_connector_binding(bot, &proposal.tool)?;
                    validate_arguments(&definition.input_schema, &proposal.arguments)?;
                }
            }
            WorkSpec::Vibe { spec } => {
                if let Some(policy) = &spec.policy.connectors {
                    self.permitted_connector_catalog(bot, policy)?;
                    for binding in &policy.tools {
                        if let ConnectionTransport::WorkerStdio {
                            worker: target,
                            account,
                            ..
                        } = self
                            .load_connection(binding.connection.connection)?
                            .transport
                            && (target != worker || account != spec.policy.account)
                        {
                            return Err(OperationError::Unauthorized);
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
    pub(in crate::manager) fn connector_audit(
        &self,
        owner: impl Into<OwnerAuthority>,
        id: ConnectionId,
        after: u64,
    ) -> Result<Vec<ConnectorAudit>, OperationError> {
        self.authorize_owner(&owner.into())?;
        if after > MAX_COUNTER {
            return Err(OperationError::InvalidInput);
        }
        self.connection.prepare("SELECT sequence,call,code,digest,at FROM connector_audit WHERE connection=?1 AND sequence>?2 ORDER BY sequence LIMIT 128")?
            .query_map(params![id.to_string(),after as i64], |r| Ok((r.get::<_,i64>(0)? as u64,r.get::<_,Option<String>>(1)?,r.get::<_,String>(2)?,r.get::<_,Option<String>>(3)?,r.get::<_,i64>(4)?)))?
            .map(|r| { let (sequence,call,code,digest,at) = r?; Ok(ConnectorAudit { sequence,connection:id,call:call.map(|s|s.parse().map_err(|_|OperationError::Internal)).transpose()?,code,digest:digest.map(|s|s.try_into().map_err(OperationError::from)).transpose()?,at }) }).collect()
    }
}
