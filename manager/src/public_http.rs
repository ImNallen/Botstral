use crate::manager::{Manager, WorkerGateway};
use crate::worker_http::VerifiedIdentity;
use axum::body::Body;
use botstral_link::{ErrorCode, MAX_MESSAGE, tls};
use hyper::{
    Request, Response, StatusCode, body::Incoming, server::conn::http1, service::service_fn,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use rustls::{ServerConfig, server::WebPkiClientVerifier};
use std::{convert::Infallible, io, net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    net::TcpListener,
    sync::{Semaphore, oneshot, watch},
    task::JoinSet,
};
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{
        handshake::server::create_response,
        protocol::{Role, WebSocketConfig},
    },
};
const DEADLINE: Duration = Duration::from_secs(5);
const CONNECTIONS: usize = 16;
pub struct PublicTls {
    pub listen: SocketAddr,
    pub certificate: PathBuf,
    pub key: PathBuf,
    pub client_ca: PathBuf,
}

pub(crate) struct PublicListener {
    listener: TcpListener,
    acceptor: TlsAcceptor,
}
impl PublicListener {
    pub(crate) async fn bind(config: PublicTls) -> io::Result<Self> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = WebPkiClientVerifier::builder_with_provider(
            Arc::new(tls::roots(&config.client_ca)?),
            provider.clone(),
        )
        .allow_unauthenticated()
        .build()
        .map_err(io::Error::other)?;
        let config_tls = ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(io::Error::other)?
            .with_client_cert_verifier(verifier)
            .with_single_cert(
                tls::certificates(&config.certificate)?,
                tls::private_key(&config.key)?,
            )
            .map_err(io::Error::other)?;
        Ok(Self {
            listener: TcpListener::bind(config.listen).await?,
            acceptor: TlsAcceptor::from(Arc::new(config_tls)),
        })
    }
    pub(crate) fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }
    pub(crate) async fn serve(
        self,
        manager: Manager,
        mut stopped: watch::Receiver<bool>,
    ) -> io::Result<()> {
        let permits = Arc::new(Semaphore::new(CONNECTIONS));
        let mut tasks = JoinSet::new();
        loop {
            tokio::select! {
                biased;
                _ = stopped.changed() => break,
                _ = tasks.join_next(), if !tasks.is_empty() => {},
                accepted = self.listener.accept() => {
                    let (stream, _) = accepted?;
                    let Ok(permit) = permits.clone().try_acquire_owned() else { continue };
                    let acceptor = self.acceptor.clone();
                    let gateway = manager.worker_gateway();
                    let manager = manager.clone();
                    let mut stop = stopped.clone();
                    tasks.spawn(async move {
                        let _permit = permit;
                        let connection = async {
                            let stream = tokio::time::timeout(DEADLINE, acceptor.accept(stream)).await.map_err(io::Error::other)??;
                            let peer = stream.get_ref().1.peer_certificates().and_then(|certs| certs.first()).map(|certificate| VerifiedIdentity::from_verified(certificate.as_ref()));
                            let (upgrade_tx, upgrade_rx) = oneshot::channel();
                            let upgrade_tx = Arc::new(std::sync::Mutex::new(Some(upgrade_tx)));
                            let service_gateway = gateway.clone();
                            let service_manager = manager.clone();
                            let service_peer = peer.clone();
                            let service = service_fn(move |request| public_request(request, service_peer.clone(), service_gateway.clone(), service_manager.clone(), upgrade_tx.clone()));
                            tokio::time::timeout(Duration::from_secs(125), http1::Builder::new().timer(TokioTimer::new()).header_read_timeout(DEADLINE)
                                .max_headers(32).max_buf_size(16384).serve_connection(TokioIo::new(stream), service).with_upgrades()).await.map_err(io::Error::other)?.map_err(io::Error::other)?;
                            if let Ok((upgrade, owner)) = upgrade_rx.await {
                                let stream = tokio::time::timeout(DEADLINE, upgrade).await.map_err(io::Error::other)?.map_err(io::Error::other)?;
                                let config = WebSocketConfig::default().max_message_size(Some(MAX_MESSAGE)).max_frame_size(Some(MAX_MESSAGE));
                                let socket = WebSocketStream::from_raw_socket(TokioIo::new(stream), Role::Server, Some(config)).await;
                                if owner { crate::owner_http::connected(socket, manager.clone()).await?; } else { crate::worker_http::connected(socket, gateway.clone(), peer.expect("worker route requires certificate")).await?; }
                            }
                            Ok::<_, io::Error>(())
                        };
                        tokio::select! { _ = stop.changed() => {}, _ = connection => {} }
                    });
                }
            }
        }
        drop(self.listener);
        if tokio::time::timeout(DEADLINE, async {
            while tasks.join_next().await.is_some() {}
        })
        .await
        .is_err()
        {
            tasks.shutdown().await;
        }
        Ok(())
    }
}

pub(crate) type UpgradeSender =
    Arc<std::sync::Mutex<Option<oneshot::Sender<(hyper::upgrade::OnUpgrade, bool)>>>>;
async fn public_request(
    mut request: Request<Incoming>,
    peer: Option<VerifiedIdentity>,
    gateway: WorkerGateway,
    manager: Manager,
    upgrade: UpgradeSender,
) -> Result<Response<Body>, Infallible> {
    use crate::worker_http::{error_response, json_response};
    if request.headers().contains_key("origin")
        || request
            .headers()
            .keys()
            .any(|h| h.as_str().starts_with("sec-fetch-"))
    {
        return Ok(error_response(
            StatusCode::FORBIDDEN,
            ErrorCode::Unauthorized,
        ));
    }
    if request.method() == "POST"
        && request.uri().path().starts_with("/events/v1/publishers/")
        && request.uri().query().is_none()
    {
        use http_body_util::BodyExt;
        let publisher = request
            .uri()
            .path()
            .trim_start_matches("/events/v1/publishers/")
            .parse::<botstral_link::PublisherId>();
        let credential = request
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::to_owned);
        let (Ok(publisher), Some(credential)) = (publisher, credential) else {
            return Ok(error_response(
                StatusCode::UNAUTHORIZED,
                ErrorCode::Unauthorized,
            ));
        };
        let collected = tokio::time::timeout(
            DEADLINE,
            http_body_util::Limited::new(request.into_body(), botstral_link::MAX_EVENT_BYTES)
                .collect(),
        )
        .await;
        let bytes = match collected {
            Ok(Ok(body)) => body.to_bytes().to_vec(),
            _ => {
                return Ok(error_response(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    ErrorCode::InvalidMessage,
                ));
            }
        };
        return Ok(
            match manager
                .publish_routine_event(publisher, credential, bytes)
                .await
            {
                Ok(receipt) => json_response(
                    StatusCode::ACCEPTED,
                    serde_json::to_value(receipt).expect("receipt serializes"),
                ),
                Err(error) => {
                    use crate::manager::OperationError;
                    let status = match error {
                        OperationError::Unauthorized | OperationError::NotFound => {
                            StatusCode::UNAUTHORIZED
                        }
                        OperationError::Conflict => StatusCode::CONFLICT,
                        OperationError::Locked | OperationError::Busy => {
                            StatusCode::SERVICE_UNAVAILABLE
                        }
                        OperationError::RateLimited => StatusCode::TOO_MANY_REQUESTS,
                        OperationError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
                        _ => StatusCode::BAD_REQUEST,
                    };
                    json_response(status, serde_json::json!({"error":format!("{error:?}")}))
                }
            },
        );
    }
    if request.uri().path().starts_with("/worker/") {
        return match peer {
            Some(peer) => {
                crate::worker_http::worker_request(request, peer, gateway, manager, upgrade).await
            }
            None => Ok(error_response(
                StatusCode::FORBIDDEN,
                ErrorCode::Unauthorized,
            )),
        };
    }
    if request.method() == "GET"
        && request.uri().path() == "/owner/v1/connect"
        && request.uri().query().is_none()
    {
        if peer.is_some() {
            return Ok(error_response(
                StatusCode::FORBIDDEN,
                ErrorCode::Unauthorized,
            ));
        }
        let mut head = Request::builder()
            .version(request.version())
            .method(request.method())
            .uri(request.uri());
        *head.headers_mut().unwrap() = request.headers().clone();
        return Ok(match create_response(&head.body(()).unwrap()) {
            Ok(response) => {
                if let Some(sender) = upgrade.lock().unwrap().take() {
                    let _ = sender.send((hyper::upgrade::on(&mut request), true));
                    response.map(|_| Body::empty())
                } else {
                    error_response(StatusCode::BAD_REQUEST, ErrorCode::WrongPhase)
                }
            }
            Err(_) => error_response(StatusCode::BAD_REQUEST, ErrorCode::InvalidMessage),
        });
    }
    Ok(json_response(
        StatusCode::NOT_FOUND,
        serde_json::json!({"error":"not_found"}),
    ))
}
