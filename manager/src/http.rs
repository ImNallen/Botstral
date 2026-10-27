mod inference;
mod runs;
mod tasks;
mod workers;

use crate::{LocalState, manager::*};
use age::secrecy::SecretString;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, FromRequest, FromRequestParts, Path, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use hyper::server::conn::http1;
use hyper_util::{
    rt::{TokioIo, TokioTimer},
    service::TowerToHyperService,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{io, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    net::TcpListener,
    sync::{Semaphore, watch},
    task::JoinSet,
};

pub async fn serve(
    local: LocalState,
    listen: SocketAddr,
    setup_listen: SocketAddr,
) -> io::Result<()> {
    serve_with_worker(local, listen, setup_listen, None).await
}

pub async fn serve_with_worker(
    local: LocalState,
    listen: SocketAddr,
    setup_listen: SocketAddr,
    worker_tls: Option<crate::WorkerTls>,
) -> io::Result<()> {
    if !listen.ip().is_loopback() || !setup_listen.ip().is_loopback() {
        return Err(io::Error::other("unsafe bind"));
    }
    local.remove("listeners.json")?;
    let directory = local.directory().to_owned();
    let runtime = tokio::task::spawn_blocking(move || Manager::start(directory))
        .await
        .map_err(io::Error::other)?
        .map_err(|_| io::Error::other("state unavailable"))?;
    let crate::manager::Runtime {
        manager,
        setup,
        thread,
        inference,
    } = runtime;
    let result = listeners(&local, manager, setup, listen, setup_listen, worker_tls).await;
    let inference_result = inference
        .shutdown()
        .await
        .map_err(|_| io::Error::other("inference stopped"));
    let joined = tokio::task::spawn_blocking(move || {
        thread.join().map_err(|_| io::Error::other("state stopped"))
    })
    .await
    .map_err(io::Error::other)?;
    local.remove("listeners.json")?;
    result.and(inference_result).and(joined)
}

#[derive(Serialize)]
struct Listeners {
    listen: SocketAddr,
    setup: Option<SocketAddr>,
    worker: Option<SocketAddr>,
}

async fn listeners(
    local: &LocalState,
    manager: Manager,
    setup: Option<LocalSetup>,
    listen: SocketAddr,
    setup_listen: SocketAddr,
    worker_tls: Option<crate::WorkerTls>,
) -> io::Result<()> {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let regular_listener = TcpListener::bind(listen).await?;
    let setup_listener = if setup.is_some() {
        Some(TcpListener::bind(setup_listen).await?)
    } else {
        None
    };
    let worker_listener = match worker_tls {
        Some(config) => Some(crate::worker_http::WorkerListener::bind(config).await?),
        None => None,
    };
    let mut addresses = Listeners {
        worker: worker_listener
            .as_ref()
            .map(crate::worker_http::WorkerListener::local_addr)
            .transpose()?,
        listen: regular_listener.local_addr()?,
        setup: setup_listener
            .as_ref()
            .map(TcpListener::local_addr)
            .transpose()?,
    };
    local.write("listeners.json", &serde_json::to_vec(&addresses)?)?;
    let (stop, stopped) = watch::channel(false);
    let mut tasks = JoinSet::new();
    if let Some(listener) = worker_listener {
        tasks.spawn(listener.serve(manager.clone(), stopped.clone()));
    }
    tasks.spawn(serve_listener(
        regular_listener,
        regular_router(manager),
        stopped.clone(),
        None,
    ));
    if let (Some(listener), Some(setup)) = (setup_listener, setup) {
        let configured = setup.configured.clone();
        tasks.spawn(serve_listener(
            listener,
            setup_router(setup),
            stopped,
            Some(configured),
        ));
    }
    println!("manager ready");
    let mut result = Ok(());
    loop {
        tokio::select! {
            _ = terminate.recv() => break,
            _ = interrupt.recv() => break,
            task = tasks.join_next() => {
                match task {
                    Some(Ok(Ok(()))) if !tasks.is_empty() => {
                        addresses.setup = None;
                        local.write("listeners.json", &serde_json::to_vec(&addresses)?)?;
                    },
                    Some(Ok(Err(error))) => { result = Err(error); break; },
                    Some(Err(error)) => { result = Err(io::Error::other(error)); break; },
                    _ => break,
                }
            }
        }
    }
    stop.send_replace(true);
    while let Some(task) = tasks.join_next().await {
        match task {
            Ok(Err(error)) => result = Err(error),
            Err(error) => result = Err(io::Error::other(error)),
            _ => {}
        }
    }
    result
}

pub(crate) async fn serve_listener(
    listener: TcpListener,
    router: Router,
    mut stopped: watch::Receiver<bool>,
    mut configured: Option<watch::Receiver<bool>>,
) -> io::Result<()> {
    let host = listener.local_addr()?.to_string();
    let router = router.layer(middleware::from_fn_with_state(host, browser_boundary));
    let connections = Arc::new(Semaphore::new(64));
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
            _ = stopped.changed() => break,
            _ = async {
                match &mut configured {
                    Some(configured) => { if !*configured.borrow() { let _ = configured.changed().await; } },
                    None => std::future::pending().await,
                }
            } => break,
            accepted = listener.accept() => {
                let (stream, peer) = accepted?;
                if !peer.ip().is_loopback() { continue; }
                let Ok(permit) = connections.clone().try_acquire_owned() else { continue; };
                let service = TowerToHyperService::new(router.clone());
                tasks.spawn(async move {
                    let _permit = permit;
                    let _ = http1::Builder::new().timer(TokioTimer::new())
                        .header_read_timeout(Duration::from_secs(5)).max_headers(64).max_buf_size(16384)
                        .keep_alive(false).serve_connection(TokioIo::new(stream), service).await;
                });
            }
            _ = tasks.join_next(), if !tasks.is_empty() => {},
        }
    }
    drop(listener);
    if tokio::time::timeout(Duration::from_secs(5), async {
        while tasks.join_next().await.is_some() {}
    })
    .await
    .is_err()
    {
        tasks.shutdown().await;
    }
    Ok(())
}

async fn browser_boundary(State(host): State<String>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    let valid_host = headers.get_all(header::HOST).iter().count() == 1
        && headers
            .get(header::HOST)
            .is_some_and(|value| value.as_bytes() == host.as_bytes());
    let hostile = headers.contains_key(header::ORIGIN)
        || headers
            .keys()
            .any(|name| name.as_str().starts_with("sec-fetch-"));
    let mut response = if !valid_host || hostile {
        ApiError(StatusCode::FORBIDDEN, "forbidden").into_response()
    } else if headers
        .iter()
        .map(|(key, value)| key.as_str().len() + value.len())
        .sum::<usize>()
        > 8192
    {
        ApiError(
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
            "headers_too_large",
        )
        .into_response()
    } else {
        next.run(request).await
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

pub(crate) fn regular_router(manager: Manager) -> Router {
    Router::new()
        .merge(workers::router())
        .merge(tasks::router())
        .merge(inference::router())
        .merge(runs::router())
        .route(
            "/health",
            get(|| async { Json(serde_json::json!({"status":"ok"})) }),
        )
        .route("/v1/owner", get(owner))
        .route("/v1/sessions", post(sign_in).get(sessions))
        .route("/v1/session", delete(sign_out))
        .route("/v1/sessions/{id}", delete(revoke_session))
        .route("/v1/bots", get(list_bots))
        .route("/v1/bots/{id}", put(create_bot).get(get_bot))
        .route("/v1/status", get(status))
        .route("/v1/vault/unlock", post(unlock))
        .route("/v1/vault/lock", post(lock))
        .route("/v1/provider-keys/mistral", put(replace_key))
        .fallback(not_found)
        .method_not_allowed_fallback(not_found)
        .layer(DefaultBodyLimit::max(16384))
        .with_state(manager)
}

fn setup_router(setup: LocalSetup) -> Router {
    Router::new()
        .route("/v1/owner", post(create_owner))
        .fallback(not_found)
        .method_not_allowed_fallback(not_found)
        .layer(DefaultBodyLimit::max(16384))
        .with_state(setup)
}

async fn not_found() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "not_found")
}

struct ApiError(StatusCode, &'static str);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({"error":self.1}))).into_response()
    }
}
impl From<OperationError> for ApiError {
    fn from(error: OperationError) -> Self {
        match error {
            OperationError::Protocol(code) => Self(StatusCode::CONFLICT, code.as_str()),
            OperationError::InvalidInput => Self(StatusCode::BAD_REQUEST, "invalid_input"),
            OperationError::Unauthorized => Self(StatusCode::UNAUTHORIZED, "unauthorized"),
            OperationError::NotFound => Self(StatusCode::NOT_FOUND, "not_found"),
            OperationError::Conflict => Self(StatusCode::CONFLICT, "conflict"),
            OperationError::Locked => Self(StatusCode::CONFLICT, "vault_locked"),
            OperationError::UnlockFailed => Self(StatusCode::UNAUTHORIZED, "unlock_failed"),
            OperationError::RateLimited => Self(StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
            OperationError::Busy => Self(StatusCode::SERVICE_UNAVAILABLE, "busy"),
            OperationError::Internal => Self(StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
            OperationError::InferenceUnverified => {
                Self(StatusCode::CONFLICT, "inference_unverified")
            }
            OperationError::InferenceBudget => Self(StatusCode::CONFLICT, "inference_budget"),
        }
    }
}
type ApiResult<T> = Result<T, ApiError>;

struct StrictJson<T>(T);
struct ObjectBody<T>(T);
impl<'de, T: Deserialize<'de>> Deserialize<'de> for ObjectBody<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ObjectVisitor<T>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for ObjectVisitor<T> {
            type Value = ObjectBody<T>;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a JSON object")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                map: A,
            ) -> Result<Self::Value, A::Error> {
                T::deserialize(serde::de::value::MapAccessDeserializer::new(map)).map(ObjectBody)
            }
        }
        deserializer.deserialize_map(ObjectVisitor(std::marker::PhantomData))
    }
}

struct IdPath(String);
impl<S: Send + Sync> FromRequestParts<S> for IdPath {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Path::<String>::from_request_parts(parts, state)
            .await
            .map(|Path(id)| Self(id))
            .map_err(|_| OperationError::InvalidInput.into())
    }
}

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for StrictJson<T> {
    type Rejection = ApiError;
    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        if request
            .headers()
            .get_all(header::CONTENT_TYPE)
            .iter()
            .count()
            != 1
            || request
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .is_none_or(|value| value != "application/json")
        {
            return Err(ApiError(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "json_required",
            ));
        }
        match tokio::time::timeout(
            Duration::from_secs(5),
            Json::<ObjectBody<T>>::from_request(request, state),
        )
        .await
        {
            Ok(Ok(Json(ObjectBody(value)))) => Ok(Self(value)),
            Ok(Err(error)) if error.status() == StatusCode::PAYLOAD_TOO_LARGE => {
                Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE, "body_too_large"))
            }
            Ok(Err(_)) => Err(ApiError(StatusCode::BAD_REQUEST, "invalid_json")),
            Err(_) => Err(ApiError(StatusCode::REQUEST_TIMEOUT, "request_timeout")),
        }
    }
}

fn bearer(headers: &HeaderMap) -> ApiResult<String> {
    if headers.get_all(header::AUTHORIZATION).iter().count() != 1 {
        return Err(OperationError::Unauthorized.into());
    }
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| {
            value.len() == 43
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        })
        .map(str::to_owned)
        .ok_or(OperationError::Unauthorized.into())
}
fn token(headers: &HeaderMap) -> ApiResult<SessionToken> {
    SessionToken::new(bearer(headers)?).map_err(Into::into)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetupBody {
    name: String,
    password: SecretString,
    vault_passphrase: SecretString,
    acknowledge_no_escrow: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PasswordBody {
    password: SecretString,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UnlockBody {
    passphrase: SecretString,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyBody {
    secret: SecretString,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BotBody {
    name: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

#[derive(Serialize)]
struct OwnerWire {
    name: String,
}
impl From<OwnerProfile> for OwnerWire {
    fn from(owner: OwnerProfile) -> Self {
        Self {
            name: owner.name.as_str().into(),
        }
    }
}
#[derive(Serialize)]
struct BotWire {
    id: String,
    name: String,
    created_at: i64,
}
impl From<Bot> for BotWire {
    fn from(bot: Bot) -> Self {
        Self {
            id: bot.id.0.to_string(),
            name: bot.name.as_str().into(),
            created_at: bot.created_at,
        }
    }
}
#[derive(Serialize)]
struct SessionWire {
    id: String,
    created_at: i64,
    expires_at: i64,
}
impl From<SessionMetadata> for SessionWire {
    fn from(session: SessionMetadata) -> Self {
        Self {
            id: session.id.0.to_string(),
            created_at: session.created_at,
            expires_at: session.expires_at,
        }
    }
}
#[derive(Serialize)]
struct ProviderWire {
    provider: &'static str,
    updated_at: i64,
}
impl From<ProviderMetadata> for ProviderWire {
    fn from(metadata: ProviderMetadata) -> Self {
        Self {
            provider: "mistral",
            updated_at: metadata.updated_at,
        }
    }
}

async fn create_owner(
    State(setup): State<LocalSetup>,
    headers: HeaderMap,
    StrictJson(body): StrictJson<SetupBody>,
) -> ApiResult<(StatusCode, Json<OwnerWire>)> {
    if !body.acknowledge_no_escrow {
        return Err(OperationError::InvalidInput.into());
    }
    let profile = setup
        .create_owner(
            BootstrapToken::new(bearer(&headers)?)?,
            OwnerSetup {
                name: OwnerName::new(body.name)?,
                login_password: LoginPassword::from_secret(body.password)?,
                vault_passphrase: VaultPassphrase::from_secret(body.vault_passphrase)?,
                recovery: RecoveryAcknowledgment::OfflinePassphraseCopy,
            },
        )
        .await?;
    Ok((StatusCode::CREATED, Json(profile.into())))
}
async fn sign_in(
    State(manager): State<Manager>,
    StrictJson(body): StrictJson<PasswordBody>,
) -> ApiResult<Response> {
    use age::secrecy::ExposeSecret;
    let grant = manager
        .sign_in(LoginPassword::from_secret(body.password)?)
        .await?;
    let (token, session) = grant.into_response_parts();
    #[derive(Serialize)]
    struct GrantWire<'a> {
        token: &'a str,
        session: SessionWire,
    }
    Ok((
        StatusCode::CREATED,
        Json(GrantWire {
            token: token.expose_secret(),
            session: session.into(),
        }),
    )
        .into_response())
}
async fn owner(State(manager): State<Manager>, headers: HeaderMap) -> ApiResult<Json<OwnerWire>> {
    Ok(Json(manager.owner(token(&headers)?).await?.into()))
}
async fn sessions(
    State(manager): State<Manager>,
    headers: HeaderMap,
) -> ApiResult<Json<Vec<SessionWire>>> {
    Ok(Json(
        manager
            .sessions(token(&headers)?)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}
async fn sign_out(
    State(manager): State<Manager>,
    headers: HeaderMap,
    StrictJson(_): StrictJson<Empty>,
) -> ApiResult<StatusCode> {
    manager.sign_out(token(&headers)?).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn revoke_session(
    State(manager): State<Manager>,
    IdPath(id): IdPath,
    headers: HeaderMap,
    StrictJson(_): StrictJson<Empty>,
) -> ApiResult<StatusCode> {
    manager
        .revoke_session(
            token(&headers)?,
            SessionId(id.parse().map_err(|_| OperationError::InvalidInput)?),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn create_bot(
    State(manager): State<Manager>,
    IdPath(id): IdPath,
    headers: HeaderMap,
    StrictJson(body): StrictJson<BotBody>,
) -> ApiResult<Json<BotWire>> {
    Ok(Json(
        manager
            .create_bot(
                token(&headers)?,
                NewBot {
                    id: BotId(id.parse().map_err(|_| OperationError::InvalidInput)?),
                    name: BotName::new(body.name)?,
                },
            )
            .await?
            .into(),
    ))
}
async fn get_bot(
    State(manager): State<Manager>,
    IdPath(id): IdPath,
    headers: HeaderMap,
) -> ApiResult<Json<BotWire>> {
    Ok(Json(
        manager
            .get_bot(
                token(&headers)?,
                BotId(id.parse().map_err(|_| OperationError::InvalidInput)?),
            )
            .await?
            .into(),
    ))
}
async fn list_bots(
    State(manager): State<Manager>,
    headers: HeaderMap,
) -> ApiResult<Json<Vec<BotWire>>> {
    Ok(Json(
        manager
            .list_bots(token(&headers)?)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}
async fn status(
    State(manager): State<Manager>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    let status = manager.connection_status(token(&headers)?).await?;
    Ok(Json(serde_json::json!({
        "vault": match status.vault { VaultStatus::Locked => "locked", VaultStatus::Unlocked => "unlocked" },
        "provider": status.provider.map(ProviderWire::from), "worker":"not_implemented", "inference":"single_turn_text", "remote":"disabled",
    })))
}
async fn unlock(
    State(manager): State<Manager>,
    headers: HeaderMap,
    StrictJson(body): StrictJson<UnlockBody>,
) -> ApiResult<StatusCode> {
    manager
        .unlock_vault(
            token(&headers)?,
            VaultPassphrase::from_secret(body.passphrase)?,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn lock(
    State(manager): State<Manager>,
    headers: HeaderMap,
    StrictJson(_): StrictJson<Empty>,
) -> ApiResult<StatusCode> {
    manager.lock_vault(token(&headers)?).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn replace_key(
    State(manager): State<Manager>,
    headers: HeaderMap,
    StrictJson(body): StrictJson<KeyBody>,
) -> ApiResult<Json<ProviderWire>> {
    Ok(Json(
        manager
            .replace_provider_key(token(&headers)?, ProviderSecret::from_secret(body.secret)?)
            .await?
            .into(),
    ))
}
