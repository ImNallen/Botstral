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

#[tokio::test]
async fn enrollment_tls_negotiation_and_lifecycle_use_actual_binaries() {
    let mut f = Fixture::new().await;
    let grant = f.invitation(&f.worker, 600).await;
    assert_eq!(
        worker_http(
            &f.other,
            "POST",
            "/worker/v1/enroll",
            json!({"invitation":grant["invitation"]})
        )
        .await
        .1["error"],
        "identity_mismatch"
    );
    let expired = f.invitation(&f.other, 1).await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(
        worker_http(
            &f.other,
            "POST",
            "/worker/v1/enroll",
            json!({"invitation":expired["invitation"]})
        )
        .await
        .1["error"],
        "invitation_expired"
    );
    let invitation_file = f.root.path().join("invitation.json");
    private(&invitation_file, &serde_json::to_vec(&grant).unwrap());
    let agent_state = f.root.path().join("agent");
    let output = Command::new(agent_binary())
        .args([
            "enroll",
            "--state-dir",
            agent_state.to_str().unwrap(),
            "--manager",
            &f.worker.manager,
            "--manager-ca",
            f.worker.manager_ca.to_str().unwrap(),
            "--client-cert",
            f.worker.client_cert.to_str().unwrap(),
            "--client-key",
            f.worker.client_key.to_str().unwrap(),
            "--invitation-file",
            invitation_file.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "actual enrollment: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        worker_http(
            &f.worker,
            "POST",
            "/worker/v1/enroll",
            json!({"invitation":grant["invitation"]})
        )
        .await
        .1["error"],
        "invitation_used"
    );
    for config in [&f.wrong, &f.expired] {
        assert!(transport::connect_socket(config).await.is_err());
    }
    let mut bad = f.worker.clone();
    bad.manager_ca = f.root.path().join("wrong.pem");
    assert!(transport::connect_socket(&bad).await.is_err());
    bad = f.worker.clone();
    bad.manager = bad.manager.replace("localhost", "127.0.0.1");
    assert!(transport::connect_socket(&bad).await.is_err());
    for (name, config) in [
        ("wrong-host", bad.clone()),
        (
            "wrong-ca",
            Config {
                manager_ca: f.root.path().join("wrong.pem"),
                ..f.worker.clone()
            },
        ),
    ] {
        let state = f.root.path().join(name);
        fs::create_dir(&state).unwrap();
        fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
        private(
            &state.join("config.json"),
            &serde_json::to_vec(&config).unwrap(),
        );
        let result = Command::new(agent_binary())
            .args(["check", "--state-dir", state.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(!result.status.success(), "actual daemon rejects {name}");
        assert_eq!(
            String::from_utf8_lossy(&result.stderr),
            "worker operation failed\n"
        );
    }
    for path in [
        "/v1/owner",
        "/v1/sessions",
        "/v1/bots",
        "/v1/status",
        "/v1/vault/unlock",
        "/v1/vault/lock",
        "/v1/provider-keys/mistral",
        "/v1/worker-invitations",
        "/v1/devices",
        "/v1/approvals",
        "/v1/security",
    ] {
        for method in ["GET", "POST", "PUT", "DELETE"] {
            assert_eq!(
                worker_http(&f.worker, method, path, json!({})).await.0,
                404,
                "worker route {method} {path}"
            );
        }
    }
    assert_eq!(
        worker_http(
            &f.worker,
            "POST",
            "/worker/v1/enroll",
            json!({"invitation":f.token})
        )
        .await
        .1["error"],
        "invalid_invitation"
    );
    assert_eq!(
        local(
            f.listen,
            "GET",
            "/v1/owner",
            Some(grant["invitation"].as_str().unwrap()),
            json!({})
        )
        .await
        .0,
        401
    );
    let mut capped = Vec::new();
    for _ in 0..16 {
        capped.push(transport::connect_socket(&f.worker).await.unwrap());
    }
    assert!(
        transport::connect_socket(&f.worker).await.is_err(),
        "upgraded sockets retain all 16 permits"
    );
    drop(capped);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut silent = transport::connect_socket(&f.worker).await.unwrap();
    assert_eq!(
        read(&mut silent).await,
        ManagerMessage::Error {
            code: ErrorCode::WrongPhase
        }
    );
    for message in [
        Message::Binary(vec![1, 2, 3].into()),
        Message::Text("{".into()),
        Message::Text("x".repeat(MAX_MESSAGE + 1).into()),
    ] {
        let mut socket = transport::connect_socket(&f.worker).await.unwrap();
        let _ = socket.send(message).await;
        let response = tokio::time::timeout(Duration::from_secs(6), socket.next())
            .await
            .unwrap();
        if let Some(Ok(Message::Text(text))) = response {
            assert_eq!(
                decode::<ManagerMessage>(text.as_bytes()).unwrap().message,
                ManagerMessage::Error {
                    code: ErrorCode::InvalidMessage
                }
            );
        }
    }
    let mut unenrolled = transport::connect_socket(&f.other).await.unwrap();
    wire(&mut unenrolled, WorkerMessage::Hello(Hello::current(false))).await;
    assert_eq!(
        read(&mut unenrolled).await,
        ManagerMessage::Error {
            code: ErrorCode::Unauthorized
        }
    );
    for (hello, expected) in [
        (
            Hello {
                versions: vec![99],
                ..Hello::current(false)
            },
            ErrorCode::IncompatibleVersion,
        ),
        (
            Hello {
                capabilities: vec![],
                ..Hello::current(false)
            },
            ErrorCode::MissingCapability,
        ),
        (
            Hello {
                required: vec![Capability::TestInput],
                ..Hello::current(false)
            },
            ErrorCode::MissingCapability,
        ),
        (
            Hello {
                channel: ReleaseChannel::Experimental,
                ..Hello::current(false)
            },
            ErrorCode::IncompatibleChannel,
        ),
    ] {
        if cfg!(feature = "nonproduction-test-worker")
            && hello.required.contains(&Capability::TestInput)
        {
            continue;
        }
        let mut socket = transport::connect_socket(&f.worker).await.unwrap();
        wire(&mut socket, WorkerMessage::Hello(hello)).await;
        assert_eq!(
            read(&mut socket).await,
            ManagerMessage::Error { code: expected }
        );
    }
    let mut early = transport::connect_socket(&f.worker).await.unwrap();
    wire(
        &mut early,
        WorkerMessage::Heartbeat {
            generation: Generation::INITIAL,
        },
    )
    .await;
    assert_eq!(
        read(&mut early).await,
        ManagerMessage::Error {
            code: ErrorCode::WrongPhase
        }
    );
    for method in [
        "approve",
        "enroll_owner_device",
        "get_credential",
        "change_security_policy",
    ] {
        let mut socket = transport::connect_socket(&f.worker).await.unwrap();
        let mut value =
            serde_json::to_value(Envelope::new(WorkerMessage::Hello(Hello::current(false))))
                .unwrap();
        value["message"] = json!({"type":method});
        socket
            .send(Message::Text(value.to_string().into()))
            .await
            .unwrap();
        assert_eq!(
            read(&mut socket).await,
            ManagerMessage::Error {
                code: ErrorCode::InvalidMessage
            }
        );
    }
    let fingerprint = CertificateFingerprint::from_der(
        tls::certificates(&f.worker.client_cert).unwrap()[0].as_ref(),
    );
    let workers = f.owner("GET", "/v1/workers", json!({})).await.1;
    assert_eq!(
        workers
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["fingerprint"] == fingerprint.as_str())
            .unwrap()["generation"],
        0
    );
    let race = Config {
        client_cert: f.root.path().join("race.pem"),
        client_key: f.root.path().join("race.key"),
        ..f.worker.clone()
    };
    let invitation = f.invitation(&race, 600).await;
    let body = json!({"invitation": invitation["invitation"]});
    let (one, two) = tokio::join!(
        worker_http(&race, "POST", "/worker/v1/enroll", body.clone()),
        worker_http(&race, "POST", "/worker/v1/enroll", body)
    );
    assert!(
        (one.0 == 201 && two.1["error"] == "invitation_used")
            || (two.0 == 201 && one.1["error"] == "invitation_used")
    );
    let recovery = Config {
        client_cert: f.root.path().join("recovery.pem"),
        client_key: f.root.path().join("recovery.key"),
        ..f.worker.clone()
    };
    let invitation = f.invitation(&recovery, 600).await;
    let body = serde_json::to_vec(&json!({"invitation":invitation["invitation"]})).unwrap();
    let mut lost = transport::connect_tls(&recovery).await.unwrap();
    use tokio::io::AsyncWriteExt;
    lost.write_all(format!("POST /worker/v1/enroll HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await.unwrap();
    lost.write_all(&body).await.unwrap();
    lost.flush().await.unwrap();
    let fingerprint = CertificateFingerprint::from_der(
        tls::certificates(&recovery.client_cert).unwrap()[0].as_ref(),
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if f.owner("GET", "/v1/workers", json!({}))
                .await
                .1
                .as_array()
                .unwrap()
                .iter()
                .any(|w| w["fingerprint"] == fingerprint.as_str())
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    drop(lost);
    let mut recovered = transport::connect_socket(&recovery).await.unwrap();
    wire(&mut recovered, WorkerMessage::Hello(Hello::current(false))).await;
    let ManagerMessage::Welcome { generation, .. } = read(&mut recovered).await else {
        panic!("certificate recovers lost enrollment response");
    };
    wire(
        &mut recovered,
        WorkerMessage::SnapshotApplied { generation },
    )
    .await;
    drop(recovered);
    let mut daemon = launch(
        agent_binary(),
        &["run", "--state-dir", agent_state.to_str().unwrap()],
        f.root.path(),
        "agent",
    );
    let first = f.wait_phase("ready", 1).await;
    daemon.stop().await;
    f.wait_phase("disconnected", 1).await;
    let mut daemon = launch(
        agent_binary(),
        &["run", "--state-dir", agent_state.to_str().unwrap()],
        f.root.path(),
        "agent-reconnect",
    );
    let next = f
        .wait_phase("ready", first["generation"].as_u64().unwrap() + 1)
        .await;
    assert_eq!(next["id"], first["id"]);
    let worker: WorkerId = serde_json::from_value(next["id"].clone()).unwrap();
    let generation = Generation::try_from(next["generation"].as_u64().unwrap()).unwrap();
    let session = f.create_session(worker, "Production refusal").await;
    let session = f.confirmed(session.id).await;
    let unsupported = f
        .command(
            &session,
            generation,
            TaskId::new(),
            CommandAction::Start {
                work: WorkSpec::Unavailable,
            },
        )
        .await;
    f.receipt(worker, unsupported.id, |v| {
        v["acknowledgment"] == json!({"type":"rejected","code":"unsupported_execution"})
    })
    .await;
    let approved = f
        .command(
            &session,
            generation,
            TaskId::new(),
            CommandAction::ExecuteApproved {
                approval_id: ApprovalId::new(),
                operation_id: OperationId::new(),
                argument_digest: CertificateFingerprint::from_der(b"synthetic arguments"),
                expires_at: 9999999999,
            },
        )
        .await;
    f.receipt(worker, approved.id, |v| {
        v["acknowledgment"] == json!({"type":"rejected","code":"unsupported_execution"})
    })
    .await;
    let taken = f.takeover(&session, InputOwner::Owner).await;
    let input = CommandId::new();
    assert_eq!(
        f.owner(
            "POST",
            "/v1/worker-input",
            json!({"id":input,"fence":taken.fence(generation),"value":1})
        )
        .await
        .0,
        200
    );
    f.receipt(worker, input, |v| {
        v["acknowledgment"] == json!({"type":"rejected","code":"unsupported_execution"})
    })
    .await;
    assert_eq!(
        f.owner("DELETE", &format!("/v1/workers/{worker}"), json!({}))
            .await
            .0,
        204
    );
    f.wait_phase("disconnected", generation.get()).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if fs::read_to_string(f.root.path().join("agent-reconnect.log"))
                .unwrap()
                .contains("worker disconnected; retrying")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("revocation closes the live daemon socket");
    let mut revoked = transport::connect_socket(&f.worker).await.unwrap();
    wire(&mut revoked, WorkerMessage::Hello(Hello::current(false))).await;
    assert_eq!(
        read(&mut revoked).await,
        ManagerMessage::Error {
            code: ErrorCode::Unauthorized
        }
    );
    f.manager.stop().await;
    daemon.stop().await;
    for name in ["manager", "manager.log", "agent.log", "agent-reconnect.log"] {
        let path = f.root.path().join(name);
        let files: Vec<PathBuf> = if path.is_dir() {
            fs::read_dir(path)
                .unwrap()
                .map(|e| e.unwrap().path())
                .collect()
        } else {
            vec![path]
        };
        for path in files {
            let bytes = fs::read(&path).unwrap();
            for secret in [
                grant["invitation"].as_str().unwrap(),
                "PRIVATE KEY",
                "synthetic transport password",
                "synthetic transport passphrase",
            ] {
                assert!(
                    !bytes.windows(secret.len()).any(|w| w == secret.as_bytes()),
                    "secret absent in {}",
                    path.display()
                );
            }
        }
    }
    assert_eq!(
        fs::metadata(agent_state.join("config.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
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

#[cfg(feature = "nonproduction-test-worker")]
#[tokio::test]
async fn confirmed_takeover_two_sessions_cancellation_and_results() {
    let mut f = Fixture::new().await;
    let grant = f.invitation(&f.worker, 600).await;
    let enrolled = worker_http(
        &f.worker,
        "POST",
        "/worker/v1/enroll",
        json!({"invitation":grant["invitation"]}),
    )
    .await;
    assert_eq!(enrolled.0, 201);
    let worker: WorkerId = serde_json::from_value(enrolled.1["worker_id"].clone()).unwrap();
    let a = f.create_session(worker, "A").await;
    let b = f.create_session(worker, "B").await;
    let state = f.root.path().join("test-agent");
    fs::create_dir(&state).unwrap();
    fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
    private(
        &state.join("config.json"),
        &serde_json::to_vec(&f.worker).unwrap(),
    );
    let mut daemon = launch(
        Path::new(env!("CARGO_BIN_EXE_botstral-test-agent")),
        &["run", "--state-dir", state.to_str().unwrap()],
        f.root.path(),
        "test-agent",
    );
    let ready = f.wait_phase("ready", 1).await;
    let generation = Generation::try_from(ready["generation"].as_u64().unwrap()).unwrap();
    let a = f.confirmed(a.id).await;
    let b = f.confirmed(b.id).await;
    let start_a = f
        .command(
            &a,
            generation,
            TaskId::new(),
            CommandAction::Start {
                work: WorkSpec::TestInput { steps: 1000 },
            },
        )
        .await;
    let start_b = f
        .command(
            &b,
            generation,
            TaskId::new(),
            CommandAction::Start {
                work: WorkSpec::TestInput { steps: 1000 },
            },
        )
        .await;
    f.receipt(worker, start_a.id, |v| {
        v["last_event"]["applied"].as_u64().unwrap_or(0) >= 3
    })
    .await;
    f.receipt(worker, start_b.id, |v| {
        v["last_event"]["applied"].as_u64().unwrap_or(0) >= 3
    })
    .await;
    let owner_a = f.takeover(&a, InputOwner::Owner).await;
    let b_after = f.confirmed(b.id).await;
    assert_eq!(b_after, b);
    let a_status = f
        .command(&owner_a, generation, start_a.task_id, CommandAction::Status)
        .await;
    let stopped_a = f
        .receipt(worker, a_status.id, |v| {
            v["last_event"]["status"] == "suspended"
        })
        .await;
    let before_b = f.receipt(worker, start_b.id, |_| true).await["bot_inputs"]
        .as_u64()
        .unwrap();
    f.receipt(worker, start_b.id, |v| {
        v["bot_inputs"].as_u64().unwrap() > before_b + 2
    })
    .await;
    let a_status = f
        .command(&owner_a, generation, start_a.task_id, CommandAction::Status)
        .await;
    let still_stopped = f
        .receipt(worker, a_status.id, |v| {
            v["last_event"]["status"] == "suspended"
        })
        .await;
    assert_eq!(
        stopped_a["last_event"]["applied"],
        still_stopped["last_event"]["applied"]
    );
    let mut stale = a.fence(generation);
    stale.owner = InputOwner::Owner;
    let refusal = f
        .owner(
            "POST",
            "/v1/worker-input",
            json!({"id":CommandId::new(),"fence":stale,"value":1}),
        )
        .await;
    assert_eq!(refusal, (409, json!({"error":"stale_revision"})));
    let owner_input = CommandId::new();
    assert_eq!(
        f.owner(
            "POST",
            "/v1/worker-input",
            json!({"id":owner_input,"fence":owner_a.fence(generation),"value":1})
        )
        .await
        .0,
        200
    );
    f.receipt(worker, owner_input, |v| {
        v["acknowledgment"]["type"] == "applied"
    })
    .await;
    let bot_a = f.takeover(&owner_a, InputOwner::Bot).await;
    assert_eq!(
        f.owner(
            "POST",
            "/v1/worker-input",
            json!({"id":CommandId::new(),"fence":owner_a.fence(generation),"value":1})
        )
        .await
        .1["error"],
        "stale_revision"
    );
    let cancel = f
        .command(&b, generation, start_b.task_id, CommandAction::Cancel)
        .await;
    let cancelled = f
        .receipt(worker, cancel.id, |v| {
            v["last_event"]["type"] == "cancelled"
        })
        .await;
    assert!(cancelled["last_event"]["applied"].as_u64().unwrap() < 1000);
    let result = f
        .command(&b, generation, start_b.task_id, CommandAction::Result)
        .await;
    let result = f
        .receipt(worker, result.id, |v| {
            v["last_event"]["status"] == "cancelled"
        })
        .await;
    assert_eq!(
        result["last_event"]["applied"],
        cancelled["last_event"]["applied"]
    );
    let small = f
        .command(
            &bot_a,
            generation,
            TaskId::new(),
            CommandAction::Start {
                work: WorkSpec::TestInput { steps: 3 },
            },
        )
        .await;
    f.receipt(worker, small.id, |v| {
        v["last_event"] == json!({"type":"completed","applied":3})
    })
    .await;
    let result = f
        .command(&bot_a, generation, small.task_id, CommandAction::Result)
        .await;
    f.receipt(worker, result.id, |v| {
        v["last_event"] == json!({"type":"completed","applied":3})
    })
    .await;
    let same = f
        .owner(
            "POST",
            "/v1/worker-commands",
            serde_json::to_value(&small).unwrap(),
        )
        .await;
    assert_eq!(same.0, 200);
    let mut changed = small.clone();
    changed.action = CommandAction::Cancel;
    assert_eq!(
        f.owner(
            "POST",
            "/v1/worker-commands",
            serde_json::to_value(changed).unwrap()
        )
        .await
        .1["error"],
        "conflict"
    );
    daemon.stop().await;
    f.wait_phase("disconnected", generation.get()).await;
    let mut daemon = launch(
        Path::new(env!("CARGO_BIN_EXE_botstral-test-agent")),
        &["run", "--state-dir", state.to_str().unwrap()],
        f.root.path(),
        "test-agent-reconnect",
    );
    let ready = f.wait_phase("ready", generation.get() + 1).await;
    let new_generation = Generation::try_from(ready["generation"].as_u64().unwrap()).unwrap();
    let new_a = f.confirmed(a.id).await;
    assert!(new_a.revision > bot_a.revision);
    assert_eq!(
        f.owner(
            "POST",
            "/v1/worker-commands",
            serde_json::to_value(small.clone()).unwrap()
        )
        .await
        .1["error"],
        "stale_generation"
    );
    let missing = f
        .command(&new_a, new_generation, small.task_id, CommandAction::Result)
        .await;
    f.receipt(worker, missing.id, |v| {
        v["acknowledgment"] == json!({"type":"rejected","code":"unknown_task"})
    })
    .await;
    f.manager.stop().await;
    daemon.stop().await;
}

#[cfg(feature = "nonproduction-test-worker")]
#[tokio::test]
async fn actual_daemon_rejects_stale_frames_and_queued_receipts() {
    use std::sync::Arc;
    let mut f = Fixture::new().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
        Arc::new(tls::roots(&f.worker.manager_ca).unwrap()),
        provider.clone(),
    )
    .build()
    .unwrap();
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_client_cert_verifier(verifier)
        .with_single_cert(
            tls::certificates(&f.root.path().join("server.pem")).unwrap(),
            tls::private_key(&f.root.path().join("server.key")).unwrap(),
        )
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let state = f.root.path().join("probe-agent");
    fs::create_dir(&state).unwrap();
    fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
    let mut client = f.worker.clone();
    client.manager = format!("wss://localhost:{}", listener.local_addr().unwrap().port());
    private(
        &state.join("config.json"),
        &serde_json::to_vec(&client).unwrap(),
    );
    let mut daemon = launch(
        Path::new(env!("CARGO_BIN_EXE_botstral-test-agent")),
        &["run", "--state-dir", state.to_str().unwrap()],
        f.root.path(),
        "probe-agent",
    );
    let stream = tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .unwrap()
        .unwrap()
        .0;
    let stream = acceptor.accept(stream).await.unwrap();
    let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
    async fn send_probe(
        socket: &mut WebSocketStream<tokio_rustls::server::TlsStream<TcpStream>>,
        message: ManagerMessage,
    ) {
        socket
            .send(Message::Text(
                serde_json::to_string(&Envelope::new(message))
                    .unwrap()
                    .into(),
            ))
            .await
            .unwrap();
    }
    async fn report(
        socket: &mut WebSocketStream<tokio_rustls::server::TlsStream<TcpStream>>,
    ) -> Envelope<WorkerMessage> {
        loop {
            let Message::Text(text) = tokio::time::timeout(Duration::from_secs(5), socket.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap()
            else {
                panic!("text report")
            };
            let envelope = decode::<WorkerMessage>(text.as_bytes()).unwrap();
            if !matches!(envelope.message, WorkerMessage::Heartbeat { .. }) {
                return envelope;
            }
        }
    }
    assert!(matches!(
        report(&mut socket).await.message,
        WorkerMessage::Hello(_)
    ));
    let generation = Generation::INITIAL.advance().unwrap();
    let worker = WorkerId::new();
    let a = ComputerSession {
        id: ComputerSessionId::new(),
        bot_id: BotId::new(),
        worker_id: worker,
        owner: InputOwner::Bot,
        revision: Revision::INITIAL,
        status: ControlStatus::Pending,
    };
    send_probe(
        &mut socket,
        ManagerMessage::Welcome {
            worker_id: worker,
            generation,
            sessions: vec![a.clone()],
        },
    )
    .await;
    assert!(matches!(
        report(&mut socket).await.message,
        WorkerMessage::SnapshotApplied { .. }
    ));
    let start = botstral_link::Command {
        id: CommandId::new(),
        task_id: TaskId::new(),
        fence: a.fence(generation),
        action: CommandAction::Start {
            work: WorkSpec::TestInput { steps: 10 },
        },
    };
    send_probe(&mut socket, ManagerMessage::Command(start.clone())).await;
    let queued = loop {
        let report = report(&mut socket).await;
        if matches!(report.message, WorkerMessage::BotInput { .. }) {
            break report;
        }
    };
    let mut owned = a.clone();
    owned.owner = InputOwner::Owner;
    owned.revision = owned.revision.advance().unwrap();
    send_probe(
        &mut socket,
        ManagerMessage::Control {
            generation,
            session: owned.clone(),
        },
    )
    .await;
    assert!(matches!(
        report(&mut socket).await.message,
        WorkerMessage::ControlApplied { .. }
    ));
    socket
        .send(Message::Text(
            serde_json::to_string(&Envelope {
                version: VERSION,
                request_id: queued.request_id,
                message: ManagerMessage::Receipt {
                    command_id: start.id,
                    outcome: Acknowledgment::Accepted,
                },
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let status = botstral_link::Command {
        id: CommandId::new(),
        task_id: start.task_id,
        fence: owned.fence(generation),
        action: CommandAction::Status,
    };
    send_probe(&mut socket, ManagerMessage::Command(status)).await;
    assert!(matches!(
        report(&mut socket).await.message,
        WorkerMessage::Acknowledgment { .. }
    ));
    assert!(matches!(
        report(&mut socket).await.message,
        WorkerMessage::Event {
            event: TaskEvent::Status {
                status: TaskStatus::Suspended,
                applied: 0
            },
            ..
        }
    ));
    for (fence, code) in [
        (a.fence(generation), ErrorCode::StaleRevision),
        (owned.fence(Generation::INITIAL), ErrorCode::StaleGeneration),
    ] {
        send_probe(
            &mut socket,
            ManagerMessage::Input {
                command_id: CommandId::new(),
                fence,
                value: 1,
            },
        )
        .await;
        assert!(
            matches!(report(&mut socket).await.message, WorkerMessage::Acknowledgment { outcome: Acknowledgment::Rejected { code: received }, .. } if received == code)
        );
    }
    daemon.stop().await;
    f.manager.stop().await;
}
