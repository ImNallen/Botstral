use crate::manager::{ConnectionLease, Manager, OperationError, WorkerGateway};
use age::secrecy::SecretString;
use axum::body::{Body, to_bytes};
use botstral_link::{tls, *};
use futures_util::{SinkExt, StreamExt};
use hyper::{
    Request, Response, StatusCode, body::Incoming, server::conn::http1, service::service_fn,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use rustls::{ServerConfig, server::WebPkiClientVerifier};
use std::{convert::Infallible, io, net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    net::TcpListener,
    sync::{Semaphore, mpsc, oneshot, watch},
    task::JoinSet,
};
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{
        Message,
        handshake::server::create_response,
        protocol::{Role, WebSocketConfig},
    },
};

const DEADLINE: Duration = Duration::from_secs(5);
const CONNECTIONS: usize = 16;

#[derive(Clone)]
pub(crate) struct VerifiedIdentity(CertificateFingerprint);
impl VerifiedIdentity {
    pub(crate) fn fingerprint(&self) -> &CertificateFingerprint {
        &self.0
    }
    #[cfg(test)]
    pub(crate) fn for_test(der: &[u8]) -> Self {
        Self(CertificateFingerprint::from_der(der))
    }
}

pub struct WorkerTls {
    pub listen: SocketAddr,
    pub certificate: PathBuf,
    pub key: PathBuf,
    pub client_ca: PathBuf,
}

pub(crate) struct WorkerListener {
    listener: TcpListener,
    acceptor: TlsAcceptor,
}
impl WorkerListener {
    pub(crate) async fn bind(config: WorkerTls) -> io::Result<Self> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = WebPkiClientVerifier::builder_with_provider(
            Arc::new(tls::roots(&config.client_ca)?),
            provider.clone(),
        )
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
                    let mut stop = stopped.clone();
                    tasks.spawn(async move {
                        let _permit = permit;
                        let connection = async {
                            let stream = tokio::time::timeout(DEADLINE, acceptor.accept(stream)).await.map_err(io::Error::other)??;
                            let certificate = stream.get_ref().1.peer_certificates().and_then(|certs| certs.first()).ok_or_else(|| io::Error::other("missing client certificate"))?;
                            let peer = VerifiedIdentity(CertificateFingerprint::from_der(certificate.as_ref()));
                            let (upgrade_tx, upgrade_rx) = oneshot::channel();
                            let upgrade_tx = Arc::new(std::sync::Mutex::new(Some(upgrade_tx)));
                            let service_gateway = gateway.clone();
                            let service_peer = peer.clone();
                            let service = service_fn(move |request| worker_request(request, service_peer.clone(), service_gateway.clone(), upgrade_tx.clone()));
                            tokio::time::timeout(DEADLINE, http1::Builder::new().timer(TokioTimer::new()).header_read_timeout(DEADLINE)
                                .max_headers(32).max_buf_size(16384).serve_connection(TokioIo::new(stream), service).with_upgrades()).await.map_err(io::Error::other)?.map_err(io::Error::other)?;
                            if let Ok(upgrade) = upgrade_rx.await {
                                let stream = tokio::time::timeout(DEADLINE, upgrade).await.map_err(io::Error::other)?.map_err(io::Error::other)?;
                                let config = WebSocketConfig::default().max_message_size(Some(MAX_MESSAGE)).max_frame_size(Some(MAX_MESSAGE));
                                let socket = WebSocketStream::from_raw_socket(TokioIo::new(stream), Role::Server, Some(config)).await;
                                connected(socket, gateway.clone(), peer).await?;
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

type UpgradeSender = Arc<std::sync::Mutex<Option<oneshot::Sender<hyper::upgrade::OnUpgrade>>>>;
async fn worker_request(
    mut request: Request<Incoming>,
    peer: VerifiedIdentity,
    gateway: WorkerGateway,
    upgrade: UpgradeSender,
) -> Result<Response<Body>, Infallible> {
    let response = if request.headers().contains_key("origin")
        || request
            .headers()
            .keys()
            .any(|h| h.as_str().starts_with("sec-fetch-"))
    {
        error_response(StatusCode::FORBIDDEN, ErrorCode::Unauthorized)
    } else if request.method() == "POST" && request.uri().path() == "/worker/v1/enroll" {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Enrollment {
            invitation: SecretString,
        }
        let result = async {
            if request
                .headers()
                .get("content-type")
                .and_then(|h| h.to_str().ok())
                != Some("application/json")
            {
                return Err(OperationError::InvalidInput);
            }
            let bytes = to_bytes(Body::new(request.into_body()), 1024)
                .await
                .map_err(|_| OperationError::InvalidInput)?;
            let body: Enrollment =
                serde_json::from_slice(&bytes).map_err(|_| OperationError::InvalidInput)?;
            gateway.enroll(peer, body.invitation).await
        }
        .await;
        match result {
            Ok(worker_id) => json_response(
                StatusCode::CREATED,
                serde_json::json!({"worker_id": worker_id}),
            ),
            Err(error) => error_response(StatusCode::FORBIDDEN, protocol_error(error)),
        }
    } else if request.method() == "GET"
        && request.uri().path() == "/worker/v1/connect"
        && request.uri().query().is_none()
    {
        let mut head = Request::builder()
            .version(request.version())
            .method(request.method())
            .uri(request.uri());
        *head.headers_mut().unwrap() = request.headers().clone();
        match create_response(&head.body(()).unwrap()) {
            Ok(response) => {
                let sender = upgrade.lock().unwrap().take();
                if let Some(sender) = sender {
                    let _ = sender.send(hyper::upgrade::on(&mut request));
                    response.map(|_| Body::empty())
                } else {
                    error_response(StatusCode::BAD_REQUEST, ErrorCode::WrongPhase)
                }
            }
            Err(_) => error_response(StatusCode::BAD_REQUEST, ErrorCode::InvalidMessage),
        }
    } else {
        json_response(
            StatusCode::NOT_FOUND,
            serde_json::json!({"error":"not_found"}),
        )
    };
    Ok(response)
}

fn json_response(status: StatusCode, value: serde_json::Value) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("connection", "close")
        .header("content-type", "application/json")
        .header("cache-control", "no-store")
        .body(Body::from(value.to_string()))
        .unwrap()
}
fn error_response(status: StatusCode, code: ErrorCode) -> Response<Body> {
    json_response(status, serde_json::json!({"error":code}))
}
pub(crate) fn protocol_error(error: OperationError) -> ErrorCode {
    match error {
        OperationError::Protocol(code) => code,
        OperationError::Busy => ErrorCode::QueueFull,
        OperationError::Unauthorized => ErrorCode::Unauthorized,
        _ => ErrorCode::InvalidMessage,
    }
}

type Socket = WebSocketStream<TokioIo<hyper::upgrade::Upgraded>>;
async fn send(socket: &mut Socket, message: Envelope<ManagerMessage>) -> io::Result<()> {
    tokio::time::timeout(
        DEADLINE,
        socket.send(Message::Text(serde_json::to_string(&message)?.into())),
    )
    .await
    .map_err(io::Error::other)?
    .map_err(io::Error::other)
}
async fn receive(socket: &mut Socket) -> Result<Envelope<WorkerMessage>, ErrorCode> {
    match socket.next().await {
        Some(Ok(Message::Text(text))) => decode(text.as_bytes()),
        _ => Err(ErrorCode::InvalidMessage),
    }
}
async fn connected(
    mut socket: Socket,
    gateway: WorkerGateway,
    peer: VerifiedIdentity,
) -> io::Result<()> {
    let first = tokio::time::timeout(DEADLINE, receive(&mut socket)).await;
    let (request_id, hello) = match first {
        Ok(Ok(Envelope {
            request_id,
            message: WorkerMessage::Hello(hello),
            ..
        })) => (request_id, hello),
        other => {
            let code = match other {
                Ok(Err(code)) => code,
                _ => ErrorCode::WrongPhase,
            };
            return send(&mut socket, Envelope::new(ManagerMessage::Error { code })).await;
        }
    };
    let (outbound, mut receive_outbound) = mpsc::channel(CHANNEL_CAPACITY);
    let (stop, mut stopped) = watch::channel(false);
    let lease = match gateway
        .connect(peer, hello, outbound, stop, request_id)
        .await
    {
        Ok(lease) => lease,
        Err(error) => {
            return send(
                &mut socket,
                Envelope {
                    version: VERSION,
                    request_id,
                    message: ManagerMessage::Error {
                        code: protocol_error(error),
                    },
                },
            )
            .await;
        }
    };
    let operation = async {
        let welcome = receive_outbound
            .recv()
            .await
            .ok_or_else(|| io::Error::other("connection closed"))?;
        send(&mut socket, welcome).await?;
        let snapshot = tokio::time::timeout(DEADLINE, receive(&mut socket))
            .await
            .map_err(io::Error::other)?
            .map_err(io::Error::other)?;
        gateway
            .receive(lease, snapshot)
            .await
            .map_err(|_| io::Error::other("snapshot rejected"))?;
        socket_loop(&mut socket, &gateway, lease, &mut receive_outbound).await
    };
    let result = tokio::select! { _ = stopped.changed() => Ok(()), result = operation => result };
    let _ = gateway.disconnected(lease).await;
    result
}
async fn socket_loop(
    socket: &mut Socket,
    gateway: &WorkerGateway,
    lease: ConnectionLease,
    outbound: &mut mpsc::Receiver<Envelope<ManagerMessage>>,
) -> io::Result<()> {
    let idle = tokio::time::sleep(Duration::from_secs(30));
    tokio::pin!(idle);
    loop {
        tokio::select! {
            _ = &mut idle => return Ok(()),
            message = outbound.recv() => {
                let Some(message) = message else { return Ok(()) };
                if gateway.admit_delivery(lease, message.message.clone()).await.is_ok() { send(socket, message).await?; }
            }
            report = receive(socket) => {
                let report = report.map_err(io::Error::other)?;
                idle.as_mut().reset(tokio::time::Instant::now() + Duration::from_secs(30));
                let request_id = report.request_id;
                if let Err(error) = gateway.receive(lease, report).await {
                    send(socket, Envelope { version: VERSION, request_id, message: ManagerMessage::Error { code: protocol_error(error) } }).await?;
                    return Ok(());
                }
            }
        }
    }
}
