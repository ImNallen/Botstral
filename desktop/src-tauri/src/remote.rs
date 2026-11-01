use base64::{Engine, engine::general_purpose::STANDARD};
use botstral_link::*;
use botstral_owner_client::{ClientError, OwnerClient, OwnerSession, SecretStore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs,
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::{mpsc, oneshot, watch};
use uuid::Uuid;

const MAX_JOURNAL_BYTES: usize = 1024 * 1024;
const MAX_JOURNAL_RECORDS: usize = 128;
const COMMAND_CAPACITY: usize = 16;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProtectedWriteRecord {
    manager: ManagerId,
    status: CredentialWriteStatus,
}

#[derive(Clone, Serialize)]
pub struct PairingCandidate {
    pub id: Uuid,
    pub manager: ManagerId,
    pub endpoint: String,
    pub pin: CertificateFingerprint,
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
#[derive(Clone, Default, Serialize)]
pub struct RoutinePanel {
    pub message: Option<String>,
    pub history: Vec<RoutineOccurrence>,
    pub preview: Vec<i64>,
}
#[derive(Clone, Serialize)]
pub struct RemoteSnapshot {
    pub routine_panel: RoutinePanel,
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
    pub session: Option<ComputerSessionId>,
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
    pending_file: Arc<Mutex<Option<PathBuf>>>,
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
enum ReadResult {
    Done(Result<(), String>),
    Interrupted(Option<Envelope>),
}
async fn finish_read(
    operation: impl std::future::Future<Output = Result<(), String>>,
    rx: &mut mpsc::Receiver<Envelope>,
    pending: &mut VecDeque<Envelope>,
) -> ReadResult {
    tokio::pin!(operation);
    loop {
        tokio::select! {
            result = &mut operation => return ReadResult::Done(result),
            next = rx.recv() => match next {
                Some(next) if matches!(&next.command, IntentCommand::CredentialUses { .. } | IntentCommand::LoginItems { .. }) => {
                    if pending.len() < COMMAND_CAPACITY {
                        pending.push_back(next);
                    } else {
                        let _ = next.reply.send(Err("busy".into()));
                    }
                },
                next => return ReadResult::Interrupted(next),
            },
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
pub struct OwnerReadScope {
    pub generation: u64,
    pub manager: ManagerId,
}
#[derive(Serialize)]
pub struct SavedResults {
    pub generation: u64,
    pub manager: ManagerId,
    pub page: ArtifactPage,
}
#[derive(Serialize)]
pub struct SourceTask {
    pub generation: u64,
    pub manager: ManagerId,
    pub task: TaskViewModel,
}
pub enum IntentCommand {
    Artifacts {
        query: ArtifactQuery,
        result: oneshot::Sender<Result<SavedResults, String>>,
    },
    SourceTask {
        id: TaskId,
        result: oneshot::Sender<Result<SourceTask, String>>,
    },
    CredentialUses {
        result: oneshot::Sender<Result<Vec<CredentialUseReview>, String>>,
    },
    DecideCredentialUse {
        review: Box<CredentialUseReview>,
        decision: UseDecision,
    },
    LockVault,
    Protected {
        generation: u64,
        request: protected::ProtectedOwnerRequest,
        result: oneshot::Sender<Result<CredentialWriteStatus, String>>,
    },
    ProtectedStatus {
        result: oneshot::Sender<Result<Option<CredentialWriteStatus>, String>>,
    },
    LoginItems {
        result: oneshot::Sender<Result<Vec<LoginItemView>, String>>,
    },
    UpdateLogin {
        id: CredentialId,
        expected_revision: CredentialRevision,
        metadata: Option<LoginMetadata>,
    },
    Routine(RoutineCommand),
    RunDetails(RunId),
    RoutineText {
        bot: BotId,
        text: String,
    },
    RoutineHistory {
        id: RoutineId,
        before: Option<OccurrenceId>,
    },
    RoutinePreview {
        trigger: RoutineTrigger,
        after: i64,
    },
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
    revoked: bool,
    reconnect_attempt: u32,
    pending_file: Arc<Mutex<Option<PathBuf>>>,
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
    if file.metadata().map_err(|_| "storage")?.len() > MAX_JOURNAL_BYTES as u64 {
        return Err("storage".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_JOURNAL_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "storage")?;
    if bytes.len() > MAX_JOURNAL_BYTES {
        return Err("storage".into());
    }
    let intents: Vec<Intent> = serde_json::from_slice(&bytes).map_err(|_| "storage")?;
    if intents.len() > MAX_JOURNAL_RECORDS
        || intents.iter().any(|i| {
            !matches!(
                i.status.as_str(),
                "unknown" | "pending" | "confirmed" | "attention" | "refused"
            ) || !matches!(
                &i.operation,
                OwnerOperation::RoutineCommand { .. }
                    | OwnerOperation::RoutineConversation { .. }
                    | OwnerOperation::SubmitRun { .. }
                    | OwnerOperation::CancelRun { .. }
                    | OwnerOperation::CancelTask { .. }
                    | OwnerOperation::GrantReviewedApproval { .. }
                    | OwnerOperation::RevokeApproval { .. }
                    | OwnerOperation::SetPermission { .. }
                    | OwnerOperation::OpenComputerSession { .. }
            ) && !matches!(&i.operation, OwnerOperation::WriteDocument { input } if input.kind == DocumentKind::Memory)
        })
    {
        return Err("storage".into());
    }
    check_journal_budget(&intents)?;
    Ok(intents)
}
fn check_journal_budget(intents: &[Intent]) -> Result<(), String> {
    let bytes = serde_json::to_vec(intents).map_err(|_| "storage")?;
    let reserved_status_bytes = 2 * intents
        .iter()
        .filter(|i| matches!(i.status.as_str(), "unknown" | "pending"))
        .count();
    if intents.len() > MAX_JOURNAL_RECORDS
        || bytes.len().saturating_add(reserved_status_bytes) > MAX_JOURNAL_BYTES
    {
        return Err("journal_full".into());
    }
    Ok(())
}
pub(crate) fn read_attached_file(path: &Path) -> Result<(String, Vec<u8>), String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| attachment_name(name))
        .ok_or("invalid_input")?
        .to_string();
    let meta = fs::symlink_metadata(path).map_err(|_| "invalid_input")?;
    if !meta.file_type().is_file() || meta.nlink() != 1 {
        return Err("invalid_input".into());
    }
    let len = meta.len();
    if len == 0 || len > MAX_RUN_FILE_BYTES as u64 {
        return Err("invalid_input".into());
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| "invalid_input")?;
    let opened = file.metadata().map_err(|_| "invalid_input")?;
    if !opened.is_file() || opened.nlink() != 1 || opened.len() != len {
        return Err("invalid_input".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_RUN_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "invalid_input")?;
    if bytes.len() as u64 != len {
        return Err("invalid_input".into());
    }
    Ok((name, bytes))
}

fn attachment_name(name: &str) -> bool {
    (1..=120).contains(&name.len())
        && !name.contains(['/', '\\', '\0'])
        && name != "."
        && name != ".."
        && Path::new(name).file_name().and_then(|part| part.to_str()) == Some(name)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0xf) as usize] as char);
    }
    encoded
}

fn error_code(error: ClientError) -> String {
    match error {
        ClientError::Trust => "trust",
        ClientError::Storage => "keychain_or_storage",
        ClientError::Authentication(OwnerError::Unauthorized) => "revoked",
        ClientError::Authentication(OwnerError::Protocol(ErrorCode::IncompatibleVersion)) => {
            "incompatible_manager"
        }
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
fn latest_configuration(workspace: &OwnerWorkspace, bot: BotId) -> Option<&InferenceConfiguration> {
    workspace
        .state
        .inference_configurations
        .iter()
        .filter(|configuration| configuration.bot == bot)
        .max_by_key(|configuration| configuration.revision)
}
impl RemoteService {
    pub fn new(root: PathBuf, store: Arc<dyn SecretStore>) -> Self {
        let (view, _) = watch::channel(RemoteSnapshot {
            routine_panel: RoutinePanel::default(),
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
            pending_file: Arc::new(Mutex::new(None)),
        }
    }
    pub fn remember_pending_file(&self, path: PathBuf) {
        *self.pending_file.lock().unwrap() = Some(path);
    }
    pub fn clear_pending_file(&self) {
        *self.pending_file.lock().unwrap() = None;
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
        let (tx, rx) = mpsc::channel(COMMAND_CAPACITY);
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
            revoked: false,
            reconnect_attempt: 0,
            pending_file: self.pending_file.clone(),
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
    pub async fn protected(
        &self,
        generation: u64,
        request: protected::ProtectedOwnerRequest,
    ) -> Result<CredentialWriteStatus, String> {
        let (result, receiver) = oneshot::channel();
        self.command(IntentCommand::Protected {
            generation,
            request,
            result,
        })
        .await?;
        receiver.await.map_err(|_| "unresolved".to_string())?
    }
    pub async fn protected_status(&self) -> Result<Option<CredentialWriteStatus>, String> {
        let (result, receiver) = oneshot::channel();
        self.command(IntentCommand::ProtectedStatus { result })
            .await?;
        receiver.await.map_err(|_| "unresolved".to_string())?
    }
    pub async fn credential_uses(&self) -> Result<Vec<CredentialUseReview>, String> {
        let (result, receiver) = oneshot::channel();
        self.command(IntentCommand::CredentialUses { result })
            .await?;
        receiver.await.map_err(|_| "offline".to_string())?
    }
    pub async fn login_items(&self) -> Result<Vec<LoginItemView>, String> {
        let (result, receiver) = oneshot::channel();
        self.command(IntentCommand::LoginItems { result }).await?;
        receiver.await.map_err(|_| "offline".to_string())?
    }
    pub fn owner_scope(&self) -> Result<OwnerReadScope, String> {
        let view = self.inspect();
        if view.status != "online" || self.signing_out.load(Ordering::Acquire) {
            return Err("stale".into());
        }
        Ok(OwnerReadScope {
            generation: view.generation,
            manager: view.workspace.ok_or("stale")?.manager,
        })
    }
    pub async fn saved_results(&self, query: ArtifactQuery) -> Result<SavedResults, String> {
        let (result, receiver) = oneshot::channel();
        self.command(IntentCommand::Artifacts { query, result })
            .await?;
        receiver.await.map_err(|_| "offline".to_string())?
    }
    pub async fn source_task(&self, id: TaskId) -> Result<SourceTask, String> {
        let (result, receiver) = oneshot::channel();
        self.command(IntentCommand::SourceTask { id, result })
            .await?;
        receiver.await.map_err(|_| "offline".to_string())?
    }
    pub async fn save_export(
        &self,
        scope: OwnerReadScope,
        task: TaskId,
        destination: &Path,
        artifact: VerifiedArtifact,
    ) -> Result<(), String> {
        if self.owner_scope()? != scope {
            return Err("stale".into());
        }
        let authority = self.source_task(task).await?;
        if authority.generation != scope.generation
            || authority.manager != scope.manager
            || self.owner_scope()? != scope
        {
            return Err("stale".into());
        }
        save_artifact(destination, artifact)
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
        let _gate = self.signout_gate.lock().await;
        self.signing_out.store(true, Ordering::Release);
        self.view.send_modify(|v| {
            v.generation += 1;
            v.revision += 1;
            v.status = "signed_out".into();
            v.workspace = None;
            v.routine_panel = RoutinePanel::default();
            v.review = None;
            v.pairing = None;
            v.candidate = None;
            v.intents.clear();
            v.issue = None;
        });
        self.shutdown().await;
        let written = fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.root)
            .map_err(|_| "sign_out_storage_failed".to_string())
            .and_then(|()| storage(&self.root, "signed-out.json", &true));
        let deleted = self.store.delete();
        if written.is_err() {
            self.view
                .send_modify(|v| v.issue = Some("sign_out_storage_failed".into()));
            return Err("sign_out_storage_failed".into());
        }
        if deleted.is_err() {
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
        let mut pending = VecDeque::new();
        loop {
            let envelope = if !pending.is_empty() {
                pending.pop_front()
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
            let result = if matches!(
                &envelope.command,
                IntentCommand::Fetch { .. }
                    | IntentCommand::Artifacts { .. }
                    | IntentCommand::SourceTask { .. }
            ) {
                let outcome =
                    finish_read(self.execute(envelope.command), &mut rx, &mut pending).await;
                match outcome {
                    ReadResult::Done(result) => result,
                    ReadResult::Interrupted(next) => {
                        self.session = None;
                        if let Some(next) = next {
                            pending.push_front(next);
                        }
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
                v.routine_panel = RoutinePanel::default();
                v.issue = failed.then(|| "credential_deletion_failed".into());
            });
            return Ok(());
        }
        if self.root.join("revoked.json").exists() {
            self.revoked = true;
            return Err("revoked".into());
        }
        if self.store.read().map_err(error_code)?.is_none() {
            return Ok(());
        }
        let client = OwnerClient::open_with(&self.root, self.store.clone()).map_err(error_code)?;
        let intents =
            read_journal(&self.root.join("intents.json")).map_err(|_| "journal_unavailable")?;
        let image = client.installed_workspace().map_err(error_code)?;
        let pairing = client
            .trust()
            .is_none()
            .then(|| client.pairing_request("Pending desktop".into()));
        self.client = Some(client);
        self.intents = intents;
        if let Some(pairing) = pairing {
            self.publish(|v| {
                v.status = "pairing".into();
                v.pairing = Some(pairing);
            });
        }
        if let Some(image) = image {
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
        let error = if error == "revoked" {
            self.revoked = true;
            self.client = None;
            self.candidate = None;
            if !self.root.join("revoked.json").exists()
                && storage(&self.root, "revoked.json", &true).is_err()
            {
                let deleted = self.store.delete();
                let cleared = fs::remove_file(self.root.join("workspace.json"));
                if deleted.is_err() && cleared.is_err() {
                    "revocation_storage_failed".into()
                } else {
                    error
                }
            } else {
                error
            }
        } else {
            error
        };
        self.publish(|v| {
            v.status = if error == "offline" || error == "unknown_outcome" {
                "offline"
            } else {
                "blocked"
            }
            .into();
            if self.revoked {
                v.workspace = None;
                v.routine_panel = RoutinePanel::default();
                v.review = None;
                v.pairing = None;
                v.candidate = None;
                v.intents.clear();
            }
            v.issue = Some(error);
        });
    }
    async fn recover(&mut self) -> Result<(), String> {
        if self.view.borrow().generation != self.generation {
            return Err("stale".into());
        }
        if self.revoked {
            return Err("revoked".into());
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
        let mut image = client
            .recover_workspace(self.session.as_mut().ok_or("offline")?)
            .await
            .map_err(error_code)?;
        let routine_intents = self
            .intents
            .iter()
            .filter_map(|i| {
                let request = match &i.operation {
                    OwnerOperation::RoutineCommand { input } => input.id,
                    OwnerOperation::RoutineConversation { id, .. } => *id,
                    _ => return None,
                };
                (!matches!(i.status.as_str(), "confirmed" | "refused")).then_some((
                    i.id,
                    request,
                    i.operation.clone(),
                ))
            })
            .collect::<Vec<_>>();
        let mut replayed = false;
        let mut routine_message = None;
        for (intent_id, request, operation) in routine_intents {
            if self.view.borrow().generation != self.generation {
                return Err("stale".into());
            }
            let confirmed = match self
                .session
                .as_mut()
                .ok_or("offline")?
                .call(operation)
                .await
            {
                Ok(OwnerReply::RoutineControl(result)) if result.request == request => {
                    replayed = true;
                    routine_message = Some(result.message);
                    true
                }
                Err(ClientError::Refused(_)) => {
                    if let Some(i) = self.intents.iter_mut().find(|i| i.id == intent_id) {
                        i.status = "refused".into();
                    }
                    false
                }
                Err(error) => return Err(error_code(error)),
                _ => return Err("incompatible_manager".into()),
            };
            if confirmed && let Some(i) = self.intents.iter_mut().find(|i| i.id == intent_id) {
                i.status = "confirmed".into();
            }
        }
        if replayed {
            image = client
                .recover_workspace(self.session.as_mut().ok_or("offline")?)
                .await
                .map_err(error_code)?;
        }
        if let Some(message) = routine_message {
            self.publish(|v| v.routine_panel.message = Some(message));
        }
        for intent in &mut self.intents {
            if matches!(intent.status.as_str(), "refused" | "confirmed") {
                continue;
            }
            let run_id = match &intent.operation {
                OwnerOperation::SubmitRun { input } => Some(input.id),
                OwnerOperation::CancelRun { id } => Some(*id),
                _ => None,
            };
            let recovered_run = if let Some(id) = run_id {
                match self
                    .session
                    .as_mut()
                    .ok_or("offline")?
                    .call(OwnerOperation::Run { id })
                    .await
                {
                    Ok(OwnerReply::Run(run)) if run.summary.id == id => Some(run),
                    Err(ClientError::Refused(OwnerError::NotFound)) => None,
                    Err(error) => return Err(error_code(error)),
                    _ => return Err("incompatible_manager".into()),
                }
            } else {
                None
            };
            let task_id = match &intent.operation {
                OwnerOperation::CancelTask { id }
                | OwnerOperation::GrantReviewedApproval { id, .. }
                | OwnerOperation::RevokeApproval { id } => Some(*id),
                _ => None,
            };
            let recovered_task = if let Some(id) = task_id {
                match self
                    .session
                    .as_mut()
                    .ok_or("offline")?
                    .call(OwnerOperation::Task { id })
                    .await
                {
                    Ok(OwnerReply::Task(task)) if task.id == id => Some(task),
                    Err(ClientError::Refused(OwnerError::NotFound)) => None,
                    Err(error) => return Err(error_code(error)),
                    _ => return Err("incompatible_manager".into()),
                }
            } else {
                None
            };
            let confirmed = match &intent.operation {
                OwnerOperation::SubmitRun { input } => {
                    recovered_run.is_some_and(|run| run.request == **input)
                }
                OwnerOperation::CancelRun { .. } => {
                    let Some(run) = recovered_run else {
                        continue;
                    };
                    let mut stopped = matches!(
                        run.summary.phase,
                        RunPhase::Completed | RunPhase::Cancelled | RunPhase::Stopped { .. }
                    );
                    if stopped {
                        for task in run
                            .turns
                            .iter()
                            .filter_map(|turn| turn.intent.as_ref().map(|i| i.task))
                        {
                            let terminal = if let Some(task) =
                                image.state.tasks.iter().find(|t| t.id == task)
                            {
                                task.state.terminal()
                            } else {
                                match self
                                    .session
                                    .as_mut()
                                    .ok_or("offline")?
                                    .call(OwnerOperation::Task { id: task })
                                    .await
                                {
                                    Ok(OwnerReply::Task(view)) if view.id == task => {
                                        view.state.terminal()
                                    }
                                    Err(ClientError::Refused(OwnerError::NotFound)) => false,
                                    Err(error) => return Err(error_code(error)),
                                    _ => return Err("incompatible_manager".into()),
                                }
                            };
                            stopped &= terminal;
                        }
                    }
                    stopped
                }
                OwnerOperation::CancelTask { .. } => {
                    recovered_task.is_some_and(|t| t.state.terminal())
                }
                OwnerOperation::GrantReviewedApproval {
                    id,
                    command,
                    approval,
                    permission_revision,
                    expires_at,
                } => recovered_task.is_some_and(|t| {
                    t.id == *id
                        && t.command == **command
                        && t.approval.id == approval.id
                        && !t.approval.revoked
                        && t.approval.binding.as_ref().is_some_and(|b| {
                            b.permission_revision == *permission_revision
                                && b.approval == approval.id
                                && b.expires_at == *expires_at
                                && t.command.matches_authority(b)
                        })
                }),
                OwnerOperation::RevokeApproval { .. } => {
                    recovered_task.is_some_and(|t| t.approval.revoked)
                }
                OwnerOperation::WriteDocument { .. } => {
                    intent.status = "attention".into();
                    false
                }
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
        self.save_intents(&self.intents)?;
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
                    OwnerOperation::RoutineCommand { input } => input.id.to_string(),
                    OwnerOperation::RoutineConversation { id, .. } => id.to_string(),
                    OwnerOperation::SubmitRun { input } => input.id.to_string(),
                    OwnerOperation::CancelRun { id } => id.to_string(),
                    OwnerOperation::CancelTask { id }
                    | OwnerOperation::GrantReviewedApproval { id, .. }
                    | OwnerOperation::RevokeApproval { id } => id.to_string(),
                    _ => i.id.to_string(),
                },
                description: match &i.operation {
                    OwnerOperation::RoutineCommand { .. }
                    | OwnerOperation::RoutineConversation { .. } => "Routine owner command",
                    OwnerOperation::SubmitRun { .. } => "Run submission",
                    OwnerOperation::CancelRun { .. } | OwnerOperation::CancelTask { .. } => {
                        "Cancellation requested"
                    }
                    OwnerOperation::GrantReviewedApproval { .. } => "Approval",
                    OwnerOperation::RevokeApproval { .. } => "Approval revocation",
                    OwnerOperation::WriteDocument { .. } => "Legacy memory edit remains uncertain. This obsolete write will not be replayed",
                    OwnerOperation::SetPermission { .. } => "Permission change",
                    OwnerOperation::OpenComputerSession { .. } => "Computer connection",
                    _ => "Operation",
                }
                .into(),
            })
            .collect()
    }
    fn save_intents(&self, intents: &[Intent]) -> Result<(), String> {
        check_journal_budget(intents)?;
        storage(&self.root, "intents.json", &intents)
    }
    async fn stage_attached(&mut self, id: RunId, path: &Path) -> Result<(), String> {
        let (name, bytes) = read_attached_file(path)?;
        let reply = self
            .session
            .as_mut()
            .ok_or("offline")?
            .call(OwnerOperation::StageRunFile {
                input: StageRunFile {
                    id,
                    name,
                    sha256: CertificateFingerprint::from_der(&bytes),
                    hex: hex_encode(&bytes),
                },
            })
            .await
            .map_err(error_code)?;
        if !matches!(reply, OwnerReply::RunFileStaged) {
            return Err("incompatible_manager".into());
        }
        Ok(())
    }
    async fn routine_text(&mut self, bot: BotId, text: String) -> Result<(), String> {
        self.recover().await?;
        let operation = OwnerOperation::RoutineConversation {
            bot,
            id: RequestId::new(),
            text: text.clone(),
        };
        if text == "/routine help"
            || text == "/routine list"
            || text.starts_with("/routine history ")
        {
            let reply = self
                .session
                .as_mut()
                .ok_or("offline")?
                .call(operation)
                .await
                .map_err(error_code)?;
            let OwnerReply::RoutineControl(result) = reply else {
                return Err("protocol".into());
            };
            self.publish(|v| v.routine_panel.message = Some(result.message));
            return Ok(());
        }
        self.mutate(operation).await
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
        let id = Uuid::new_v4();
        let mut next = self.intents.clone();
        next.push(Intent {
            id,
            operation: operation.clone(),
            status: "unknown".into(),
        });
        if check_journal_budget(&next).is_err() {
            next.retain(|i| !matches!(i.status.as_str(), "confirmed" | "refused"));
        }
        self.save_intents(&next)?;
        self.intents = next;
        self.publish(|v| v.intents = self.intent_views());
        let routine_request = match &operation {
            OwnerOperation::RoutineCommand { input } => Some(input.id),
            OwnerOperation::RoutineConversation { id, .. } => Some(*id),
            _ => None,
        };
        let reply = self
            .session
            .as_mut()
            .ok_or("offline")?
            .call(operation)
            .await;
        let routine_ack = matches!(&reply,Ok(OwnerReply::RoutineControl(result)) if Some(result.request)==routine_request);
        if routine_request.is_some() && reply.is_ok() && !routine_ack {
            return Err("incompatible_manager".into());
        }
        if let Ok(OwnerReply::RoutineControl(result)) = &reply {
            self.publish(|v| v.routine_panel.message = Some(result.message.clone()));
        }
        if let Err(error) = reply {
            if matches!(error, ClientError::Refused(_)) {
                if matches!(
                    self.intents.last().ok_or("storage")?.operation,
                    OwnerOperation::SubmitRun { .. }
                ) {
                    self.intents.pop();
                } else {
                    self.intents.last_mut().ok_or("storage")?.status = "refused".into();
                }
                self.save_intents(&self.intents)?;
                self.publish(|v| v.intents = self.intent_views());
            }
            return Err(error_code(error));
        }
        self.intents.last_mut().ok_or("storage")?.status =
            if routine_ack { "confirmed" } else { "pending" }.into();
        self.save_intents(&self.intents)
            .map_err(|_| "unknown_outcome")?;
        self.recover().await.map_err(|_| "unknown_outcome".into())
    }
    fn read_scope(&self) -> Result<OwnerReadScope, String> {
        let view = self.view.borrow();
        if view.generation != self.generation
            || view.status != "online"
            || self.revoked
            || self.suspended
        {
            return Err("stale".into());
        }
        Ok(OwnerReadScope {
            generation: self.generation,
            manager: view.workspace.as_ref().ok_or("offline")?.manager,
        })
    }
    async fn retained_task(&mut self, id: TaskId) -> Result<TaskView, String> {
        let OwnerReply::Task(task) = self
            .session
            .as_mut()
            .ok_or("offline")?
            .call(OwnerOperation::Task { id })
            .await
            .map_err(error_code)?
        else {
            return Err("protocol".into());
        };
        if task.id != id {
            return Err("protocol".into());
        }
        Ok(*task)
    }
    async fn read_artifacts(&mut self, query: ArtifactQuery) -> Result<SavedResults, String> {
        self.recover().await?;
        let scope = self.read_scope()?;
        let OwnerReply::Artifacts(page) = self
            .session
            .as_mut()
            .ok_or("offline")?
            .call(OwnerOperation::Artifacts { query })
            .await
            .map_err(error_code)?
        else {
            return Err("protocol".into());
        };
        if self.read_scope()? != scope {
            return Err("stale".into());
        }
        Ok(SavedResults {
            generation: scope.generation,
            manager: scope.manager,
            page,
        })
    }
    async fn read_source(&mut self, id: TaskId) -> Result<SourceTask, String> {
        self.recover().await?;
        let scope = self.read_scope()?;
        let task = self.retained_task(id).await?;
        if self.read_scope()? != scope {
            return Err("stale".into());
        }
        Ok(SourceTask {
            generation: scope.generation,
            manager: scope.manager,
            task: TaskViewModel::from(&task),
        })
    }
    async fn fetch(
        &mut self,
        task: TaskId,
        artifact: ArtifactId,
    ) -> Result<VerifiedArtifact, String> {
        self.recover().await?;
        let retained = self.retained_task(task).await?;
        let output = retained
            .outputs
            .iter()
            .find(|o| matches!(&o.state, ArtifactState::Available { id, .. } if *id == artifact))
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
    async fn protected(
        &mut self,
        generation: u64,
        request: protected::ProtectedOwnerRequest,
    ) -> Result<CredentialWriteStatus, String> {
        if generation != self.generation
            || self.view.borrow().generation != generation
            || self.suspended
            || self.revoked
        {
            return Err("stale".into());
        }
        self.recover().await?;
        if matches!(
            &request.operation,
            protected::ProtectedOwnerOperation::Unlock { .. }
        ) {
            return self
                .session
                .as_mut()
                .ok_or("offline")?
                .protected(request)
                .await
                .map_err(error_code);
        }
        let manager = self.protected_manager()?;
        if let Some(previous) = self.protected_record()?
            && matches!(previous.status, CredentialWriteStatus::Unresolved { .. })
        {
            return Err(if previous.manager == manager {
                "reconcile_previous_entry"
            } else {
                "receipt_manager_mismatch"
            }
            .into());
        }
        let id = request.id;
        storage(
            &self.root,
            "protected-status.json",
            &ProtectedWriteRecord {
                manager,
                status: CredentialWriteStatus::Unresolved { request: id },
            },
        )?;
        let status = match self
            .session
            .as_mut()
            .ok_or("offline")?
            .protected(request)
            .await
        {
            Ok(status) => status,
            Err(_) => {
                self.session = None;
                return Ok(CredentialWriteStatus::Unresolved { request: id });
            }
        };
        storage(
            &self.root,
            "protected-status.json",
            &ProtectedWriteRecord {
                manager,
                status: status.clone(),
            },
        )?;
        Ok(status)
    }
    fn protected_manager(&self) -> Result<ManagerId, String> {
        self.client
            .as_ref()
            .and_then(|client| client.trust())
            .map(|trust| trust.manager)
            .ok_or_else(|| "offline".into())
    }
    fn protected_record(&self) -> Result<Option<ProtectedWriteRecord>, String> {
        let path = self.root.join("protected-status.json");
        if !path.exists() {
            return Ok(None);
        }
        let bytes = botstral_link::tls::private_bytes(&path).map_err(|_| "storage")?;
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| "receipt_provenance_unknown".into())
    }
    async fn protected_status(&mut self) -> Result<Option<CredentialWriteStatus>, String> {
        self.recover().await?;
        let manager = self.protected_manager()?;
        let Some(record) = self.protected_record()? else {
            return Ok(None);
        };
        if record.manager != manager {
            return if matches!(record.status, CredentialWriteStatus::Unresolved { .. }) {
                Err("receipt_manager_mismatch".into())
            } else {
                Ok(None)
            };
        }
        let status = record.status;
        let CredentialWriteStatus::Unresolved { request } = status else {
            return Ok(Some(status));
        };
        let OwnerReply::CredentialWriteStatus(status) = self
            .session
            .as_mut()
            .ok_or("offline")?
            .call(OwnerOperation::CredentialWriteStatus {
                request,
                close: true,
            })
            .await
            .map_err(error_code)?
        else {
            return Err("protocol".into());
        };
        storage(
            &self.root,
            "protected-status.json",
            &ProtectedWriteRecord {
                manager,
                status: status.clone(),
            },
        )?;
        Ok(Some(status))
    }
    async fn execute(&mut self, command: IntentCommand) -> Result<(), String> {
        match command {
            IntentCommand::Protected {
                generation,
                request,
                result,
            } => {
                let value = self.protected(generation, request).await;
                let _ = result.send(value);
                Ok(())
            }
            IntentCommand::ProtectedStatus { result } => {
                let value = self.protected_status().await;
                let _ = result.send(value);
                Ok(())
            }
            IntentCommand::CredentialUses { result } => {
                self.recover().await?;
                let value = match self
                    .session
                    .as_mut()
                    .ok_or("offline")?
                    .call(OwnerOperation::CredentialUses)
                    .await
                {
                    Ok(OwnerReply::CredentialUses(reviews)) => Ok(reviews),
                    Err(ClientError::Refused(OwnerError::Locked)) => Err("vault_locked".into()),
                    Err(error) => return Err(error_code(error)),
                    Ok(_) => Err("protocol".into()),
                };
                let _ = result.send(value);
                Ok(())
            }
            IntentCommand::DecideCredentialUse { review, decision } => {
                self.recover().await?;
                self.session
                    .as_mut()
                    .ok_or("offline")?
                    .call(OwnerOperation::DecideCredentialUse { review, decision })
                    .await
                    .map_err(error_code)?;
                Ok(())
            }
            IntentCommand::LockVault => {
                self.recover().await?;
                self.session
                    .as_mut()
                    .ok_or("offline")?
                    .call(OwnerOperation::LockVault)
                    .await
                    .map_err(error_code)?;
                Ok(())
            }
            IntentCommand::LoginItems { result } => {
                self.recover().await?;
                let value = match self
                    .session
                    .as_mut()
                    .ok_or("offline")?
                    .call(OwnerOperation::LoginItems)
                    .await
                {
                    Ok(OwnerReply::LoginItems(items)) => Ok(items),
                    Err(ClientError::Refused(OwnerError::Locked)) => Err("vault_locked".into()),
                    Err(error) => return Err(error_code(error)),
                    Ok(_) => Err("protocol".into()),
                };
                let _ = result.send(value);
                Ok(())
            }
            IntentCommand::UpdateLogin {
                id,
                expected_revision,
                metadata,
            } => {
                self.recover().await?;
                self.session
                    .as_mut()
                    .ok_or("offline")?
                    .call(OwnerOperation::UpdateLogin {
                        id,
                        expected_revision,
                        metadata,
                    })
                    .await
                    .map_err(error_code)?;
                Ok(())
            }
            IntentCommand::Artifacts { query, result } => {
                let value = self.read_artifacts(query).await;
                let status = value.as_ref().map(|_| ()).map_err(Clone::clone);
                let _ = result.send(value);
                status
            }
            IntentCommand::SourceTask { id, result } => {
                let value = self.read_source(id).await;
                let status = value.as_ref().map(|_| ()).map_err(Clone::clone);
                let _ = result.send(value);
                status
            }
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
                if label.is_empty() || label.len() > 100 {
                    return Err("invalid_input".into());
                }
                self.session = None;
                self.client = None;
                let revoked = self.revoked || self.root.join("revoked.json").exists();
                let signed_out = self.root.join("signed-out.json").exists();
                let intents = if revoked || signed_out {
                    self.store.delete().map_err(error_code)?;
                    for name in ["workspace.json", "state.json"] {
                        let _ = fs::remove_file(self.root.join(name));
                    }
                    Vec::new()
                } else {
                    let journal = self.root.join("intents.json");
                    let has_journal = match fs::symlink_metadata(&journal) {
                        Ok(_) => true,
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                        Err(_) => return Err("journal_unavailable".into()),
                    };
                    if self.store.read().map_err(error_code)?.is_some() || has_journal {
                        read_journal(&journal).map_err(|_| "journal_unavailable")?
                    } else {
                        Vec::new()
                    }
                };
                let pairing = OwnerClient::initialize_with(&self.root, label, self.store.clone())
                    .map_err(error_code)?;
                let client =
                    OwnerClient::open_with(&self.root, self.store.clone()).map_err(error_code)?;
                self.save_intents(&intents)?;
                if signed_out {
                    fs::remove_file(self.root.join("signed-out.json")).map_err(|_| "storage")?;
                }
                if self.root.join("revoked.json").exists() {
                    fs::remove_file(self.root.join("revoked.json")).map_err(|_| "storage")?;
                }
                self.revoked = false;
                self.client = Some(client);
                self.intents = intents;
                if revoked {
                    self.generation += 1;
                    self.view.send_modify(|v| {
                        v.generation = self.generation;
                        v.workspace = None;
                        v.routine_panel = RoutinePanel::default();
                        v.review = None;
                        v.candidate = None;
                        v.pairing = None;
                        v.intents.clear();
                    });
                }
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
                    pin: bundle.trust.pin.clone(),
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
                    pin: trust.pin.clone(),
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
            IntentCommand::RunDetails(id) => {
                self.recover().await?;
                let reply = self
                    .session
                    .as_mut()
                    .ok_or("offline")?
                    .call(OwnerOperation::Run { id })
                    .await
                    .map_err(error_code)?;
                let OwnerReply::Run(run) = reply else {
                    return Err("protocol".into());
                };
                let run = WorkspaceRun::project(*run, false);
                self.publish(|v| {
                    if let Some(workspace) = &mut v.workspace
                        && let Some(prior) = workspace.runs.iter_mut().find(|r| r.summary.id == id)
                    {
                        *prior = run;
                    }
                });
                Ok(())
            }
            IntentCommand::RoutineText { bot, text } => self.routine_text(bot, text).await,
            IntentCommand::Routine(input) => {
                self.mutate(OwnerOperation::RoutineCommand { input }).await
            }
            IntentCommand::RoutineHistory { id, before } => {
                self.recover().await?;
                let reply = self
                    .session
                    .as_mut()
                    .ok_or("offline")?
                    .call(OwnerOperation::RoutineHistory {
                        id,
                        before,
                        limit: 32,
                    })
                    .await
                    .map_err(error_code)?;
                let OwnerReply::RoutineHistory(history) = reply else {
                    return Err("protocol".into());
                };
                self.publish(|v| v.routine_panel.history = history);
                Ok(())
            }
            IntentCommand::RoutinePreview { trigger, after } => {
                self.recover().await?;
                let reply = self
                    .session
                    .as_mut()
                    .ok_or("offline")?
                    .call(OwnerOperation::RoutinePreview {
                        trigger,
                        after,
                        limit: 5,
                    })
                    .await
                    .map_err(error_code)?;
                let OwnerReply::RoutinePreview(preview) = reply else {
                    return Err("protocol".into());
                };
                self.publish(|v| v.routine_panel.preview = preview);
                Ok(())
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
                self.save_intents(&self.intents)?;
                self.publish(|v| v.intents = self.intent_views());
                Ok(())
            }
            IntentCommand::RevokeApproval(id) => {
                self.mutate(OwnerOperation::RevokeApproval { id }).await
            }
            IntentCommand::Send(input) => {
                let pending = self.pending_file.lock().unwrap().take();
                self.recover().await?;
                if input.text.starts_with("/routine ") {
                    return self.routine_text(input.bot, input.text).await;
                }
                let image = self.view.borrow().workspace.clone().ok_or("unpaired")?;
                if !image.run_ready_bots.contains(&input.bot) {
                    return Err("inference_unverified".into());
                }
                if !image.vault_unlocked {
                    return Err("vault_locked".into());
                }
                let config =
                    latest_configuration(&image, input.bot).ok_or("configure_inference_locally")?;
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
                        processes: 128,
                        memory_bytes: 1024 * 1024 * 1024,
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
                let id = RunId::new();
                if let Some(path) = pending {
                    self.stage_attached(id, &path).await?;
                }
                self.mutate(OwnerOperation::SubmitRun {
                    input: Box::new(SubmitRun {
                        images: vec![],
                        id,
                        bot: input.bot,
                        session: input.session,
                        configuration_revision: config.revision,
                        prompt: input.text,
                        execution_policy: RunExecutionPolicy {
                            connectors: None,
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

pub enum PairingLink {
    Bundle(OwnerInvitation),
    Code(String),
}

pub fn parse_pairing_link(value: &str) -> Result<PairingLink, String> {
    let value = value.trim();
    if value.len() > 65536 {
        return Err("invalid_invitation".into());
    }
    if let Some(raw) = value.strip_prefix("bj1/") {
        let code = JoinCode::parse(&format!("bj1/{raw}")).map_err(|_| "invalid_invitation")?;
        if code.role() != PeerRole::Device {
            return Err("invalid_invitation".into());
        }
        return Ok(PairingLink::Code(format!("bj1/{raw}")));
    }
    let encoded = value
        .strip_prefix("botstral://pair?bundle=")
        .ok_or("invalid_invitation")?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| "invalid_invitation")?;
    serde_json::from_slice(&bytes)
        .map(PairingLink::Bundle)
        .map_err(|_| "invalid_invitation".into())
}

pub async fn stage_join_code(
    service: &RemoteService,
    code: &str,
) -> Result<RemoteSnapshot, String> {
    let fingerprint = service
        .inspect()
        .pairing
        .ok_or("begin_pairing_first")?
        .fingerprint;
    let code = JoinCode::parse(code.trim()).map_err(|_| "invalid_invitation")?;
    if code.role() != PeerRole::Device {
        return Err("invalid_invitation".into());
    }
    let manager =
        botstral_owner_client::read_manager_id(code.endpoint().as_str(), code.pin().fingerprint())
            .await
            .map_err(|_| "trust")?;
    let bundle = OwnerInvitation {
        trust: ManagerTrust {
            manager,
            endpoint: code.endpoint().as_str().to_owned(),
            pin: code.pin().fingerprint().clone(),
        },
        role: DeviceRole::Owner,
        fingerprint,
        expires_at: now() + 600,
        invitation: code.secret_text().to_string(),
    };
    service.command(IntentCommand::Stage(bundle)).await
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
    routines: Vec<RoutineViewModel>,
    routine_occurrences: &'a [RoutineOccurrence],
    inbox: &'a [InboxEntry],
    publishers: &'a [PublisherView],
    routine_budget: &'a Option<RoutineBudget>,
    runs: Vec<RunViewModel<'a>>,
    tasks: Vec<TaskViewModel>,
}
#[derive(Serialize)]
struct RoutineViewModel {
    id: RoutineId,
    bot: BotId,
    revision: u64,
    state: RoutineState,
    name: String,
    next_at: Option<i64>,
    trigger: RoutineTrigger,
    definition_json: String,
}
#[derive(Serialize)]
struct ConfigurationView<'a> {
    bot: BotId,
    model: &'a str,
    ready: bool,
    context_tokens: u32,
    revision: u64,
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
    truncated: bool,
    id: RunTurnId,
    ordinal: u32,
    text: Option<&'a str>,
    task: Option<TaskId>,
    result: Option<&'a str>,
    manager_outcome: &'a Option<ManagerOutcome>,
}
#[derive(Serialize)]
pub struct TaskViewModel {
    id: TaskId,
    bot: BotId,
    state: TaskState,
    progress: u32,
    action: String,
    approval_revoked: bool,
    approval_consumed: bool,
    outputs: Vec<OutputView>,
    evidence: String,
}
impl From<&TaskView> for TaskViewModel {
    fn from(t: &TaskView) -> Self {
        Self {
            id: t.id,
            bot: t.command.bot,
            state: t.state,
            progress: t.progress,
            action: describe_action(&t.command),
            approval_revoked: t.approval.revoked,
            approval_consumed: t.approval.consumed,
            outputs: t.outputs.clone(),
            evidence: serde_json::to_string_pretty(&t.effect_evidence)
                .expect("typed evidence serializes"),
        }
    }
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
                    ComputerOperation::ProtectedLogin { item, revision, purpose, plan } => format!("Protected {purpose:?} login using item {item}, revision {}. Native form POST to {}{}. A separate exact live review is required.", revision.get(), plan.origin, plan.action),
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
        WorkSpec::Connector { spec } => format!(
            "Run connector operation\n{}",
            serde_json::to_string_pretty(&spec.operation)
                .expect("typed connector operation serializes")
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
            routines: w
                .state
                .routine_state
                .routines
                .iter()
                .map(|r| RoutineViewModel {
                    id: r.id,
                    bot: r.definition.bot,
                    revision: r.revision,
                    state: r.state,
                    name: r.definition.name.clone(),
                    next_at: r.next_at,
                    trigger: r.definition.trigger.clone(),
                    definition_json: serde_json::to_string_pretty(&r.definition)
                        .expect("definition serializes"),
                })
                .collect(),
            routine_occurrences: &w.state.routine_state.occurrences,
            inbox: &w.state.routine_state.inbox,
            publishers: &w.state.routine_state.publishers,
            routine_budget: &w.state.routine_state.budget,
            configurations: w
                .bots
                .iter()
                .filter_map(|bot| latest_configuration(w, bot.id))
                .map(|c| ConfigurationView {
                    bot: c.bot,
                    model: &c.policy.model_id,
                    ready: w.run_ready_bots.contains(&c.bot),
                    context_tokens: c.policy.context_tokens,
                    revision: c.revision,
                })
                .collect(),
            runs: w
                .runs
                .iter()
                .map(|r| RunViewModel {
                    id: r.summary.id,
                    bot: r.summary.bot,
                    prompt: r
                        .request
                        .as_ref()
                        .map_or("Conversation content is protected.", |request| {
                            request.prompt.as_str()
                        }),
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
                        RunPhase::WaitingForMemory { .. } => "waiting_for_memory",
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
                            text: t.text.as_deref(),
                            task: t.task,
                            result: t.result.as_deref(),
                            manager_outcome: &t.manager_outcome,
                            truncated: t.truncated,
                        })
                        .collect(),
                })
                .collect(),
            tasks: w.state.tasks.iter().map(TaskViewModel::from).collect(),
        })
        .serialize(serializer)
}

#[cfg(test)]
#[path = "remote/read_tests.rs"]
mod read_tests;

#[cfg(test)]
mod memory_tests {
    use super::*;

    #[test]
    fn task_projection_keeps_exact_actions_and_protected_login_references() {
        let mut command = CommandBody {
            id: CommandId::new(),
            owner: OwnerId::new(),
            task: TaskId::new(),
            bot: BotId::new(),
            worker: WorkerId::new(),
            session: ComputerSessionId::new(),
            operation: OperationId::new(),
            execution: ExecutionId::new(),
            work: WorkSpec::Unavailable,
        };
        let mut spec = ComputerSpec {
            profile: ComputerProfile::LinuxPlaywrightV1,
            account: ProcessAccount {
                uid: 1000,
                gid: 1000,
            },
            runtime: ComputerRuntime {
                sha256: CertificateFingerprint::from_der(b"runtime"),
            },
            deadline: 1,
            limits: ProcessLimits {
                runtime_seconds: 1,
                output_bytes: 1024,
                processes: 64,
                memory_bytes: 512 * 1024 * 1024,
            },
            viewport: Viewport {
                width: 800,
                height: 600,
            },
            outputs: vec![],
            operations: vec![ComputerOperation::Fill {
                selector: "#ordinary-input".into(),
                text: "exact ordinary user text".into(),
            }],
        };
        command.work = WorkSpec::Computer {
            spec: Box::new(spec.clone()),
        };
        assert!(describe_action(&command).contains("exact ordinary user text"));
        let item = CredentialId::new();
        spec.operations = vec![ComputerOperation::ProtectedLogin {
            item,
            revision: CredentialRevision::INITIAL,
            purpose: CredentialFields::Password,
            plan: Box::new(LoginSitePlan {
                version: 1,
                origin: "https://example.test".into(),
                route: "/login".into(),
                frame: None,
                form: "#login".into(),
                username: "#username".into(),
                password: "#password".into(),
                totp: "#totp".into(),
                submitter: "#submit".into(),
                action: "/session".into(),
            }),
        }];
        command.work = WorkSpec::Computer {
            spec: Box::new(spec),
        };
        let action = describe_action(&command);
        assert!(action.contains(&item.to_string()));
        assert!(action.contains("https://example.test"));
        assert!(action.contains("#password"));
        assert!(!action.contains("exact ordinary user text"));
        let task = TaskView {
            id: command.task,
            request: SubmitTask {
                id: command.task,
                session_id: command.session,
                conversation_id: None,
                settings: TaskSettings {
                    model: "projection fixture".into(),
                    max_turns: 1,
                    max_tokens: 64,
                    deadline: 1,
                    max_cost_micros: 0,
                },
                work: command.work.clone(),
            },
            command: command.clone(),
            state: TaskState::AwaitingApproval,
            progress: 0,
            outcome: None,
            effect_evidence: vec![],
            process: None,
            outputs: vec![],
            approval: ApprovalView {
                id: ApprovalId::new(),
                binding: None,
                permission_revision: 1,
                revoked: false,
                consumed: false,
            },
        };
        let projected = serde_json::to_value(TaskViewModel::from(&task)).unwrap();
        assert!(projected.get("command").is_none());
        assert!(projected.get("request").is_none());
        assert!(
            projected["action"]
                .as_str()
                .unwrap()
                .contains(&item.to_string())
        );
        let work = serde_json::to_value(&command.work).unwrap();
        let protected = &work["spec"]["operations"][0];
        assert_eq!(
            protected
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["item", "plan", "purpose", "revision", "type"]
        );
        let mut secret_field = work;
        secret_field["spec"]["operations"][0]["password"] =
            serde_json::json!("synthetic private password");
        assert!(serde_json::from_value::<WorkSpec>(secret_field).is_err());
    }
    #[test]
    fn journal_retains_obsolete_memory_writes_for_uncertainty_only() {
        let root = tempfile::tempdir().unwrap();
        let mut input = WriteDocument {
            bot: BotId::new(),
            kind: DocumentKind::Memory,
            expected_revision: 1,
            sources: vec![],
            markdown: String::new(),
        };
        let save = |input: WriteDocument| {
            storage(
                root.path(),
                "intents.json",
                &vec![Intent {
                    id: Uuid::new_v4(),
                    operation: OwnerOperation::WriteDocument { input },
                    status: "unknown".into(),
                }],
            )
            .unwrap();
            read_journal(&root.path().join("intents.json"))
        };
        assert!(save(input.clone()).is_ok());
        input.kind = DocumentKind::Instructions;
        assert!(save(input).is_err());
    }
}
