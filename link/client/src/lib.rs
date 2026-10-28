use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use botstral_link::*;
use futures_util::{SinkExt, StreamExt};
use rand::RngCore;
use rustls::pki_types::ServerName;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read, Seek, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::net::TcpStream;
use tokio_rustls::{TlsConnector, client::TlsStream};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, protocol::WebSocketConfig},
};
use zeroize::{Zeroize, Zeroizing};

#[derive(Debug)]
pub enum ClientError {
    Network,
    Trust,
    Authentication(OwnerError),
    Refused(OwnerError),
    Protocol,
    Storage,
    UnknownOutcome,
}
impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ClientError {}
type Result<T> = std::result::Result<T, ClientError>;
const DEADLINE: Duration = Duration::from_secs(5);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Credentials {
    credential: String,
    trust: Option<ManagerTrust>,
    invitation: Option<String>,
    device: Option<DeviceId>,
}
impl Drop for Credentials {
    fn drop(&mut self) {
        self.credential.zeroize();
        if let Some(invitation) = &mut self.invitation {
            invitation.zeroize();
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstalledState {
    pub manager: ManagerId,
    pub cursor: u64,
    pub state: OwnerSnapshot,
}

pub struct OwnerClient {
    root: PathBuf,
    credentials: Credentials,
    store: Arc<dyn SecretStore>,
    lock: Arc<fs::File>,
    sessions: Arc<tokio::sync::Semaphore>,
    pub cursor: u64,
}

pub trait SecretStore: Send + Sync {
    fn read(&self) -> Result<Option<Zeroizing<Vec<u8>>>>;
    fn replace(&self, bytes: Zeroizing<Vec<u8>>) -> Result<()>;
    fn delete(&self) -> Result<()>;
}

#[cfg(feature = "probe-files")]
struct ProbeStore(PathBuf);
#[cfg(feature = "probe-files")]
impl SecretStore for ProbeStore {
    fn read(&self) -> Result<Option<Zeroizing<Vec<u8>>>> {
        if !self.0.join("device.json").exists() {
            return Ok(None);
        }
        tls::private_bytes(&self.0.join("device.json"))
            .map(Some)
            .map_err(|_| ClientError::Storage)
    }
    fn replace(&self, bytes: Zeroizing<Vec<u8>>) -> Result<()> {
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| ClientError::Storage)?;
        atomic_write(&self.0, "device.json", &value)
    }
    fn delete(&self) -> Result<()> {
        fs::remove_file(self.0.join("device.json")).map_err(|_| ClientError::Storage)
    }
}

fn private_file(path: &Path) -> io::Result<fs::File> {
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    tls::check_private_file(path)?;
    Ok(file)
}
fn atomic_write(root: &Path, name: &str, value: &impl Serialize) -> Result<()> {
    let staging = root.join(format!(".{name}.part"));
    let result = || -> io::Result<()> {
        let mut file = private_file(&staging)?;
        file.set_len(0)?;
        serde_json::to_writer(&mut file, value)?;
        file.sync_all()?;
        fs::rename(&staging, root.join(name))?;
        fs::File::open(root)?.sync_all()
    };
    let output = result().map_err(|_| ClientError::Storage);
    if output.is_err() {
        let _ = fs::remove_file(staging);
    }
    output
}

impl OwnerClient {
    #[cfg(feature = "probe-files")]
    pub fn initialize(root: &Path, label: String) -> Result<PairingRequest> {
        Self::initialize_with(root, label, Arc::new(ProbeStore(root.to_owned())))
    }
    pub fn initialize_with(
        root: &Path,
        label: String,
        store: Arc<dyn SecretStore>,
    ) -> Result<PairingRequest> {
        fs::DirBuilder::new()
            .mode(0o700)
            .recursive(true)
            .create(root)
            .map_err(|_| ClientError::Storage)?;
        let init_lock =
            private_file(&root.join("client.lock")).map_err(|_| ClientError::Storage)?;
        init_lock.try_lock().map_err(|_| ClientError::Storage)?;
        if let Some(bytes) = store.read()? {
            let saved: Credentials =
                serde_json::from_slice(&bytes).map_err(|_| ClientError::Storage)?;
            return Ok(PairingRequest {
                fingerprint: DeviceFingerprint::from_credential(&saved.credential),
                label,
            });
        }
        let mut bytes = Zeroizing::new([0u8; 32]);
        rand::rngs::OsRng.fill_bytes(bytes.as_mut());
        let credential = URL_SAFE_NO_PAD.encode(&bytes[..]);
        let fingerprint = DeviceFingerprint::from_credential(&credential);
        let credentials = Credentials {
            credential,
            trust: None,
            invitation: None,
            device: None,
        };
        store.replace(Zeroizing::new(
            serde_json::to_vec(&credentials).map_err(|_| ClientError::Storage)?,
        ))?;
        let saved = store.read()?.ok_or(ClientError::Storage)?;
        let saved: Credentials =
            serde_json::from_slice(&saved).map_err(|_| ClientError::Storage)?;
        if saved.credential != credentials.credential {
            return Err(ClientError::Storage);
        }
        Ok(PairingRequest { fingerprint, label })
    }
    #[cfg(feature = "probe-files")]
    pub fn open(root: &Path) -> Result<Self> {
        Self::open_with(root, Arc::new(ProbeStore(root.to_owned())))
    }
    pub fn open_with(root: &Path, store: Arc<dyn SecretStore>) -> Result<Self> {
        let metadata = fs::symlink_metadata(root).map_err(|_| ClientError::Storage)?;
        if !metadata.is_dir()
            || metadata.mode() & 0o7777 != 0o700
            || metadata.uid() != unsafe { libc::geteuid() }
        {
            return Err(ClientError::Storage);
        }
        let root = root.canonicalize().map_err(|_| ClientError::Storage)?;
        let lock = private_file(&root.join("client.lock")).map_err(|_| ClientError::Storage)?;
        lock.try_lock().map_err(|_| ClientError::Storage)?;
        let credentials: Credentials =
            serde_json::from_slice(&store.read()?.ok_or(ClientError::Storage)?)
                .map_err(|_| ClientError::Storage)?;
        for name in [
            ".transfer.part",
            ".state.json.part",
            ".device.json.part",
            ".workspace.json.part",
        ] {
            let path = root.join(name);
            if path.exists() {
                tls::check_private_file(&path).map_err(|_| ClientError::Storage)?;
                fs::remove_file(path).map_err(|_| ClientError::Storage)?;
            }
        }
        let cursor = if root.join("state.json").exists() {
            tls::check_private_file(&root.join("state.json")).map_err(|_| ClientError::Storage)?;
            let file = fs::File::open(root.join("state.json")).map_err(|_| ClientError::Storage)?;
            if file.metadata().map_err(|_| ClientError::Storage)?.len()
                > OWNER_TRANSFER_BYTES as u64
            {
                return Err(ClientError::Storage);
            }
            let state: InstalledState =
                serde_json::from_reader(file.take(OWNER_TRANSFER_BYTES as u64 + 1))
                    .map_err(|_| ClientError::Storage)?;
            if credentials
                .trust
                .as_ref()
                .is_none_or(|trust| trust.manager != state.manager)
            {
                return Err(ClientError::Trust);
            }
            state.cursor
        } else {
            0
        };
        Ok(Self {
            root,
            credentials,
            store,
            lock: Arc::new(lock),
            sessions: Arc::new(tokio::sync::Semaphore::new(1)),
            cursor,
        })
    }
    pub fn import_invitation(&mut self, bundle: OwnerInvitation) -> Result<()> {
        if bundle.role != DeviceRole::Owner
            || bundle.fingerprint
                != DeviceFingerprint::from_credential(&self.credentials.credential)
            || self.credentials.device.is_some()
            || self
                .credentials
                .trust
                .as_ref()
                .is_some_and(|t| t.manager != bundle.trust.manager)
        {
            return Err(ClientError::Trust);
        }
        tls::pinned_manager_config(&bundle.trust).map_err(|_| ClientError::Trust)?;
        self.credentials.trust = Some(bundle.trust);
        self.credentials.invitation = Some(bundle.invitation);
        self.store.replace(Zeroizing::new(
            serde_json::to_vec(&self.credentials).map_err(|_| ClientError::Storage)?,
        ))
    }
    pub fn import_trust(&mut self, trust: ManagerTrust) -> Result<()> {
        if self
            .credentials
            .trust
            .as_ref()
            .is_none_or(|old| old.manager != trust.manager)
        {
            return Err(ClientError::Trust);
        }
        tls::pinned_manager_config(&trust).map_err(|_| ClientError::Trust)?;
        self.credentials.trust = Some(trust);
        self.store.replace(Zeroizing::new(
            serde_json::to_vec(&self.credentials).map_err(|_| ClientError::Storage)?,
        ))
    }
    pub async fn connect(&mut self) -> Result<OwnerSession> {
        let lease = Arc::new(ClientLease {
            _lock: self.lock.clone(),
            _session: self
                .sessions
                .clone()
                .try_acquire_owned()
                .map_err(|_| ClientError::Refused(OwnerError::Busy))?,
        });
        let trust = self.credentials.trust.as_ref().ok_or(ClientError::Trust)?;
        let session = OwnerSession::authenticate(
            trust,
            &self.credentials.credential,
            None,
            self.root.clone(),
            lease.clone(),
        )
        .await;
        let mut session = match session {
            Err(ClientError::Authentication(OwnerError::Unauthorized))
                if self.credentials.device.is_none() && self.credentials.invitation.is_some() =>
            {
                OwnerSession::authenticate(
                    trust,
                    &self.credentials.credential,
                    self.credentials.invitation.as_deref(),
                    self.root.clone(),
                    lease,
                )
                .await?
            }
            result => result?,
        };
        self.credentials.device = Some(session.device);
        if let Some(mut invitation) = self.credentials.invitation.take() {
            invitation.zeroize();
        }
        self.store.replace(Zeroizing::new(
            serde_json::to_vec(&self.credentials).map_err(|_| ClientError::Storage)?,
        ))?;
        session
            .send(OwnerClientMessage::Watch {
                cursor: self.cursor,
            })
            .await?;
        Ok(session)
    }
    pub async fn recover(&mut self, session: &mut OwnerSession) -> Result<()> {
        if !Arc::ptr_eq(&self.lock, &session._lease._lock) {
            return Err(ClientError::Protocol);
        }
        let reply = session
            .call(OwnerOperation::Resume {
                cursor: self.cursor,
                limit: 64,
            })
            .await?;
        let replay = match reply {
            OwnerReply::Replay(replay) => *replay,
            _ => return Err(ClientError::Protocol),
        };
        let (state, cursor) = match replay {
            EventReplay::Snapshot { state, cursor } => (state, cursor),
            EventReplay::Events { entries, cursor } if entries.is_empty() => {
                if cursor != self.cursor {
                    return Err(ClientError::Protocol);
                }
                session.send(OwnerClientMessage::Applied { cursor }).await?;
                return Ok(());
            }
            EventReplay::Events { .. } => {
                match session
                    .call(OwnerOperation::Resume {
                        cursor: 0,
                        limit: 64,
                    })
                    .await?
                {
                    OwnerReply::Replay(replay) => match *replay {
                        EventReplay::Snapshot { state, cursor } => (state, cursor),
                        _ => return Err(ClientError::Protocol),
                    },
                    _ => return Err(ClientError::Protocol),
                }
            }
        };
        let manager = self
            .credentials
            .trust
            .as_ref()
            .ok_or(ClientError::Trust)?
            .manager;
        atomic_write(
            &self.root,
            "state.json",
            &InstalledState {
                manager,
                cursor,
                state,
            },
        )?;
        self.cursor = cursor;
        session.send(OwnerClientMessage::Applied { cursor }).await
    }
    pub fn installed_workspace(&self) -> Result<Option<OwnerWorkspace>> {
        let path = self.root.join("workspace.json");
        if !path.exists() {
            return Ok(None);
        }
        tls::check_private_file(&path).map_err(|_| ClientError::Storage)?;
        let file = fs::File::open(&path).map_err(|_| ClientError::Storage)?;
        if file.metadata().map_err(|_| ClientError::Storage)?.len() > OWNER_TRANSFER_BYTES as u64 {
            return Err(ClientError::Storage);
        }
        let image: OwnerWorkspace =
            serde_json::from_reader(file.take(OWNER_TRANSFER_BYTES as u64 + 1))
                .map_err(|_| ClientError::Storage)?;
        if self.trust().is_none_or(|t| t.manager != image.manager) {
            return Err(ClientError::Trust);
        }
        Ok(Some(image))
    }
    pub fn trust(&self) -> Option<&ManagerTrust> {
        self.credentials.trust.as_ref()
    }
    pub fn device(&self) -> Option<DeviceId> {
        self.credentials.device
    }
    pub async fn recover_workspace(
        &mut self,
        session: &mut OwnerSession,
    ) -> Result<OwnerWorkspace> {
        if !Arc::ptr_eq(&self.lock, &session._lease._lock) {
            return Err(ClientError::Protocol);
        }
        let OwnerReply::Workspace(image) = session.call(OwnerOperation::Workspace).await? else {
            return Err(ClientError::Protocol);
        };
        if self.trust().is_none_or(|t| t.manager != image.manager) || image.cursor < self.cursor {
            return Err(ClientError::Trust);
        }
        atomic_write(&self.root, "workspace.json", &image)?;
        self.cursor = image.cursor;
        session
            .send(OwnerClientMessage::Applied {
                cursor: image.cursor,
            })
            .await?;
        Ok(*image)
    }
    pub async fn watch(
        &mut self,
        duration: Duration,
        mut notification: impl FnMut(u64),
    ) -> Result<()> {
        let until = tokio::time::Instant::now() + duration;
        let mut attempt = 0u32;
        while tokio::time::Instant::now() < until {
            let work = async {
                let mut session = self.connect().await?;
                self.recover(&mut session).await?;
                attempt = 0;
                loop {
                    let head = if let Some(head) = session.notification.take() {
                        head
                    } else {
                        match session.next().await? {
                            OwnerServerMessage::Notification { head } => head,
                            OwnerServerMessage::Pong => continue,
                            _ => return Err(ClientError::Protocol),
                        }
                    };
                    notification(head);
                    self.recover(&mut session).await?;
                }
            };
            match tokio::time::timeout_at(until, work).await {
                Err(_) => return Ok(()),
                Ok(Err(
                    ClientError::Network
                    | ClientError::Refused(OwnerError::Busy)
                    | ClientError::Authentication(OwnerError::Busy | OwnerError::RateLimited),
                )) => {}
                Ok(Err(error)) => return Err(error),
                Ok(Ok(())) => return Ok(()),
            }
            let base = 250u64.saturating_mul(1u64 << attempt.min(7)).min(30_000);
            let jitter = rand::random::<u64>() % (base / 2 + 1);
            attempt = attempt.saturating_add(1);
            tokio::time::sleep_until(
                (tokio::time::Instant::now() + Duration::from_millis(base / 2 + jitter)).min(until),
            )
            .await;
        }
        Ok(())
    }
}

type Socket = WebSocketStream<TlsStream<TcpStream>>;
struct ClientLease {
    _lock: Arc<fs::File>,
    _session: tokio::sync::OwnedSemaphorePermit,
}
pub struct OwnerSession {
    socket: Socket,
    root: PathBuf,
    device: DeviceId,
    heartbeat: tokio::time::Instant,
    last_received: tokio::time::Instant,
    notification: Option<u64>,
    _lease: Arc<ClientLease>,
}
struct Staging {
    path: PathBuf,
    file: fs::File,
}
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

impl OwnerSession {
    async fn authenticate(
        trust: &ManagerTrust,
        credential: &str,
        invitation: Option<&str>,
        root: PathBuf,
        lease: Arc<ClientLease>,
    ) -> Result<Self> {
        let tls = tls::pinned_manager_config(trust).map_err(|_| ClientError::Trust)?;
        let name =
            ServerName::try_from(trust.server_name.clone()).map_err(|_| ClientError::Trust)?;
        let socket = tokio::time::timeout(DEADLINE, async {
            let tcp = TcpStream::connect(&trust.endpoint)
                .await
                .map_err(|_| ClientError::Network)?;
            let tls = TlsConnector::from(Arc::new(tls))
                .connect(name, tcp)
                .await
                .map_err(|error| {
                    if matches!(
                        error
                            .get_ref()
                            .and_then(|e| e.downcast_ref::<rustls::Error>()),
                        Some(
                            rustls::Error::InvalidCertificate(_)
                                | rustls::Error::NoCertificatesPresented
                        )
                    ) {
                        ClientError::Trust
                    } else {
                        ClientError::Network
                    }
                })?;
            let url = format!("wss://{}/owner/v1/connect", trust.endpoint);
            let limits = WebSocketConfig::default()
                .max_message_size(Some(MAX_MESSAGE))
                .max_frame_size(Some(MAX_MESSAGE));
            tokio_tungstenite::client_async_with_config(url, tls, Some(limits))
                .await
                .map(|(socket, _)| socket)
                .map_err(|_| ClientError::Network)
        })
        .await
        .map_err(|_| ClientError::Network)??;
        let now = tokio::time::Instant::now();
        let mut session = Self {
            socket,
            root,
            device: DeviceId::new(),
            heartbeat: now + Duration::from_secs(10),
            last_received: now,
            notification: None,
            _lease: lease,
        };
        let hello = match invitation {
            Some(invitation) => OwnerHello::Enroll {
                version: OWNER_VERSION,
                manager: trust.manager,
                role: DeviceRole::Owner,
                credential: credential.into(),
                invitation: invitation.into(),
            },
            None => OwnerHello::Authenticate {
                version: OWNER_VERSION,
                manager: trust.manager,
                role: DeviceRole::Owner,
                credential: credential.into(),
            },
        };
        let text =
            Zeroizing::new(serde_json::to_string(&hello).map_err(|_| ClientError::Protocol)?);
        tokio::time::timeout(
            DEADLINE,
            session.socket.send(Message::Text(text.to_string().into())),
        )
        .await
        .map_err(|_| ClientError::Network)?
        .map_err(|_| ClientError::Network)?;
        match tokio::time::timeout(DEADLINE, session.next())
            .await
            .map_err(|_| ClientError::Network)??
        {
            OwnerServerMessage::Welcome {
                version,
                manager,
                device,
            } if version == OWNER_VERSION && manager == trust.manager => {
                session.device = device;
                Ok(session)
            }
            OwnerServerMessage::Error { code, .. } => Err(ClientError::Authentication(code)),
            _ => Err(ClientError::Protocol),
        }
    }
    pub async fn next_signal(&mut self) -> Result<u64> {
        if let Some(head) = self.notification.take() {
            return Ok(head);
        }
        loop {
            match self.next().await? {
                OwnerServerMessage::Notification { head } => return Ok(head),
                OwnerServerMessage::Pong => {}
                _ => return Err(ClientError::Protocol),
            }
        }
    }
    async fn send(&mut self, message: OwnerClientMessage) -> Result<()> {
        let bytes = serde_json::to_string(&message).map_err(|_| ClientError::Protocol)?;
        if bytes.len() >= MAX_MESSAGE {
            return Err(ClientError::Protocol);
        }
        tokio::time::timeout(DEADLINE, self.socket.send(Message::Text(bytes.into())))
            .await
            .map_err(|_| ClientError::Network)?
            .map_err(|_| ClientError::Network)
    }
    async fn next(&mut self) -> Result<OwnerServerMessage> {
        loop {
            let now = tokio::time::Instant::now();
            if now.duration_since(self.last_received) >= Duration::from_secs(30) {
                return Err(ClientError::Network);
            }
            tokio::select! {
                biased;
                _=tokio::time::sleep_until(self.heartbeat)=>{
                    self.send(OwnerClientMessage::Ping).await?;
                    self.heartbeat=tokio::time::Instant::now()+Duration::from_secs(10);
                }
                message=self.socket.next()=>{
                    self.last_received=tokio::time::Instant::now();
                    return match message {
                        Some(Ok(Message::Text(text)))=>serde_json::from_str(&text).map_err(|_|ClientError::Protocol),
                        _=>Err(ClientError::Network),
                    };
                }
            }
        }
    }
    pub async fn call(&mut self, operation: OwnerOperation) -> Result<OwnerReply> {
        let read = matches!(
            &operation,
            OwnerOperation::Workspace
                | OwnerOperation::Resume { .. }
                | OwnerOperation::Task { .. }
                | OwnerOperation::Run { .. }
                | OwnerOperation::Inference { .. }
                | OwnerOperation::InferenceConfiguration { .. }
                | OwnerOperation::ComputerSession { .. }
                | OwnerOperation::SearchDocuments { .. }
                | OwnerOperation::RebuildDocuments
                | OwnerOperation::Receipt { .. }
                | OwnerOperation::Artifact { .. }
                | OwnerOperation::CodingCalls { .. }
        );
        let id = RequestId::new();
        let sent = self
            .send(OwnerClientMessage::Call {
                id,
                operation: Box::new(operation),
            })
            .await;
        match sent {
            Err(ClientError::Network) if !read => return Err(ClientError::UnknownOutcome),
            Err(error) => return Err(error),
            Ok(()) => {}
        }
        match tokio::time::timeout(
            Duration::from_secs(OWNER_TRANSFER_SECONDS),
            self.receive_reply(id),
        )
        .await
        {
            Err(_) => Err(if read {
                ClientError::Network
            } else {
                ClientError::UnknownOutcome
            }),
            Ok(Err(error)) if !read && !matches!(error, ClientError::Refused(_)) => {
                Err(ClientError::UnknownOutcome)
            }
            Ok(result) => result,
        }
    }
    async fn receive_reply(&mut self, request: RequestId) -> Result<OwnerReply> {
        let (id, length, count, expected) = loop {
            match self.next().await? {
                OwnerServerMessage::Pong => {}
                OwnerServerMessage::Notification { head } => self.notification = Some(head),
                OwnerServerMessage::Error { id: Some(id), code } if id == request => {
                    return Err(ClientError::Refused(code));
                }
                OwnerServerMessage::TransferStart {
                    id,
                    request: Some(r),
                    bytes,
                    chunks,
                    digest,
                } if r == request => break (id, bytes, chunks, digest),
                _ => return Err(ClientError::Protocol),
            }
        };
        if length == 0
            || length > OWNER_TRANSFER_BYTES as u64
            || count != length.div_ceil(OWNER_CHUNK_BYTES as u64) as u32
            || expected.len() != 64
        {
            return Err(ClientError::Protocol);
        }
        let path = self.root.join(".transfer.part");
        let file = private_file(&path).map_err(|_| ClientError::Storage)?;
        file.set_len(0).map_err(|_| ClientError::Storage)?;
        let mut stage = Staging { path, file };
        let mut index = 0;
        let mut received = 0;
        let mut digest = Sha256::new();
        loop {
            match self.next().await? {
                OwnerServerMessage::Pong => {}
                OwnerServerMessage::Notification { head } => self.notification = Some(head),
                OwnerServerMessage::TransferChunk {
                    id: chunk,
                    index: n,
                    data,
                } if chunk == id && n == index && index < count => {
                    let bytes = STANDARD.decode(data).map_err(|_| ClientError::Protocol)?;
                    if bytes.len() > OWNER_CHUNK_BYTES
                        || bytes.is_empty()
                        || received + bytes.len() as u64 > length
                        || (index + 1 < count && bytes.len() != OWNER_CHUNK_BYTES)
                    {
                        return Err(ClientError::Protocol);
                    }
                    stage
                        .file
                        .write_all(&bytes)
                        .map_err(|_| ClientError::Storage)?;
                    digest.update(&bytes);
                    received += bytes.len() as u64;
                    index += 1;
                }
                OwnerServerMessage::TransferComplete { id: complete }
                    if complete == id && index == count && received == length =>
                {
                    break;
                }
                _ => return Err(ClientError::Protocol),
            }
        }
        if format!("{:x}", digest.finalize()) != expected {
            return Err(ClientError::Protocol);
        }
        stage.file.sync_all().map_err(|_| ClientError::Storage)?;
        stage.file.rewind().map_err(|_| ClientError::Storage)?;
        serde_json::from_reader(&mut stage.file).map_err(|_| ClientError::Protocol)
    }
}
