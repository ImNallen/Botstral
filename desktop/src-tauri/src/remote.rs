use base64::{Engine, engine::general_purpose::STANDARD};
use botstral_link::*;
use botstral_owner_client::{ClientError, OwnerClient, OwnerSession, SecretStore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::{mpsc, oneshot, watch};
use uuid::Uuid;

#[derive(Clone, Serialize)]
pub struct PairingCandidate {
    pub id: Uuid,
    pub manager: ManagerId,
    pub endpoint: String,
    pub server_name: String,
    pub leaf: CertificateFingerprint,
    pub fingerprint: DeviceFingerprint,
    pub role: DeviceRole,
    pub expires_at: i64,
}
#[derive(Clone, Serialize)]
pub struct ActionReview {
    pub id: Uuid,
    pub task: TaskId,
    pub command: CommandBody,
    pub permission_revision: u64,
    pub approval: ApprovalView,
    pub expires_at: i64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub id: Uuid,
    pub operation: OwnerOperation,
    pub status: String,
}
#[derive(Clone, Serialize)]
pub struct RemoteSnapshot {
    pub generation: u64,
    pub revision: u64,
    pub status: String,
    pub issue: Option<String>,
    pub pairing: Option<PairingRequest>,
    pub candidate: Option<PairingCandidate>,
    #[serde(serialize_with = "serialize_workspace")]
    pub workspace: Option<OwnerWorkspace>,
    #[serde(serialize_with = "serialize_review")]
    pub review: Option<ActionReview>,
    pub intents: Vec<IntentView>,
}
#[derive(Clone, Serialize)]
pub struct IntentView {
    pub id: Uuid,
    pub description: String,
    pub identity: String,
    pub status: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SendInput {
    pub bot: BotId,
    pub session: ComputerSessionId,
    pub text: String,
    pub account: ProcessAccount,
    pub runtime: Option<CertificateFingerprint>,
    pub process: bool,
    pub seconds: u32,
    pub turns: u32,
    pub tokens: u64,
    pub cost_microusd: u64,
    pub output_tokens: u32,
}
#[derive(Clone)]
pub struct RemoteService {
    root: PathBuf,
    store: Arc<dyn SecretStore>,
    view: watch::Sender<RemoteSnapshot>,
    control: Arc<Mutex<Option<Control>>>,
    signing_out: Arc<AtomicBool>,
    signout_gate: Arc<tokio::sync::Mutex<()>>,
}
struct Control {
    tx: mpsc::Sender<Envelope>,
    task: tokio::task::JoinHandle<()>,
}
struct Envelope {
    generation: u64,
    command: IntentCommand,
    reply: oneshot::Sender<Result<RemoteSnapshot, String>>,
}
pub enum IntentCommand {
    Begin {
        label: String,
    },
    Stage(OwnerInvitation),
    StageTrust(ManagerTrust),
    Confirm(Uuid),
    Refresh,
    Send(SendInput),
    OpenSession {
        bot: BotId,
        worker: WorkerId,
    },
    Permission {
        bot: BotId,
        expected: u64,
        allow: bool,
    },
    Review(TaskId),
    Approve(Uuid),
    CancelTask(TaskId),
    CancelRun(RunId),
    RevokeApproval(TaskId),
    AcknowledgeUncertainty(Uuid),
    Suspend,
    Resume,
    Fetch {
        task: TaskId,
        artifact: ArtifactId,
        result: oneshot::Sender<Result<VerifiedArtifact, String>>,
    },
}
enum Candidate {
    Invitation(OwnerInvitation),
    Trust(ManagerTrust),
}
struct Actor {
    root: PathBuf,
    store: Arc<dyn SecretStore>,
    view: watch::Sender<RemoteSnapshot>,
    generation: u64,
    client: Option<OwnerClient>,
    session: Option<OwnerSession>,
    candidate: Option<Candidate>,
    intents: Vec<Intent>,
    suspended: bool,
    reconnect_attempt: u32,
}
pub struct VerifiedArtifact {
    pub name: String,
    pub bytes: Vec<u8>,
}
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
fn storage(root: &Path, name: &str, value: &impl Serialize) -> Result<(), String> {
    let path = root.join(format!(".{name}.part"));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&path)
        .map_err(|_| "storage")?;
    validate_private(&file)?;
    file.set_len(0).map_err(|_| "storage")?;
    serde_json::to_writer(&mut file, value).map_err(|_| "storage")?;
    file.sync_all().map_err(|_| "storage")?;
    fs::rename(path, root.join(name)).map_err(|_| "storage")?;
    fs::File::open(root)
        .and_then(|f| f.sync_all())
        .map_err(|_| "storage".into())
}
fn validate_private(file: &fs::File) -> Result<(), String> {
    let metadata = file.metadata().map_err(|_| "storage")?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.mode() & 0o7777 != 0o600
        || metadata.uid() != unsafe { libc::geteuid() }
    {
        return Err("storage".into());
    }
    Ok(())
}
fn read_journal(path: &Path) -> Result<Vec<Intent>, String> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| "storage")?;
    validate_private(&file)?;
    if file.metadata().map_err(|_| "storage")?.len() > 1024 * 1024 {
        return Err("storage".into());
    }
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "storage")?;
    if bytes.len() > 1024 * 1024 {
        return Err("storage".into());
    }
    let intents: Vec<Intent> = serde_json::from_slice(&bytes).map_err(|_| "storage")?;
    if intents.len() > 128
        || intents.iter().any(|i| {
            !matches!(
                i.status.as_str(),
                "unknown" | "pending" | "confirmed" | "attention" | "refused"
            ) || !matches!(
                i.operation,
                OwnerOperation::SubmitRun { .. }
                    | OwnerOperation::CancelRun { .. }
                    | OwnerOperation::CancelTask { .. }
                    | OwnerOperation::GrantReviewedApproval { .. }
                    | OwnerOperation::RevokeApproval { .. }
                    | OwnerOperation::SetPermission { .. }
                    | OwnerOperation::OpenComputerSession { .. }
            )
        })
    {
        return Err("storage".into());
    }
    Ok(intents)
}
fn error_code(error: ClientError) -> String {
    match error {
        ClientError::Trust => "trust",
        ClientError::Storage => "keychain_or_storage",
        ClientError::Authentication(OwnerError::Unauthorized) => "revoked",
        ClientError::Authentication(OwnerError::Busy) | ClientError::Refused(OwnerError::Busy) => {
            "busy"
        }
        ClientError::Authentication(OwnerError::RateLimited)
        | ClientError::Refused(OwnerError::RateLimited) => "rate_limited",
        ClientError::Authentication(_) => "authentication",
        ClientError::Refused(OwnerError::InferenceUnverified) => "inference_unverified",
        ClientError::Refused(OwnerError::InferenceBudget) => "budget",
        ClientError::Refused(_) => "refused",
        ClientError::UnknownOutcome => "unknown_outcome",
        ClientError::Protocol => "incompatible_manager",
        ClientError::Network => "offline",
    }
    .into()
}
impl RemoteService {
    pub fn new(root: PathBuf, store: Arc<dyn SecretStore>) -> Self {
        let (view, _) = watch::channel(RemoteSnapshot {
            generation: 0,
            revision: 0,
            status: "unpaired".into(),
            issue: None,
            pairing: None,
            candidate: None,
            workspace: None,
            review: None,
            intents: vec![],
        });
        Self {
            root,
            store,
            view,
            control: Arc::new(Mutex::new(None)),
            signing_out: Arc::new(AtomicBool::new(false)),
            signout_gate: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
    pub fn inspect(&self) -> RemoteSnapshot {
        self.view.borrow().clone()
    }
    pub fn observe(&self) -> watch::Receiver<RemoteSnapshot> {
        self.view.subscribe()
    }
    fn start(&self, generation: u64) -> Result<(), String> {
        let mut control = self.control.lock().unwrap();
        if self.signing_out.load(Ordering::Acquire) || self.inspect().generation != generation {
            return Err("stale".into());
        }
        if control.is_some() {
            return Ok(());
        }
        let (tx, rx) = mpsc::channel(16);
        let actor = Actor {
            root: self.root.clone(),
            store: self.store.clone(),
            view: self.view.clone(),
            generation,
            client: None,
            session: None,
            candidate: None,
            intents: vec![],
            suspended: false,
            reconnect_attempt: 0,
        };
        *control = Some(Control {
            tx,
            task: tokio::spawn(actor.run(rx)),
        });
        Ok(())
    }
    pub async fn command(&self, command: IntentCommand) -> Result<RemoteSnapshot, String> {
        let generation = self.inspect().generation;
        self.start(generation)?;
        let tx = self
            .control
            .lock()
            .unwrap()
            .as_ref()
            .ok_or("unavailable")?
            .tx
            .clone();
        let (reply, result) = oneshot::channel();
        tx.try_send(Envelope {
            generation,
            command,
            reply,
        })
        .map_err(|_| "busy")?;
        result.await.map_err(|_| "stale")?
    }
    pub async fn artifact(
        &self,
        task: TaskId,
        artifact: ArtifactId,
    ) -> Result<VerifiedArtifact, String> {
        let (result, receiver) = oneshot::channel();
        self.command(IntentCommand::Fetch {
            task,
            artifact,
            result,
        })
        .await?;
        receiver.await.map_err(|_| "offline".to_string())?
    }
    pub async fn restore(&self) -> Result<RemoteSnapshot, String> {
        self.command(IntentCommand::Refresh).await
    }
    pub async fn shutdown(&self) {
        let old = self.control.lock().unwrap().take();
        if let Some(old) = old {
            old.task.abort();
            let _ = old.task.await;
        }
    }
    pub async fn sign_out(&self) -> Result<RemoteSnapshot, String> {
        self.signing_out.store(true, Ordering::Release);
        self.view.send_modify(|v| {
            v.generation += 1;
            v.revision += 1;
            v.status = "signed_out".into();
            v.workspace = None;
            v.review = None;
            v.pairing = None;
            v.candidate = None;
            v.intents.clear();
            v.issue = None;
        });
        let Ok(_gate) = self.signout_gate.try_lock() else {
            return Ok(self.inspect());
        };
        self.shutdown().await;
        if !self.root.exists() {
            self.signing_out.store(false, Ordering::Release);
            return Ok(self.inspect());
        }
        let written = storage(&self.root, "signed-out.json", &true);
        if written.is_err() {
            self.view
                .send_modify(|v| v.issue = Some("sign_out_storage_failed".into()));
            return Err("sign_out_storage_failed".into());
        }
        if self.store.delete().is_err() {
            self.view
                .send_modify(|v| v.issue = Some("credential_deletion_failed".into()));
        }
        self.signing_out.store(false, Ordering::Release);
        Ok(self.inspect())
    }
}
impl Actor {
    fn publish(&self, update: impl FnOnce(&mut RemoteSnapshot)) {
        self.view.send_modify(|v| {
            if v.generation == self.generation {
                update(v);
                v.revision += 1;
            }
        });
    }
    async fn run(mut self, mut rx: mpsc::Receiver<Envelope>) {
        if self.root.exists()
            && !self.root.join("signed-out.json").exists()
            && let Err(error) = self.restore()
        {
            self.failed(error);
        }
        let mut pending = None;
        loop {
            let envelope = if pending.is_some() {
                pending.take()
            } else if let Some(session) = self.session.as_mut() {
                tokio::select! {
                    message = rx.recv() => {
                        self.session = None;
                        message
                    },
                    signal = session.next_signal() => {
                        match signal {
                            Ok(_) => { if let Err(error) = self.recover().await { self.failed(error); } },
                            Err(error) => { self.failed(error_code(error)); },
                        }
                        continue;
                    }
                }
            } else {
                tokio::select! {
                    message = rx.recv() => message,
                    _ = tokio::time::sleep(self.retry_delay()), if self.client.as_ref().is_some_and(|c| c.trust().is_some()) && !self.suspended && self.view.borrow().issue.as_deref().is_none_or(|s| matches!(s, "offline" | "busy" | "rate_limited" | "unknown_outcome")) => {
                        if let Err(error) = self.recover().await { self.failed(error); }
                        continue;
                    }
                }
            };
            let Some(envelope) = envelope else {
                break;
            };
            if envelope.generation != self.generation
                || self.view.borrow().generation != self.generation
            {
                let _ = envelope.reply.send(Err("stale".into()));
                continue;
            }
            let result = if matches!(&envelope.command, IntentCommand::Fetch { .. }) {
                enum ReadResult {
                    Done(Result<(), String>),
                    Interrupted(Option<Envelope>),
                }
                let outcome = {
                    let operation = self.execute(envelope.command);
                    tokio::pin!(operation);
                    tokio::select! {
                        result = &mut operation => ReadResult::Done(result),
                        next = rx.recv() => ReadResult::Interrupted(next),
                    }
                };
                match outcome {
                    ReadResult::Done(result) => result,
                    ReadResult::Interrupted(next) => {
                        self.session = None;
                        pending = next;
                        let _ = envelope.reply.send(Err("interrupted_read".into()));
                        continue;
                    }
                }
            } else {
                self.execute(envelope.command).await
            };
            if let Err(error) = &result {
                self.failed(error.clone());
            }
            let _ = envelope
                .reply
                .send(result.map(|_| self.view.borrow().clone()));
        }
    }
    fn restore(&mut self) -> Result<(), String> {
        if self.root.join("signed-out.json").exists() {
            let failed = self.store.delete().is_err();
            self.publish(|v| {
                v.status = "signed_out".into();
                v.workspace = None;
                v.issue = failed.then(|| "credential_deletion_failed".into());
            });
            return Ok(());
        }
        if self.store.read().map_err(error_code)?.is_none() {
            return Ok(());
        }
        self.client =
            Some(OwnerClient::open_with(&self.root, self.store.clone()).map_err(error_code)?);
        let journal = self.root.join("intents.json");
        if journal.exists() {
            self.intents = read_journal(&journal)?;
        }
        if let Some(client) = &self.client
            && client.trust().is_none()
        {
            let pairing = client.pairing_request("Pending desktop".into());
            self.publish(|v| {
                v.status = "pairing".into();
                v.pairing = Some(pairing);
            });
        }
        if let Some(client) = &self.client
            && let Some(image) = client.installed_workspace().map_err(error_code)?
        {
            self.publish(|v| {
                v.status = "offline".into();
                v.workspace = Some(image);
                v.intents = self.intent_views();
            });
        }
        Ok(())
    }
    fn retry_delay(&self) -> Duration {
        let base = 250u64
            .saturating_mul(1u64 << self.reconnect_attempt.min(7))
            .min(30_000);
        Duration::from_millis(
            base / 2 + (Uuid::new_v4().as_u128() % u128::from(base / 2 + 1)) as u64,
        )
    }
    fn failed(&mut self, error: String) {
        self.reconnect_attempt = self.reconnect_attempt.saturating_add(1);
        self.session = None;
        self.publish(|v| {
            v.status = if error == "offline" || error == "unknown_outcome" {
                "offline"
            } else {
                "blocked"
            }
            .into();
            if error == "revoked" {
                v.workspace = None;
                v.review = None;
            }
            v.issue = Some(error);
        });
    }
    async fn recover(&mut self) -> Result<(), String> {
        if self.view.borrow().generation != self.generation {
            return Err("stale".into());
        }
        if self.suspended {
            return Err("suspended".into());
        }
        if self.client.is_none() {
            self.restore()?;
        }
        let Some(client) = self.client.as_mut() else {
            return Ok(());
        };
        if client.trust().is_none() {
            return Ok(());
        }
        if self.session.is_none() {
            self.session = Some(client.connect().await.map_err(error_code)?);
        }
        let image = client
            .recover_workspace(self.session.as_mut().ok_or("offline")?)
            .await
            .map_err(error_code)?;
        for intent in &mut self.intents {
            if intent.status == "refused" {
                continue;
            }
            let confirmed = match &intent.operation {
                OwnerOperation::SubmitRun { input } => {
                    image.runs.iter().any(|r| r.request == **input)
                }
                OwnerOperation::CancelRun { id } => image.runs.iter().any(|r| {
                    r.summary.id == *id
                        && matches!(
                            r.summary.phase,
                            RunPhase::Completed | RunPhase::Cancelled | RunPhase::Stopped { .. }
                        )
                        && r.turns.iter().all(|turn| {
                            turn.intent.as_ref().is_none_or(|intent| {
                                image
                                    .state
                                    .tasks
                                    .iter()
                                    .any(|t| t.id == intent.task && t.state.terminal())
                            })
                        })
                }),
                OwnerOperation::CancelTask { id } => image
                    .state
                    .tasks
                    .iter()
                    .any(|t| t.id == *id && t.state.terminal()),
                OwnerOperation::GrantReviewedApproval {
                    id,
                    command,
                    approval,
                    permission_revision,
                    ..
                } => image.state.tasks.iter().any(|t| {
                    t.id == *id
                        && t.command == **command
                        && t.approval.id == approval.id
                        && t.approval.binding.as_ref().is_some_and(|b| {
                            b.permission_revision == *permission_revision
                                && t.command.matches_authority(b)
                        })
                }),
                OwnerOperation::RevokeApproval { id } => image
                    .state
                    .tasks
                    .iter()
                    .any(|t| t.id == *id && t.approval.revoked),
                OwnerOperation::SetPermission {
                    bot,
                    expected_revision,
                    allow,
                } => image.state.permissions.iter().any(|p| {
                    p.bot == *bot
                        && Some(p.revision) == expected_revision.checked_add(1)
                        && p.allow == *allow
                }),
                OwnerOperation::OpenComputerSession { bot, worker } => image
                    .sessions
                    .iter()
                    .any(|s| s.bot_id == *bot && s.worker_id == *worker),
                _ => false,
            };
            if confirmed {
                intent.status = "confirmed".into();
            }
        }
        self.save_intents()?;
        self.reconnect_attempt = 0;
        self.publish(|v| {
            v.status = "online".into();
            v.issue = None;
            v.workspace = Some(image);
            v.intents = self.intent_views();
        });
        Ok(())
    }
    fn intent_views(&self) -> Vec<IntentView> {
        self.intents
            .iter()
            .map(|i| IntentView {
                id: i.id,
                status: i.status.clone(),
                identity: match &i.operation {
                    OwnerOperation::SubmitRun { input } => input.id.to_string(),
                    OwnerOperation::CancelRun { id } => id.to_string(),
                    OwnerOperation::CancelTask { id }
                    | OwnerOperation::GrantReviewedApproval { id, .. }
                    | OwnerOperation::RevokeApproval { id } => id.to_string(),
                    _ => i.id.to_string(),
                },
                description: match &i.operation {
                    OwnerOperation::SubmitRun { .. } => "Run submission",
                    OwnerOperation::CancelRun { .. } | OwnerOperation::CancelTask { .. } => {
                        "Cancellation requested"
                    }
                    OwnerOperation::GrantReviewedApproval { .. } => "Approval",
                    OwnerOperation::RevokeApproval { .. } => "Approval revocation",
                    OwnerOperation::SetPermission { .. } => "Permission change",
                    OwnerOperation::OpenComputerSession { .. } => "Computer connection",
                    _ => "Operation",
                }
                .into(),
            })
            .collect()
    }
    fn save_intents(&self) -> Result<(), String> {
        storage(&self.root, "intents.json", &self.intents)
    }
    async fn mutate(&mut self, operation: OwnerOperation) -> Result<(), String> {
        self.recover().await?;
        if self.view.borrow().generation != self.generation {
            return Err("stale".into());
        }
        if self.view.borrow().status != "online" {
            return Err("offline".into());
        }
        if self.intents.iter().any(|i| i.status == "unknown") {
            return Err("unresolved_intent".into());
        }
        if self.intents.len() >= 128 {
            self.intents
                .retain(|i| matches!(i.status.as_str(), "unknown" | "attention"));
            if self.intents.len() >= 128 {
                return Err("journal_full".into());
            }
        }
        let id = Uuid::new_v4();
        self.intents.push(Intent {
            id,
            operation: operation.clone(),
            status: "unknown".into(),
        });
        self.save_intents()?;
        self.publish(|v| v.intents = self.intent_views());
        let reply = self
            .session
            .as_mut()
            .ok_or("offline")?
            .call(operation)
            .await;
        if let Err(error) = reply {
            if matches!(error, ClientError::Refused(_)) {
                self.intents.last_mut().ok_or("storage")?.status = "refused".into();
                self.save_intents()?;
            }
            return Err(error_code(error));
        }
        self.intents.last_mut().ok_or("storage")?.status = "pending".into();
        self.save_intents().map_err(|_| "unknown_outcome")?;
        self.recover().await.map_err(|_| "unknown_outcome".into())
    }
    async fn fetch(
        &mut self,
        task: TaskId,
        artifact: ArtifactId,
    ) -> Result<VerifiedArtifact, String> {
        self.recover().await?;
        let output = self
            .view
            .borrow()
            .workspace
            .as_ref()
            .and_then(|w| w.state.tasks.iter().find(|t| t.id == task))
            .and_then(|t| {
                t.outputs.iter().find(
                    |o| matches!(&o.state, ArtifactState::Available { id, .. } if *id == artifact),
                )
            })
            .cloned()
            .ok_or("artifact_unavailable")?;
        let ArtifactState::Available { content, .. } = output.state else {
            return Err("artifact_unavailable".into());
        };
        let OwnerReply::Artifact {
            declaration,
            base64,
        } = self
            .session
            .as_mut()
            .ok_or("offline")?
            .call(OwnerOperation::Artifact { task, id: artifact })
            .await
            .map_err(error_code)?
        else {
            return Err("protocol".into());
        };
        if declaration != output.declaration
            || content.length > MAX_ARTIFACT_BYTES
            || content.length > declaration.max_bytes
            || base64.len() > MAX_ARTIFACT_BYTES as usize * 4 / 3 + 4
        {
            return Err("artifact_invalid".into());
        }
        let bytes = STANDARD.decode(base64).map_err(|_| "artifact_invalid")?;
        if bytes.len() as u64 != content.length
            || format!("{:x}", Sha256::digest(&bytes)) != content.sha256.as_str()
        {
            return Err("artifact_invalid".into());
        }
        Ok(VerifiedArtifact {
            name: declaration.name,
            bytes,
        })
    }
    async fn execute(&mut self, command: IntentCommand) -> Result<(), String> {
        match command {
            IntentCommand::Fetch {
                task,
                artifact,
                result,
            } => {
                let value = self.fetch(task, artifact).await;
                let status = value.as_ref().map(|_| ()).map_err(Clone::clone);
                let _ = result.send(value);
                status
            }
            IntentCommand::Begin { label } => {
                self.session = None;
                self.client = None;
                if self.view.borrow().issue.as_deref() == Some("revoked") {
                    self.store.delete().map_err(error_code)?;
                    self.intents.clear();
                    self.save_intents()?;
                    for name in ["workspace.json", "state.json"] {
                        let _ = fs::remove_file(self.root.join(name));
                    }
                    self.generation += 1;
                    self.view.send_modify(|v| {
                        v.generation = self.generation;
                        v.workspace = None;
                        v.review = None;
                        v.candidate = None;
                        v.pairing = None;
                        v.intents.clear();
                    });
                }
                if label.is_empty() || label.len() > 100 {
                    return Err("invalid_input".into());
                }
                if self.root.join("signed-out.json").exists() {
                    self.store.delete().map_err(error_code)?;
                }
                if self.root.join("signed-out.json").exists() {
                    self.intents.clear();
                    self.save_intents()?;
                    for name in ["workspace.json", "state.json"] {
                        let _ = fs::remove_file(self.root.join(name));
                    }
                }
                let pairing = OwnerClient::initialize_with(&self.root, label, self.store.clone())
                    .map_err(error_code)?;
                if self.root.join("signed-out.json").exists() {
                    fs::remove_file(self.root.join("signed-out.json")).map_err(|_| "storage")?;
                }
                self.client = Some(
                    OwnerClient::open_with(&self.root, self.store.clone()).map_err(error_code)?,
                );
                self.publish(|v| {
                    v.status = "pairing".into();
                    v.issue = None;
                    v.pairing = Some(pairing);
                });
                Ok(())
            }
            IntentCommand::Stage(bundle) => {
                let fingerprint = self
                    .view
                    .borrow()
                    .pairing
                    .as_ref()
                    .ok_or("begin_pairing_first")?
                    .fingerprint
                    .clone();
                if bundle.fingerprint != fingerprint
                    || bundle.role != DeviceRole::Owner
                    || bundle.expires_at <= now()
                {
                    return Err("invalid_invitation".into());
                }
                tls::pinned_manager_config(&bundle.trust).map_err(|_| "trust")?;
                let candidate = PairingCandidate {
                    id: Uuid::new_v4(),
                    manager: bundle.trust.manager,
                    endpoint: bundle.trust.endpoint.clone(),
                    server_name: bundle.trust.server_name.clone(),
                    leaf: bundle.trust.leaf.clone(),
                    fingerprint,
                    role: bundle.role,
                    expires_at: bundle.expires_at,
                };
                self.candidate = Some(Candidate::Invitation(bundle));
                self.publish(|v| {
                    v.candidate = Some(candidate);
                    v.status = "pairing".into();
                    v.issue = None;
                });
                Ok(())
            }
            IntentCommand::StageTrust(trust) => {
                let client = self.client.as_ref().ok_or("unpaired")?;
                if client
                    .trust()
                    .is_none_or(|old| old.manager != trust.manager)
                {
                    return Err("trust".into());
                }
                tls::pinned_manager_config(&trust).map_err(|_| "trust")?;
                let pairing = client.pairing_request("Existing desktop".into());
                let candidate = PairingCandidate {
                    id: Uuid::new_v4(),
                    manager: trust.manager,
                    endpoint: trust.endpoint.clone(),
                    server_name: trust.server_name.clone(),
                    leaf: trust.leaf.clone(),
                    fingerprint: pairing.fingerprint.clone(),
                    role: DeviceRole::Owner,
                    expires_at: now() + 600,
                };
                self.candidate = Some(Candidate::Trust(trust));
                self.publish(|v| {
                    v.pairing = Some(pairing);
                    v.candidate = Some(candidate);
                    v.issue = None;
                });
                Ok(())
            }
            IntentCommand::Confirm(id) => {
                if self
                    .view
                    .borrow()
                    .candidate
                    .as_ref()
                    .is_none_or(|c| c.id != id)
                {
                    return Err("stale_review".into());
                }
                let client = self.client.as_mut().ok_or("unpaired")?;
                match self.candidate.take().ok_or("unpaired")? {
                    Candidate::Invitation(bundle) => {
                        client.import_invitation(bundle).map_err(error_code)?
                    }
                    Candidate::Trust(trust) => client.import_trust(trust).map_err(error_code)?,
                }
                self.recover().await?;
                self.publish(|v| {
                    v.candidate = None;
                    v.pairing = None;
                });
                Ok(())
            }
            IntentCommand::Suspend => {
                self.session = None;
                self.suspended = true;
                self.publish(|v| {
                    v.status = "suspended".into();
                    v.review = None;
                });
                Ok(())
            }
            IntentCommand::Resume => {
                self.suspended = false;
                self.recover().await
            }
            IntentCommand::Refresh => {
                self.publish(|v| v.review = None);
                self.recover().await
            }
            IntentCommand::OpenSession { bot, worker } => {
                self.mutate(OwnerOperation::OpenComputerSession { bot, worker })
                    .await
            }
            IntentCommand::Permission {
                bot,
                expected,
                allow,
            } => {
                self.mutate(OwnerOperation::SetPermission {
                    bot,
                    expected_revision: expected,
                    allow,
                })
                .await
            }
            IntentCommand::CancelRun(id) => self.mutate(OwnerOperation::CancelRun { id }).await,
            IntentCommand::CancelTask(id) => self.mutate(OwnerOperation::CancelTask { id }).await,
            IntentCommand::AcknowledgeUncertainty(id) => {
                let intent = self
                    .intents
                    .iter_mut()
                    .find(|i| i.id == id && i.status == "unknown")
                    .ok_or("stale")?;
                intent.status = "attention".into();
                self.save_intents()?;
                self.publish(|v| v.intents = self.intent_views());
                Ok(())
            }
            IntentCommand::RevokeApproval(id) => {
                self.mutate(OwnerOperation::RevokeApproval { id }).await
            }
            IntentCommand::Send(input) => {
                self.recover().await?;
                let image = self.view.borrow().workspace.clone().ok_or("unpaired")?;
                if !image.run_ready_bots.contains(&input.bot) {
                    return Err("inference_unverified".into());
                }
                if !image.vault_unlocked {
                    return Err("vault_locked".into());
                }
                let config = image
                    .state
                    .inference_configurations
                    .iter()
                    .find(|c| c.bot == input.bot)
                    .ok_or("configure_inference_locally")?;
                if input.account.uid == 0
                    || input.seconds == 0
                    || input.seconds > 300
                    || input.turns == 0
                    || input.turns > 8
                    || input.text.is_empty()
                    || input.text.len() > MAX_INFERENCE_PROMPT_BYTES
                {
                    return Err("invalid_input".into());
                }
                let deadline = now() + i64::from(input.seconds);
                let process = input.process.then(|| ProcessToolPolicy {
                    profile: ProcessProfile::LinuxSystemdV1,
                    account: input.account,
                    environment: Default::default(),
                    limits: ProcessLimits {
                        runtime_seconds: input.seconds,
                        output_bytes: 1024,
                        processes: 8,
                        memory_bytes: 64 * 1024 * 1024,
                    },
                });
                let computer = input.runtime.map(|sha256| ComputerToolPolicy {
                    profile: ComputerProfile::LinuxPlaywrightV1,
                    account: input.account,
                    runtime: ComputerRuntime { sha256 },
                    limits: ProcessLimits {
                        runtime_seconds: input.seconds,
                        output_bytes: 1024,
                        processes: 64,
                        memory_bytes: 512 * 1024 * 1024,
                    },
                    viewport: Viewport {
                        width: 800,
                        height: 600,
                    },
                    outputs: vec![
                        OutputDeclaration {
                            slot: "text".into(),
                            name: "result.txt".into(),
                            media_type: MediaType::TextPlain,
                            max_bytes: 8192,
                        },
                        OutputDeclaration {
                            slot: "image".into(),
                            name: "screenshot.png".into(),
                            media_type: MediaType::Png,
                            max_bytes: 1024 * 1024,
                        },
                    ],
                });
                self.mutate(OwnerOperation::SubmitRun {
                    input: Box::new(SubmitRun {
                        images: vec![],
                        id: RunId::new(),
                        bot: input.bot,
                        session: input.session,
                        configuration_revision: config.revision,
                        prompt: input.text,
                        execution_policy: RunExecutionPolicy {
                            vision: None,
                            vibe: None,
                            computer,
                            process,
                        },
                        limits: RunLimits {
                            deadline,
                            max_turns: input.turns,
                            max_total_tokens: input.tokens,
                            max_cost_microusd: input.cost_microusd,
                            max_output_tokens: input.output_tokens,
                            max_operations: 12,
                            max_concurrent_operations: 1,
                        },
                    }),
                })
                .await
            }
            IntentCommand::Review(id) => {
                self.recover().await?;
                let image = self.view.borrow().workspace.clone().ok_or("offline")?;
                let task = image
                    .state
                    .tasks
                    .iter()
                    .find(|t| t.id == id)
                    .ok_or("not_found")?;
                if task.state.terminal() || task.approval.consumed {
                    return Err("stale_review".into());
                }
                let permission = image
                    .state
                    .permissions
                    .iter()
                    .find(|p| p.bot == task.command.bot)
                    .ok_or("permission_denied")?;
                if !permission.allow {
                    return Err("permission_denied".into());
                }
                let review = ActionReview {
                    id: Uuid::new_v4(),
                    task: id,
                    command: task.command.clone(),
                    permission_revision: permission.revision,
                    approval: task.approval.clone(),
                    expires_at: (now() + 60).min(task.request.settings.deadline),
                };
                self.publish(|v| v.review = Some(review));
                Ok(())
            }
            IntentCommand::Approve(id) => {
                let review = self
                    .view
                    .borrow()
                    .review
                    .clone()
                    .filter(|r| r.id == id && r.expires_at > now())
                    .ok_or("stale_review")?;
                self.publish(|v| v.review = None);
                self.mutate(OwnerOperation::GrantReviewedApproval {
                    id: review.task,
                    command: Box::new(review.command),
                    approval: review.approval,
                    permission_revision: review.permission_revision,
                    expires_at: review.expires_at,
                })
                .await
            }
        }
    }
}

pub fn save_artifact(destination: &Path, artifact: VerifiedArtifact) -> Result<(), String> {
    let parent = destination.parent().ok_or("invalid_destination")?;
    let staging = parent.join(format!(".botstral-{}.part", Uuid::new_v4()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&staging)
            .map_err(|_| "save_failed")?;
        file.write_all(&artifact.bytes).map_err(|_| "save_failed")?;
        file.sync_all().map_err(|_| "save_failed")?;
        fs::rename(&staging, destination).map_err(|_| "save_failed")?;
        fs::File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|_| "save_failed".into())
    })();
    if result.is_err() {
        let _ = fs::remove_file(staging);
    }
    result
}

pub fn parse_pairing_link(value: &str) -> Result<OwnerInvitation, String> {
    if value.len() > 65536 {
        return Err("invalid_invitation".into());
    }
    let encoded = value
        .strip_prefix("botstral://pair?bundle=")
        .ok_or("invalid_invitation")?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| "invalid_invitation")?;
    serde_json::from_slice(&bytes).map_err(|_| "invalid_invitation".into())
}

#[derive(Serialize)]
struct WorkspaceView<'a> {
    manager: ManagerId,
    cursor: u64,
    owner: &'a str,
    bots: &'a [WorkspaceBot],
    workers: &'a [WorkspaceWorker],
    sessions: &'a [ComputerSession],
    permissions: &'a [Permission],
    configurations: Vec<ConfigurationView<'a>>,
    vault_unlocked: bool,
    runs: Vec<RunViewModel<'a>>,
    tasks: Vec<TaskViewModel<'a>>,
}
#[derive(Serialize)]
struct ConfigurationView<'a> {
    bot: BotId,
    model: &'a str,
    ready: bool,
    context_tokens: u32,
}
#[derive(Serialize)]
struct RunViewModel<'a> {
    id: RunId,
    bot: BotId,
    prompt: &'a str,
    phase: &'static str,
    created_at: i64,
    reason: Option<String>,
    turns: Vec<TurnView<'a>>,
}
#[derive(Serialize)]
struct TurnView<'a> {
    id: RunTurnId,
    ordinal: u32,
    text: Option<&'a str>,
    task: Option<TaskId>,
    result: Option<&'a str>,
}
#[derive(Serialize)]
struct TaskViewModel<'a> {
    id: TaskId,
    bot: BotId,
    state: TaskState,
    progress: u32,
    action: String,
    approval_revoked: bool,
    approval_consumed: bool,
    outputs: &'a [OutputView],
    evidence: String,
}
#[derive(Serialize)]
struct ReviewView {
    id: Uuid,
    task: TaskId,
    action: String,
    expires_at: i64,
    permission_revision: u64,
}
fn describe_action(command: &CommandBody) -> String {
    let summary = match &command.work {
        WorkSpec::Computer { spec } => spec
            .operations
            .iter()
            .enumerate()
            .map(|(index, operation)| {
                let step = match operation {
                    ComputerOperation::Navigate { url } => format!("Navigate to {url:?}"),
                    ComputerOperation::Click { selector } => format!("Click {selector:?}"),
                    ComputerOperation::Fill { selector, text } => {
                        format!("Fill {selector:?} with {text:?}")
                    }
                    ComputerOperation::CaptureText { selector, output } => {
                        format!("Capture text from {selector:?} into output {output:?}")
                    }
                    ComputerOperation::Screenshot { output } => {
                        format!("Save a screenshot into output {output:?}")
                    }
                    ComputerOperation::WriteFile { path, text } => {
                        format!("Write file {path:?}\n   Exact contents: {text:?}")
                    }
                    ComputerOperation::ReadFile { path, output } => {
                        format!("Read file {path:?} into output {output:?}")
                    }
                    ComputerOperation::ExportFile { path, output } => {
                        format!("Export file {path:?} into output {output:?}")
                    }
                };
                format!("{}. {step}", index + 1)
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
        WorkSpec::Process { spec } => format!(
            "Run program {:?}\nExact arguments: {}\nWorking directory: {:?}",
            spec.executable,
            serde_json::to_string(&spec.args).expect("typed arguments serialize"),
            spec.cwd,
        ),
        WorkSpec::Vibe { spec } => format!(
            "Edit files in {:?}\nExact instruction: {:?}",
            spec.policy.workspace.path, spec.prompt,
        ),
        WorkSpec::TestInput { steps } => format!("Synthetic input fixture with {steps} steps"),
        WorkSpec::TestEffect { value, observable } => {
            format!("Synthetic effect fixture with value {value}, observable {observable}")
        }
        WorkSpec::Unavailable => "Execution is unavailable".into(),
    };
    format!(
        "{summary}\n\nExecution target\nWorker {}\nSession {}\n\nComplete execution details\n{}",
        command.worker,
        command.session,
        serde_json::to_string_pretty(command).expect("typed command serializes"),
    )
}
fn serialize_review<S: serde::Serializer>(
    value: &Option<ActionReview>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    value
        .as_ref()
        .map(|r| ReviewView {
            id: r.id,
            task: r.task,
            action: describe_action(&r.command),
            expires_at: r.expires_at,
            permission_revision: r.permission_revision,
        })
        .serialize(serializer)
}
fn serialize_workspace<S: serde::Serializer>(
    value: &Option<OwnerWorkspace>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    value
        .as_ref()
        .map(|w| WorkspaceView {
            manager: w.manager,
            cursor: w.cursor,
            owner: &w.owner,
            bots: &w.bots,
            workers: &w.workers,
            sessions: &w.sessions,
            permissions: &w.state.permissions,
            vault_unlocked: w.vault_unlocked,
            configurations: w
                .state
                .inference_configurations
                .iter()
                .map(|c| ConfigurationView {
                    bot: c.bot,
                    model: &c.policy.model_id,
                    ready: w.run_ready_bots.contains(&c.bot),
                    context_tokens: c.policy.context_tokens,
                })
                .collect(),
            runs: w
                .runs
                .iter()
                .map(|r| RunViewModel {
                    id: r.summary.id,
                    bot: r.summary.bot,
                    prompt: &r.request.prompt,
                    created_at: r.summary.created_at,
                    reason: match r.summary.phase {
                        RunPhase::Stopped { reason } => Some(format!("{reason:?}")),
                        RunPhase::AwaitingAttention { reason } => Some(format!("{reason:?}")),
                        _ => None,
                    },
                    phase: match r.summary.phase {
                        RunPhase::Ready => "ready",
                        RunPhase::Calling { .. } => "calling",
                        RunPhase::WaitingForTool { .. } => "waiting_for_tool",
                        RunPhase::Completed => "completed",
                        RunPhase::Cancelling => "cancelling",
                        RunPhase::Cancelled => "cancelled",
                        RunPhase::Stopped { .. } => "stopped",
                        RunPhase::AwaitingAttention { .. } => "awaiting_attention",
                    },
                    turns: r
                        .turns
                        .iter()
                        .map(|t| TurnView {
                            id: t.id,
                            ordinal: t.ordinal,
                            text: t.response.as_ref().and_then(|m| {
                                if let RunMessage::Assistant { content, .. } = m {
                                    Some(content.as_str())
                                } else {
                                    None
                                }
                            }),
                            task: t.intent.as_ref().map(|i| i.task),
                            result: t.intent.as_ref().and_then(|i| i.result.as_deref()),
                        })
                        .collect(),
                })
                .collect(),
            tasks: w
                .state
                .tasks
                .iter()
                .map(|t| TaskViewModel {
                    id: t.id,
                    bot: t.command.bot,
                    state: t.state,
                    progress: t.progress,
                    action: describe_action(&t.command),
                    approval_revoked: t.approval.revoked,
                    approval_consumed: t.approval.consumed,
                    outputs: &t.outputs,
                    evidence: serde_json::to_string_pretty(&t.effect_evidence)
                        .expect("typed evidence serializes"),
                })
                .collect(),
        })
        .serialize(serializer)
}
