use botstral_agent::{Config, transport};
use botstral_link::*;
use futures_util::{SinkExt, StreamExt};
use http_body_util::{BodyExt, Full};
use hyper::{Request, body::Bytes};
use hyper_util::rt::TokioIo;
use rcgen::{
    BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose,
};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    net::SocketAddr,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Child, Command},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

type Socket = WebSocketStream<tokio_rustls::client::TlsStream<TcpStream>>;

fn private(path: &Path, bytes: &[u8]) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
}
struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
impl Process {
    async fn stop(&mut self) {
        unsafe {
            libc::kill(self.0.id() as i32, libc::SIGTERM);
        }
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                assert!(status.success(), "clean shutdown: {status}");
                break;
            }
            assert!(Instant::now() < deadline, "bounded shutdown");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}
fn launch(binary: &Path, args: &[&str], root: &Path, name: &str) -> Process {
    let log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(root.join(format!("{name}.log")))
        .unwrap();
    Process(
        Command::new(binary)
            .args(args)
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap(),
    )
}
fn manager_binary() -> PathBuf {
    std::env::var_os("BOTSTRAL_MANAGER_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("botstral-manager")
        })
}
fn agent_binary() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_botstral-agent"))
}

async fn http<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    stream: S,
    host: &str,
    method: &str,
    path: &str,
    token: Option<&str>,
    value: Value,
) -> (u16, Value) {
    tokio::time::timeout(Duration::from_secs(10), async {
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .unwrap();
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("host", host)
            .header("connection", "close")
            .header("content-type", "application/json");
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let request = request
            .body(Full::new(Bytes::from(serde_json::to_vec(&value).unwrap())))
            .unwrap();
        let exchange = async {
            let response = sender.send_request(request).await.unwrap();
            let status = response.status().as_u16();
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            (
                status,
                if bytes.is_empty() {
                    Value::Null
                } else {
                    serde_json::from_slice(&bytes).unwrap()
                },
            )
        };
        let (result, _) = tokio::join!(exchange, connection);
        result
    })
    .await
    .expect("HTTP deadline")
}
async fn local(
    address: SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
    value: Value,
) -> (u16, Value) {
    http(
        TcpStream::connect(address).await.unwrap(),
        &address.to_string(),
        method,
        path,
        token,
        value,
    )
    .await
}
async fn worker_http(config: &Config, method: &str, path: &str, value: Value) -> (u16, Value) {
    http(
        transport::connect_tls(config).await.unwrap(),
        "localhost",
        method,
        path,
        None,
        value,
    )
    .await
}
async fn wire(socket: &mut Socket, message: WorkerMessage) {
    socket
        .send(Message::Text(
            serde_json::to_string(&Envelope::new(message))
                .unwrap()
                .into(),
        ))
        .await
        .unwrap();
}
async fn read(socket: &mut Socket) -> ManagerMessage {
    let message = tokio::time::timeout(Duration::from_secs(7), socket.next())
        .await
        .expect("frame deadline")
        .expect("frame")
        .expect("valid websocket");
    let Message::Text(text) = message else {
        panic!("text frame")
    };
    decode::<ManagerMessage>(text.as_bytes()).unwrap().message
}

struct Fixture {
    root: tempfile::TempDir,
    manager: Process,
    listen: SocketAddr,
    token: String,
    worker: Config,
    other: Config,
    wrong: Config,
    expired: Config,
}
impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let p = root.path();
        let mut params = CertificateParams::new(vec![]).unwrap();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::CrlSign,
        ];
        let key = KeyPair::generate().unwrap();
        let ca = params.self_signed(&key).unwrap();
        private(&p.join("ca.pem"), ca.pem().as_bytes());
        for (name, server, expired) in [
            ("server", true, false),
            ("worker", false, false),
            ("other", false, false),
            ("expired", false, true),
            ("race", false, false),
            ("recovery", false, false),
        ] {
            let mut params = CertificateParams::new(vec!["localhost".into()]).unwrap();
            params.extended_key_usages = vec![if server {
                ExtendedKeyUsagePurpose::ServerAuth
            } else {
                ExtendedKeyUsagePurpose::ClientAuth
            }];
            if expired {
                params.not_before = time::OffsetDateTime::now_utc() - time::Duration::days(2);
                params.not_after = time::OffsetDateTime::now_utc() - time::Duration::days(1);
            }
            let client_key = KeyPair::generate().unwrap();
            let cert = params.signed_by(&client_key, &ca, &key).unwrap();
            private(&p.join(format!("{name}.pem")), cert.pem().as_bytes());
            private(
                &p.join(format!("{name}.key")),
                client_key.serialize_pem().as_bytes(),
            );
        }
        let wrong_key = KeyPair::generate().unwrap();
        let mut wrong_params = CertificateParams::new(vec!["localhost".into()]).unwrap();
        wrong_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let wrong_ca = wrong_params.self_signed(&wrong_key).unwrap();
        private(&p.join("wrong.pem"), wrong_ca.pem().as_bytes());
        private(&p.join("wrong.key"), wrong_key.serialize_pem().as_bytes());
        let state = p.join("manager");
        let mut manager = launch(
            &manager_binary(),
            &[
                "--state-dir",
                state.to_str().unwrap(),
                "--listen",
                "127.0.0.1:0",
                "--setup-listen",
                "127.0.0.1:0",
                "--worker-listen",
                "127.0.0.1:0",
                "--worker-tls-cert",
                p.join("server.pem").to_str().unwrap(),
                "--worker-tls-key",
                p.join("server.key").to_str().unwrap(),
                "--worker-client-ca",
                p.join("ca.pem").to_str().unwrap(),
            ],
            p,
            "manager",
        );
        let addresses = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                assert!(manager.0.try_wait().unwrap().is_none(), "manager running");
                if let Ok(bytes) = fs::read(state.join("listeners.json")) {
                    break serde_json::from_slice::<Value>(&bytes).unwrap();
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        let listen = addresses["listen"].as_str().unwrap().parse().unwrap();
        let setup = addresses["setup"].as_str().unwrap().parse().unwrap();
        let bootstrap = fs::read_to_string(state.join("bootstrap.token")).unwrap();
        assert_eq!(local(setup, "POST", "/v1/owner", Some(&bootstrap), json!({"name":"Test owner", "password":"synthetic transport password", "vault_passphrase":"synthetic transport passphrase", "acknowledge_no_escrow":true})).await.0, 201);
        let (status, session) = local(
            listen,
            "POST",
            "/v1/sessions",
            None,
            json!({"password":"synthetic transport password"}),
        )
        .await;
        assert_eq!(status, 201);
        let address: SocketAddr = addresses["worker"].as_str().unwrap().parse().unwrap();
        let config = |name: &str| Config {
            manager: format!("wss://localhost:{}", address.port()),
            manager_ca: p.join("ca.pem"),
            client_cert: p.join(format!("{name}.pem")),
            client_key: p.join(format!("{name}.key")),
        };
        let worker = config("worker");
        let other = config("other");
        let wrong = config("wrong");
        let expired = config("expired");
        Self {
            root,
            manager,
            listen,
            token: session["token"].as_str().unwrap().into(),
            worker,
            other,
            wrong,
            expired,
        }
    }
    async fn owner(&self, method: &str, path: &str, value: Value) -> (u16, Value) {
        local(self.listen, method, path, Some(&self.token), value).await
    }
    async fn invitation(&self, config: &Config, seconds: u32) -> Value {
        let fingerprint = CertificateFingerprint::from_der(
            tls::certificates(&config.client_cert).unwrap()[0].as_ref(),
        );
        let (status, invitation) = self
            .owner(
                "POST",
                "/v1/worker-invitations",
                json!({"fingerprint":fingerprint,"lifetime_seconds":seconds}),
            )
            .await;
        assert_eq!(status, 201);
        invitation
    }
    async fn wait_phase(&self, phase: &str, minimum_generation: u64) -> Value {
        let fingerprint = CertificateFingerprint::from_der(
            tls::certificates(&self.worker.client_cert).unwrap()[0].as_ref(),
        );
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let workers = self.owner("GET", "/v1/workers", json!({})).await.1;
                if let Some(worker) = workers.as_array().unwrap().iter().find(|w| {
                    w["fingerprint"] == fingerprint.as_str()
                        && w["phase"] == phase
                        && w["generation"].as_u64().unwrap() >= minimum_generation
                }) {
                    break worker.clone();
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("worker phase deadline")
    }
}

impl Fixture {
    async fn create_session(&self, worker: WorkerId, name: &str) -> ComputerSession {
        let bot = BotId::new();
        assert_eq!(
            self.owner("PUT", &format!("/v1/bots/{bot}"), json!({"name":name}))
                .await
                .0,
            200
        );
        let (status, session) = self
            .owner(
                "POST",
                "/v1/computer-sessions",
                json!({"bot_id":bot,"worker_id":worker}),
            )
            .await;
        assert_eq!(status, 200);
        serde_json::from_value(session).unwrap()
    }
    async fn confirmed(&self, id: ComputerSessionId) -> ComputerSession {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let session: ComputerSession = serde_json::from_value(
                    self.owner("GET", &format!("/v1/computer-sessions/{id}"), json!({}))
                        .await
                        .1,
                )
                .unwrap();
                if session.status == ControlStatus::Confirmed {
                    break session;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("confirmed ownership")
    }
    async fn command(
        &self,
        session: &ComputerSession,
        generation: Generation,
        task_id: TaskId,
        action: CommandAction,
    ) -> botstral_link::Command {
        let command = botstral_link::Command {
            id: CommandId::new(),
            task_id,
            fence: session.fence(generation),
            action,
        };
        let (status, receipt) = self
            .owner(
                "POST",
                "/v1/worker-commands",
                serde_json::to_value(&command).unwrap(),
            )
            .await;
        assert_eq!(status, 200, "dispatch {receipt}");
        command
    }
    async fn receipt(
        &self,
        worker: WorkerId,
        id: CommandId,
        predicate: impl Fn(&Value) -> bool,
    ) -> Value {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let (status, receipt) = self
                    .owner(
                        "GET",
                        &format!("/v1/workers/{worker}/receipts/{id}"),
                        json!({}),
                    )
                    .await;
                assert_eq!(status, 200);
                if predicate(&receipt) {
                    break receipt;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("receipt deadline")
    }
    async fn takeover(&self, session: &ComputerSession, owner: InputOwner) -> ComputerSession {
        let (status, pending) = self
            .owner(
                "PUT",
                &format!("/v1/computer-sessions/{}/owner", session.id),
                json!({"owner":owner,"expected_revision":session.revision}),
            )
            .await;
        assert_eq!(status, 200);
        assert_eq!(pending["status"], "pending");
        let confirmed = self.confirmed(session.id).await;
        assert!(confirmed.revision > session.revision);
        assert_eq!(confirmed.owner, owner);
        confirmed
    }
}

#[path = "m02_transport/enrollment.rs"]
mod enrollment;

#[cfg(feature = "nonproduction-test-worker")]
#[path = "m02_transport/sessions.rs"]
mod sessions;

#[cfg(feature = "nonproduction-test-worker")]
#[path = "m02_transport/fencing.rs"]
mod fencing;
