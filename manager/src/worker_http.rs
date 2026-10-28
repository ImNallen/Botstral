use crate::manager::{ConnectionLease, Manager, OperationError, WorkerGateway};
use age::secrecy::SecretString;
use axum::body::{Body, to_bytes};
use botstral_link::*;
use futures_util::{SinkExt, StreamExt};
use hyper::{Request, Response, StatusCode, body::Incoming};
use hyper_util::rt::TokioIo;
use std::{convert::Infallible, io, time::Duration};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, handshake::server::create_response},
};

const DEADLINE: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub(crate) struct VerifiedIdentity(CertificateFingerprint);
impl VerifiedIdentity {
    pub(crate) fn from_verified(der: &[u8]) -> Self {
        Self(CertificateFingerprint::from_der(der))
    }
    pub(crate) fn fingerprint(&self) -> &CertificateFingerprint {
        &self.0
    }
    #[cfg(test)]
    pub(crate) fn for_test(der: &[u8]) -> Self {
        Self(CertificateFingerprint::from_der(der))
    }
}

pub(crate) use crate::public_http::PublicListener as WorkerListener;
pub use crate::public_http::PublicTls as WorkerTls;
use crate::public_http::UpgradeSender;
pub(crate) async fn worker_request(
    mut request: Request<Incoming>,
    peer: VerifiedIdentity,
    gateway: WorkerGateway,
    manager: Manager,
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
    } else if request.method() == "POST"
        && request.uri().path().starts_with("/worker/v1/connectors/")
    {
        let result = async {
            if request.uri().query().is_some()
                || request
                    .headers()
                    .get("content-type")
                    .and_then(|h| h.to_str().ok())
                    != Some("application/json")
            {
                return Err(OperationError::InvalidInput);
            }
            let parts = request
                .uri()
                .path()
                .trim_start_matches("/worker/v1/connectors/")
                .split('/')
                .collect::<Vec<_>>();
            if parts.len() != 2 {
                return Err(OperationError::InvalidInput);
            }
            let task = parts[0].parse().map_err(|_| OperationError::InvalidInput)?;
            let execution = parts[1].parse().map_err(|_| OperationError::InvalidInput)?;
            let bytes = tokio::time::timeout(
                DEADLINE,
                to_bytes(Body::new(request.into_body()), MAX_CONNECTOR_BYTES),
            )
            .await
            .map_err(|_| OperationError::InvalidInput)?
            .map_err(|_| OperationError::InvalidInput)?;
            let input = serde_json::from_slice(&bytes).map_err(|_| OperationError::InvalidInput)?;
            manager
                .connector_request(peer, task, execution, input)
                .await
        }
        .await;
        match result {
            Ok(reply) => match serde_json::to_value(reply) {
                Ok(value) => json_response(StatusCode::OK, value),
                Err(_) => {
                    error_response(StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::InvalidMessage)
                }
            },
            Err(e) => error_response(StatusCode::FORBIDDEN, protocol_error(e)),
        }
    } else if request.method() == "POST" && request.uri().path().starts_with("/worker/v1/coding/") {
        let result = async {
            if request.uri().query().is_some()
                || request
                    .headers()
                    .get("content-type")
                    .and_then(|h| h.to_str().ok())
                    != Some("application/json")
            {
                return Err(OperationError::InvalidInput);
            }
            let parts = request
                .uri()
                .path()
                .trim_start_matches("/worker/v1/coding/")
                .split('/')
                .collect::<Vec<_>>();
            if parts.len() != 3 {
                return Err(OperationError::InvalidInput);
            }
            let task = parts[0].parse().map_err(|_| OperationError::InvalidInput)?;
            let execution = parts[1].parse().map_err(|_| OperationError::InvalidInput)?;
            let attempt = parts[2].parse().map_err(|_| OperationError::InvalidInput)?;
            let body = tokio::time::timeout(
                DEADLINE,
                to_bytes(Body::new(request.into_body()), MAX_CODING_REQUEST),
            )
            .await
            .map_err(|_| OperationError::InvalidInput)?
            .map_err(|_| OperationError::InvalidInput)?;
            manager
                .coding_chat(peer, task, execution, attempt, body.to_vec())
                .await
        }
        .await;
        match result {
            Ok((false, value)) => json_response(StatusCode::OK, value),
            Ok((true, value)) => Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/event-stream")
                .header("connection", "close")
                .header("cache-control", "no-store")
                .body(Body::from(crate::manager::coding::sse(value)))
                .unwrap(),
            Err(error) => error_response(StatusCode::CONFLICT, protocol_error(error)),
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
                    let _ = sender.send((hyper::upgrade::on(&mut request), false));
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

pub(crate) fn json_response(status: StatusCode, value: serde_json::Value) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("connection", "close")
        .header("content-type", "application/json")
        .header("cache-control", "no-store")
        .body(Body::from(value.to_string()))
        .unwrap()
}
pub(crate) fn error_response(status: StatusCode, code: ErrorCode) -> Response<Body> {
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
pub(crate) async fn connected(
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
