use reqwest::{Client, Method, StatusCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;
use zeroize::Zeroizing;

const DEFAULT_ENDPOINT: &str = "127.0.0.1:43120";
const MAX_RESPONSE: usize = 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(4);
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(15);
const QUEUE_TIMEOUT: Duration = Duration::from_secs(24);

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Offline,
    Timeout,
    Unauthorized,
    InvalidCredentials,
    InvalidInput,
    Busy,
    RateLimited,
    Conflict,
    Protocol,
    ManagerError,
    Stale,
    NativeStorage,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Connection {
    Unknown,
    Online,
    Offline,
    Error,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SignOutReason {
    Initial,
    User,
    InvalidSession,
    SignInFailed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Owner {
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Bot {
    pub id: Uuid,
    pub name: String,
    pub created_at: i64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Vault {
    Locked,
    Unlocked,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    pub provider: ProviderName,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderName {
    Mistral,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkerPhase {
    Disconnected,
    Synchronizing,
    Ready,
}

#[derive(Clone, Debug, Serialize)]
pub struct Worker {
    pub id: Uuid,
    pub revoked: bool,
    pub phase: WorkerPhase,
}

#[derive(Clone, Debug, Serialize)]
pub struct Details {
    pub vault: Vault,
    pub provider: Option<Provider>,
    pub workers: Vec<Worker>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionView {
    SignedOut {
        reason: SignOutReason,
    },
    SignedIn {
        owner: Owner,
        bots: Vec<Bot>,
        expires_at: i64,
        details: Details,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub revision: u64,
    pub generation: u64,
    pub endpoint: String,
    pub connection: Connection,
    #[serde(flatten)]
    pub session: SessionView,
}

#[derive(Clone, Debug, Serialize)]
pub struct Failure {
    pub code: ErrorCode,
    pub snapshot: Box<Snapshot>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionMetadata {
    #[serde(rename = "id")]
    _id: Uuid,
    created_at: i64,
    expires_at: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Grant {
    token: Zeroizing<String>,
    session: SessionMetadata,
}
#[derive(Clone)]
struct Auth {
    token: Zeroizing<String>,
    expires_at: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Status {
    vault: Vault,
    provider: Option<Provider>,
    #[serde(rename = "worker")]
    _worker: LegacyWorker,
    #[serde(rename = "inference")]
    _inference: Inference,
    #[serde(rename = "remote")]
    _remote: Remote,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum LegacyWorker {
    NotImplemented,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Inference {
    NotImplemented,
    SingleTurnText,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Remote {
    Disabled,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerWire {
    id: Uuid,
    fingerprint: String,
    revoked: bool,
    generation: u64,
    phase: WorkerPhase,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Health {
    #[serde(rename = "status")]
    _status: HealthStatus,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum HealthStatus {
    Ok,
}

struct State {
    endpoint: SocketAddr,
    auth: Option<Auth>,
    snapshot: Snapshot,
}

#[derive(Clone)]
pub struct LocalManager {
    state: Arc<Mutex<State>>,
    admission: Arc<Semaphore>,
    gate: Arc<Semaphore>,
    client: Client,
}

struct Operation {
    generation: u64,
    endpoint: SocketAddr,
    auth: Option<Auth>,
    _admission: OwnedSemaphorePermit,
    _gate: OwnedSemaphorePermit,
}

enum Route {
    Health,
    SignIn,
    SignOut,
    Owner,
    Bots,
    Bot(Uuid),
    Status,
    Workers,
    InviteDevice,
    Devices,
    RevokeDevice(botstral_link::DeviceId),
    Update,
    ApplyUpdate,
}
impl Route {
    fn parts(&self) -> (Method, String, StatusCode) {
        match self {
            Self::InviteDevice => (
                Method::POST,
                "/v1/device-invitations".into(),
                StatusCode::CREATED,
            ),
            Self::Devices => (Method::GET, "/v1/devices".into(), StatusCode::OK),
            Self::RevokeDevice(id) => (
                Method::DELETE,
                format!("/v1/devices/{id}"),
                StatusCode::NO_CONTENT,
            ),
            Self::Health => (Method::GET, "/health".into(), StatusCode::OK),
            Self::SignIn => (Method::POST, "/v1/sessions".into(), StatusCode::CREATED),
            Self::SignOut => (Method::DELETE, "/v1/session".into(), StatusCode::NO_CONTENT),
            Self::Owner => (Method::GET, "/v1/owner".into(), StatusCode::OK),
            Self::Bots => (Method::GET, "/v1/bots".into(), StatusCode::OK),
            Self::Bot(id) => (Method::PUT, format!("/v1/bots/{id}"), StatusCode::OK),
            Self::Status => (Method::GET, "/v1/status".into(), StatusCode::OK),
            Self::Workers => (Method::GET, "/v1/workers".into(), StatusCode::OK),
            Self::Update => (Method::GET, "/v1/update".into(), StatusCode::OK),
            Self::ApplyUpdate => (Method::POST, "/v1/update".into(), StatusCode::OK),
        }
    }
}

impl LocalManager {
    pub fn new() -> Result<Self, ErrorCode> {
        let endpoint = DEFAULT_ENDPOINT
            .parse()
            .map_err(|_| ErrorCode::InvalidInput)?;
        Ok(Self {
            state: Arc::new(Mutex::new(State {
                endpoint,
                auth: None,
                snapshot: Snapshot {
                    revision: 0,
                    generation: 0,
                    endpoint: DEFAULT_ENDPOINT.into(),
                    connection: Connection::Unknown,
                    session: SessionView::SignedOut {
                        reason: SignOutReason::Initial,
                    },
                },
            })),
            admission: Arc::new(Semaphore::new(5)),
            gate: Arc::new(Semaphore::new(1)),
            client: Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(1))
                .timeout(REQUEST_TIMEOUT)
                .pool_max_idle_per_host(0)
                .build()
                .map_err(|_| ErrorCode::Protocol)?,
        })
    }

    pub async fn invite_device(&self, label: String) -> Result<String, Failure> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Grant {
            code: String,
            role: botstral_link::DeviceRole,
            #[serde(rename = "manager")]
            _manager: botstral_link::ManagerId,
            #[serde(rename = "expires_at")]
            _expires_at: i64,
            #[serde(rename = "invitation")]
            _invitation: String,
        }
        let operation = self.begin().await?;
        let token = operation
            .auth
            .as_ref()
            .ok_or_else(|| self.failure(ErrorCode::Unauthorized))?;
        let bytes = self
            .request(
                operation.endpoint,
                Route::InviteDevice,
                Some(&token.token),
                Some(serde_json::json!({"label":label,"lifetime_seconds":600})),
            )
            .await
            .map_err(|code| self.failed(operation.generation, code))?;
        let grant: Grant =
            serde_json::from_slice(&bytes).map_err(|_| self.failure(ErrorCode::Protocol))?;
        if grant.role != botstral_link::DeviceRole::Owner
            || botstral_link::JoinCode::parse(&grant.code).is_err()
        {
            return Err(self.failure(ErrorCode::Protocol));
        }
        Ok(grant.code)
    }
    pub async fn devices(&self) -> Result<Vec<botstral_link::OwnerDevice>, Failure> {
        let operation = self.begin().await?;
        let auth = operation
            .auth
            .as_ref()
            .ok_or_else(|| self.failure(ErrorCode::Unauthorized))?;
        self.get(operation.endpoint, Route::Devices, Some(&auth.token))
            .await
            .map_err(|code| self.failed(operation.generation, code))
    }
    pub async fn revoke_device(&self, id: botstral_link::DeviceId) -> Result<(), Failure> {
        let operation = self.begin().await?;
        let auth = operation
            .auth
            .as_ref()
            .ok_or_else(|| self.failure(ErrorCode::Unauthorized))?;
        self.request(
            operation.endpoint,
            Route::RevokeDevice(id),
            Some(&auth.token),
            Some(serde_json::json!({})),
        )
        .await
        .map_err(|code| self.failed(operation.generation, code))?;
        Ok(())
    }
    pub async fn update_report(&self) -> Result<botstral_link::UpdateReport, Failure> {
        let operation = self.begin().await?;
        let auth = operation
            .auth
            .as_ref()
            .ok_or_else(|| self.failure(ErrorCode::Unauthorized))?;
        self.get(operation.endpoint, Route::Update, Some(&auth.token))
            .await
            .map_err(|code| self.failed(operation.generation, code))
    }

    pub async fn apply_update(&self) -> Result<botstral_link::UpdateReport, Failure> {
        let operation = self.begin().await?;
        let auth = operation
            .auth
            .as_ref()
            .ok_or_else(|| self.failure(ErrorCode::Unauthorized))?;
        let bytes = self
            .request(
                operation.endpoint,
                Route::ApplyUpdate,
                Some(&auth.token),
                Some(serde_json::json!({})),
            )
            .await
            .map_err(|code| self.failed(operation.generation, code))?;
        serde_json::from_slice(&bytes).map_err(|_| self.failure(ErrorCode::Protocol))
    }

    pub fn inspect(&self) -> Snapshot {
        self.state.lock().unwrap().snapshot.clone()
    }

    fn failure(&self, code: ErrorCode) -> Failure {
        Failure {
            code,
            snapshot: Box::new(self.inspect()),
        }
    }

    fn admit(&self) -> Result<OwnedSemaphorePermit, Failure> {
        self.admission
            .clone()
            .try_acquire_owned()
            .map_err(|_| self.failure(ErrorCode::Busy))
    }

    async fn enter(
        &self,
        admission: OwnedSemaphorePermit,
        generation: u64,
    ) -> Result<Operation, Failure> {
        let gate = tokio::time::timeout(QUEUE_TIMEOUT, self.gate.clone().acquire_owned())
            .await
            .map_err(|_| self.failure(ErrorCode::Busy))?
            .map_err(|_| self.failure(ErrorCode::Busy))?;
        let state = self.state.lock().unwrap();
        if state.snapshot.generation != generation {
            return Err(Failure {
                code: ErrorCode::Stale,
                snapshot: Box::new(state.snapshot.clone()),
            });
        }
        Ok(Operation {
            generation,
            endpoint: state.endpoint,
            auth: state.auth.clone(),
            _admission: admission,
            _gate: gate,
        })
    }

    async fn begin(&self) -> Result<Operation, Failure> {
        let admission = self.admit()?;
        let generation = self.state.lock().unwrap().snapshot.generation;
        self.enter(admission, generation).await
    }

    fn publish(&self, generation: u64, session: SessionView) -> Result<Snapshot, Failure> {
        let mut state = self.state.lock().unwrap();
        if state.snapshot.generation != generation {
            return Err(Failure {
                code: ErrorCode::Stale,
                snapshot: Box::new(state.snapshot.clone()),
            });
        }
        state.snapshot.revision += 1;
        state.snapshot.connection = Connection::Online;
        state.snapshot.session = session;
        Ok(state.snapshot.clone())
    }

    fn failed(&self, generation: u64, code: ErrorCode) -> Failure {
        let mut state = self.state.lock().unwrap();
        if state.snapshot.generation != generation {
            return Failure {
                code: ErrorCode::Stale,
                snapshot: Box::new(state.snapshot.clone()),
            };
        }
        state.snapshot.revision += 1;
        state.snapshot.connection = match code {
            ErrorCode::Offline | ErrorCode::Timeout => Connection::Offline,
            ErrorCode::Unauthorized
            | ErrorCode::InvalidCredentials
            | ErrorCode::Conflict
            | ErrorCode::RateLimited
            | ErrorCode::Busy
            | ErrorCode::InvalidInput => Connection::Online,
            _ => Connection::Error,
        };
        if code == ErrorCode::Unauthorized {
            state.auth = None;
            state.snapshot.generation += 1;
            state.snapshot.session = SessionView::SignedOut {
                reason: SignOutReason::InvalidSession,
            };
        }
        Failure {
            code,
            snapshot: Box::new(state.snapshot.clone()),
        }
    }

    async fn request(
        &self,
        endpoint: SocketAddr,
        route: Route,
        token: Option<&str>,
        body: Option<serde_json::Value>,
    ) -> Result<Vec<u8>, ErrorCode> {
        let (method, path, expected) = route.parts();
        let mut request = self
            .client
            .request(method, format!("http://{endpoint}{path}"))
            .timeout(match route {
                Route::SignIn => SIGN_IN_TIMEOUT,
                _ => REQUEST_TIMEOUT,
            });
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let mut response = request.send().await.map_err(transport_error)?;
        if response.status() != expected {
            return Err(match response.status().as_u16() {
                400 => ErrorCode::InvalidInput,
                401 => ErrorCode::Unauthorized,
                409 => ErrorCode::Conflict,
                429 => ErrorCode::RateLimited,
                503 => ErrorCode::Busy,
                408 => ErrorCode::Timeout,
                _ => ErrorCode::ManagerError,
            });
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_RESPONSE as u64)
        {
            return Err(ErrorCode::Protocol);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
            if bytes.len() + chunk.len() > MAX_RESPONSE {
                return Err(ErrorCode::Protocol);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }

    async fn get<T: DeserializeOwned>(
        &self,
        endpoint: SocketAddr,
        route: Route,
        token: Option<&str>,
    ) -> Result<T, ErrorCode> {
        serde_json::from_slice(&self.request(endpoint, route, token, None).await?)
            .map_err(|_| ErrorCode::Protocol)
    }

    async fn revoke(&self, endpoint: SocketAddr, token: &str) {
        let _ = self
            .request(
                endpoint,
                Route::SignOut,
                Some(token),
                Some(serde_json::json!({})),
            )
            .await;
    }

    async fn session_view(
        &self,
        endpoint: SocketAddr,
        auth: &Auth,
    ) -> Result<SessionView, ErrorCode> {
        let token = Some(auth.token.as_str());
        let owner: Owner = self.get(endpoint, Route::Owner, token).await?;
        let bots: Vec<Bot> = self.get(endpoint, Route::Bots, token).await?;
        let status: Status = self.get(endpoint, Route::Status, token).await?;
        let workers: Vec<WorkerWire> = self.get(endpoint, Route::Workers, token).await?;
        if !valid_name(&owner.name) || bots.iter().any(|bot| !valid_name(&bot.name)) {
            return Err(ErrorCode::Protocol);
        }
        if workers.iter().any(|worker| {
            worker.fingerprint.len() != 64
                || !worker.fingerprint.bytes().all(|b| b.is_ascii_hexdigit())
                || worker.generation > (1 << 53) - 1
        }) {
            return Err(ErrorCode::Protocol);
        }
        let Status {
            vault, provider, ..
        } = status;
        Ok(SessionView::SignedIn {
            owner,
            bots,
            expires_at: auth.expires_at,
            details: Details {
                vault,
                provider,
                workers: workers
                    .into_iter()
                    .map(|worker| Worker {
                        id: worker.id,
                        revoked: worker.revoked,
                        phase: worker.phase,
                    })
                    .collect(),
            },
        })
    }

    pub async fn sign_in(
        &self,
        endpoint: String,
        password: Zeroizing<String>,
    ) -> Result<Snapshot, Failure> {
        let endpoint = parse_endpoint(&endpoint).map_err(|code| self.failure(code))?;
        if !(16..=1024).contains(&password.len()) {
            return Err(self.failure(ErrorCode::InvalidInput));
        }
        let admission = self.admit()?;
        let generation = {
            let mut state = self.state.lock().unwrap();
            if state.auth.is_some() {
                return Err(Failure {
                    code: ErrorCode::Conflict,
                    snapshot: Box::new(state.snapshot.clone()),
                });
            }
            state.endpoint = endpoint;
            state.snapshot.generation += 1;
            state.snapshot.revision += 1;
            state.snapshot.endpoint = endpoint.to_string();
            state.snapshot.connection = Connection::Unknown;
            state.snapshot.session = SessionView::SignedOut {
                reason: SignOutReason::SignInFailed,
            };
            state.snapshot.generation
        };
        let operation = self.enter(admission, generation).await?;
        let grant = self
            .request(
                endpoint,
                Route::SignIn,
                None,
                Some(serde_json::json!({"password": password.as_str()})),
            )
            .await;
        drop(password);
        let grant: Grant = match grant
            .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|_| ErrorCode::Protocol))
        {
            Ok(grant) => grant,
            Err(ErrorCode::Unauthorized) => {
                return Err(self.failed(generation, ErrorCode::InvalidCredentials));
            }
            Err(code) => return Err(self.failed(generation, code)),
        };
        let SessionMetadata {
            created_at,
            expires_at,
            ..
        } = grant.session;
        if grant.token.len() != 43
            || !grant
                .token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            || expires_at <= created_at
        {
            return Err(self.failed(generation, ErrorCode::Protocol));
        }
        let auth = Auth {
            token: grant.token,
            expires_at,
        };
        if self.inspect().generation != generation {
            self.revoke(endpoint, &auth.token).await;
            return Err(self.failure(ErrorCode::Stale));
        }
        let session = match self.session_view(endpoint, &auth).await {
            Ok(session) => session,
            Err(code) => {
                self.revoke(endpoint, &auth.token).await;
                return Err(self.failed(generation, code));
            }
        };
        let installed = {
            let mut state = self.state.lock().unwrap();
            if state.snapshot.generation == generation {
                state.auth = Some(auth.clone());
                true
            } else {
                false
            }
        };
        if !installed {
            self.revoke(endpoint, &auth.token).await;
            return Err(self.failure(ErrorCode::Stale));
        }
        let result = self.publish(generation, session);
        drop(operation);
        result
    }

    pub async fn refresh(&self) -> Result<Snapshot, Failure> {
        let operation = self.begin().await?;
        let result = match &operation.auth {
            Some(auth) => self.session_view(operation.endpoint, auth).await,
            None => self
                .get::<Health>(operation.endpoint, Route::Health, None)
                .await
                .map(|_| self.inspect().session),
        };
        match result {
            Ok(session) => self.publish(operation.generation, session),
            Err(code) => Err(self.failed(operation.generation, code)),
        }
    }

    pub async fn create_bot(&self, id: Uuid, name: String) -> Result<Snapshot, Failure> {
        if !valid_name(&name) {
            return Err(self.failure(ErrorCode::InvalidInput));
        }
        let operation = self.begin().await?;
        let Some(auth) = &operation.auth else {
            return Err(self.failed(operation.generation, ErrorCode::Unauthorized));
        };
        let result = self
            .request(
                operation.endpoint,
                Route::Bot(id),
                Some(&auth.token),
                Some(serde_json::json!({"name": name})),
            )
            .await;
        let result = match result {
            Ok(bytes) => match serde_json::from_slice::<Bot>(&bytes) {
                Ok(bot) if bot.id == id && bot.name == name => {
                    self.session_view(operation.endpoint, auth).await
                }
                _ => Err(ErrorCode::Protocol),
            },
            Err(code) => Err(code),
        };
        match result {
            Ok(session) => self.publish(operation.generation, session),
            Err(code) => Err(self.failed(operation.generation, code)),
        }
    }

    pub fn sign_out(&self) -> Snapshot {
        let (snapshot, auth, endpoint) = {
            let mut state = self.state.lock().unwrap();
            let auth = state.auth.take();
            state.snapshot.generation += 1;
            state.snapshot.revision += 1;
            state.snapshot.session = SessionView::SignedOut {
                reason: SignOutReason::User,
            };
            (state.snapshot.clone(), auth, state.endpoint)
        };
        if let Some(auth) = auth {
            let service = self.clone();
            if let Ok(admission) = self.admission.clone().try_acquire_owned() {
                tokio::spawn(async move {
                    let _admission = admission;
                    if let Ok(Ok(_gate)) =
                        tokio::time::timeout(QUEUE_TIMEOUT, service.gate.clone().acquire_owned())
                            .await
                    {
                        service.revoke(endpoint, &auth.token).await;
                    }
                });
            }
        }
        snapshot
    }
}

fn transport_error(error: reqwest::Error) -> ErrorCode {
    if error.is_timeout() {
        ErrorCode::Timeout
    } else {
        ErrorCode::Offline
    }
}

fn parse_endpoint(input: &str) -> Result<SocketAddr, ErrorCode> {
    input
        .parse::<SocketAddr>()
        .ok()
        .filter(|address| address.ip().is_loopback() && address.port() != 0)
        .ok_or(ErrorCode::InvalidInput)
}

fn valid_name(name: &str) -> bool {
    (1..=100).contains(&name.chars().count()) && !name.chars().any(char::is_control)
}
