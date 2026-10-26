use crate::{Config, session::Sessions};
use botstral_link::{tls, *};
use futures_util::{SinkExt, StreamExt};
use http_body_util::{BodyExt, Full, Limited};
use hyper::{Request, Uri, body::Bytes};
use hyper_util::rt::TokioIo;
use rustls::{ClientConfig, pki_types::ServerName};
use std::{io, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};
use tokio_rustls::{TlsConnector, client::TlsStream};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, protocol::WebSocketConfig},
};

const DEADLINE: Duration = Duration::from_secs(5);

pub(crate) fn endpoint(config: &Config) -> io::Result<(String, u16)> {
    let uri: Uri = config.manager.parse().map_err(io::Error::other)?;
    if uri.scheme_str() != Some("wss")
        || uri.authority().is_none_or(|a| a.as_str().contains('@'))
        || uri.path_and_query().is_some_and(|p| p.as_str() != "/")
    {
        return Err(io::Error::other("expected wss manager origin"));
    }
    Ok((
        uri.host()
            .ok_or_else(|| io::Error::other("missing host"))?
            .to_string(),
        uri.port_u16().unwrap_or(443),
    ))
}

pub async fn connect_tls(config: &Config) -> io::Result<TlsStream<TcpStream>> {
    let (host, port) = endpoint(config)?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let tls_config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(io::Error::other)?
        .with_root_certificates(tls::roots(&config.manager_ca)?)
        .with_client_auth_cert(
            tls::certificates(&config.client_cert)?,
            tls::private_key(&config.client_key)?,
        )
        .map_err(io::Error::other)?;
    let server_name = ServerName::try_from(host.clone()).map_err(io::Error::other)?;
    tokio::time::timeout(DEADLINE, async {
        let stream = TcpStream::connect((host.as_str(), port)).await?;
        TlsConnector::from(Arc::new(tls_config))
            .connect(server_name, stream)
            .await
    })
    .await
    .map_err(io::Error::other)?
}

pub async fn enroll(config: &Config, invitation: &[u8]) -> io::Result<()> {
    #[derive(serde::Deserialize)]
    struct Invitation<'a> {
        invitation: &'a str,
    }
    let grant: Invitation = serde_json::from_slice(invitation)?;
    let body = zeroize::Zeroizing::new(serde_json::to_vec(
        &serde_json::json!({"invitation":grant.invitation}),
    )?);
    let stream = connect_tls(config).await?;
    tokio::time::timeout(DEADLINE, async {
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .map_err(io::Error::other)?;
        let request = Request::builder()
            .method("POST")
            .uri("/worker/v1/enroll")
            .header("host", config.manager.trim_start_matches("wss://"))
            .header("content-type", "application/json")
            .header("connection", "close")
            .body(Full::new(Bytes::copy_from_slice(&body)))
            .map_err(io::Error::other)?;
        let exchange = async {
            let response = sender
                .send_request(request)
                .await
                .map_err(io::Error::other)?;
            let status = response.status();
            Limited::new(response.into_body(), 4096)
                .collect()
                .await
                .map_err(io::Error::other)?;
            if status != 201 {
                return Err(io::Error::other("enrollment refused"));
            }
            Ok(())
        };
        let (result, _) = tokio::join!(exchange, connection);
        result
    })
    .await
    .map_err(io::Error::other)?
}

pub async fn connect_socket(config: &Config) -> io::Result<WebSocketStream<TlsStream<TcpStream>>> {
    let stream = connect_tls(config).await?;
    let url = format!("{}/worker/v1/connect", config.manager.trim_end_matches('/'));
    let limits = WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE))
        .max_frame_size(Some(MAX_MESSAGE));
    tokio::time::timeout(
        DEADLINE,
        tokio_tungstenite::client_async_with_config(url, stream, Some(limits)),
    )
    .await
    .map_err(io::Error::other)?
    .map(|(socket, _)| socket)
    .map_err(io::Error::other)
}

pub async fn run(config: Config, test_worker: bool, once: bool) -> io::Result<()> {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let operation = async {
        let mut retry = Duration::from_millis(200);
        loop {
            let result = connection(&config, test_worker, once).await;
            if once {
                return result;
            }
            eprintln!("worker disconnected; retrying");
            tokio::time::sleep(retry).await;
            retry = (retry * 2).min(DEADLINE);
        }
    };
    tokio::select! { _ = terminate.recv() => Ok(()), _ = interrupt.recv() => Ok(()), result = operation => result }
}

async fn send<S: AsyncRead + AsyncWrite + Unpin>(
    socket: &mut WebSocketStream<S>,
    request_id: RequestId,
    message: WorkerMessage,
) -> io::Result<()> {
    let message = Envelope {
        version: VERSION,
        request_id,
        message,
    };
    tokio::time::timeout(
        DEADLINE,
        socket.send(Message::Text(serde_json::to_string(&message)?.into())),
    )
    .await
    .map_err(io::Error::other)?
    .map_err(io::Error::other)
}
async fn receive<S: AsyncRead + AsyncWrite + Unpin>(
    socket: &mut WebSocketStream<S>,
) -> io::Result<Envelope<ManagerMessage>> {
    match socket.next().await {
        Some(Ok(Message::Text(text))) => decode(text.as_bytes()).map_err(io::Error::other),
        _ => Err(io::Error::other("connection closed")),
    }
}

async fn connection(config: &Config, test_worker: bool, once: bool) -> io::Result<()> {
    let mut socket = connect_socket(config).await?;
    send(
        &mut socket,
        RequestId::new(),
        WorkerMessage::Hello(Hello::current(test_worker)),
    )
    .await?;
    let welcome = tokio::time::timeout(DEADLINE, receive(&mut socket))
        .await
        .map_err(io::Error::other)??;
    let ManagerMessage::Welcome {
        worker_id,
        generation,
        sessions,
    } = welcome.message
    else {
        return Err(io::Error::other("negotiation refused"));
    };
    let mut sessions = Sessions::new(generation, sessions, worker_id).map_err(io::Error::other)?;
    send(
        &mut socket,
        welcome.request_id,
        WorkerMessage::SnapshotApplied { generation },
    )
    .await?;
    println!("worker ready");
    if once {
        return Ok(());
    }
    let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
    let idle = tokio::time::sleep(Duration::from_secs(30));
    tokio::pin!(idle);
    loop {
        tokio::select! {
            biased;
            message = receive(&mut socket) => {
                let message = message?;
                idle.as_mut().reset(tokio::time::Instant::now() + Duration::from_secs(30));
                let report = match message.message {
                    ManagerMessage::Control { generation, session } => {
                        let fence = sessions.control(generation, session).map_err(io::Error::other)?;
                        WorkerMessage::ControlApplied { fence }
                    }
                    ManagerMessage::Command(command) => {
                        let code = sessions.check(command.fence).err().unwrap_or(ErrorCode::UnsupportedExecution);
                        WorkerMessage::Acknowledgment { generation, command_id: command.id, outcome: Acknowledgment::Rejected { code } }
                    }
                    ManagerMessage::Input { command_id, fence, .. } => {
                        let code = sessions.check(fence).err().unwrap_or(ErrorCode::UnsupportedExecution);
                        WorkerMessage::Acknowledgment { generation, command_id, outcome: Acknowledgment::Rejected { code } }
                    }
                    ManagerMessage::Pong | ManagerMessage::Receipt { .. } => continue,
                    _ => return Err(io::Error::other("unexpected manager message")),
                };
                send(&mut socket, message.request_id, report).await?;
            }
            _ = &mut idle => return Err(io::Error::other("manager idle")),
            _ = heartbeat.tick() => { send(&mut socket, RequestId::new(), WorkerMessage::Heartbeat { generation }).await?; }
        }
    }
}
