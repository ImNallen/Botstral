use botstral_desktop::local::{Connection, ErrorCode, LocalManager, SessionView};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::{mpsc, oneshot},
    task::{JoinHandle, JoinSet},
};
use uuid::Uuid;
use zeroize::Zeroizing;

const PASSWORD: &str = "SYNTHETIC desktop password";
const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ID: &str = "00000000-0000-4000-8000-000000000001";

struct Request {
    line: String,
    headers: String,
    body: Value,
    reply: oneshot::Sender<(u16, String)>,
}
struct Server {
    endpoint: String,
    requests: mpsc::Receiver<Request>,
    task: JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = listener.local_addr().unwrap().to_string();
        let (send, requests) = mpsc::channel(16);
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (mut socket, _) = accepted.unwrap();
                        let send = send.clone();
                        connections.spawn(async move {
                            let mut bytes = Vec::new();
                            let end = loop {
                                let mut chunk = [0; 1024];
                                let count = socket.read(&mut chunk).await.unwrap();
                                if count == 0 { return; }
                                bytes.extend_from_slice(&chunk[..count]);
                                if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") { break end + 4; }
                                assert!(bytes.len() < 16384);
                            };
                            let headers = String::from_utf8(bytes[..end].to_vec()).unwrap();
                            let length = headers.lines().find_map(|line| line.to_lowercase().strip_prefix("content-length: ").map(|value| value.parse::<usize>().unwrap())).unwrap_or(0);
                            while bytes.len() < end + length {
                                let mut chunk = [0; 1024];
                                let count = socket.read(&mut chunk).await.unwrap();
                                assert!(count > 0);
                                bytes.extend_from_slice(&chunk[..count]);
                            }
                            let body = if length == 0 { Value::Null } else { serde_json::from_slice(&bytes[end..end+length]).unwrap() };
                            let (reply, receive) = oneshot::channel();
                            send.send(Request { line: headers.lines().next().unwrap().to_owned(), headers, body, reply }).await.unwrap();
                            if let Ok((status, body)) = receive.await {
                                let response = format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", if status == 302 { format!("Location: {body}\r\n") } else { String::new() }, body.len());
                                let _ = socket.write_all(response.as_bytes()).await;
                            }
                        });
                    },
                    _ = connections.join_next(), if !connections.is_empty() => {},
                }
            }
        });
        Self {
            endpoint,
            requests,
            task,
        }
    }
    async fn next(&mut self, method: &str, path: &str) -> Request {
        let request = tokio::time::timeout(Duration::from_secs(5), self.requests.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(request.line, format!("{method} {path} HTTP/1.1"));
        assert!(!request.headers.to_lowercase().contains("origin:"));
        assert!(!request.headers.to_lowercase().contains("sec-fetch"));
        request
    }
    async fn reply(&mut self, method: &str, path: &str, status: u16, body: Value) {
        self.next(method, path)
            .await
            .reply
            .send((status, body.to_string()))
            .unwrap();
    }
    async fn sign_in(&mut self, bots: Value) {
        let request = self.next("POST", "/v1/sessions").await;
        assert_eq!(request.body, json!({"password": PASSWORD}));
        request.reply.send((201, json!({"token":TOKEN,"session":{"id":ID,"created_at":100,"expires_at":10000000000_i64}}).to_string())).unwrap();
        self.snapshot(bots).await;
    }
    async fn snapshot(&mut self, bots: Value) {
        self.reply("GET", "/v1/owner", 200, json!({"name":"Synthetic owner"}))
            .await;
        self.reply("GET", "/v1/bots", 200, bots).await;
        self.reply("GET", "/v1/status", 200, json!({"vault":"locked","provider":null,"worker":"not_implemented","inference":"single_turn_text","remote":"disabled"})).await;
        self.reply("GET", "/v1/workers", 200, json!([])).await;
    }
}

fn spawn_sign_in(
    service: &LocalManager,
    endpoint: &str,
) -> JoinHandle<Result<botstral_desktop::local::Snapshot, botstral_desktop::local::Failure>> {
    let service = service.clone();
    let endpoint = endpoint.to_owned();
    tokio::spawn(async move {
        service
            .sign_in(endpoint, Zeroizing::new(PASSWORD.into()))
            .await
    })
}

#[tokio::test]
async fn late_grant_is_revoked_after_immediate_sign_out() {
    let mut server = Server::start().await;
    let service = LocalManager::new().unwrap();
    let signing_in = spawn_sign_in(&service, &server.endpoint);
    let grant_request = server.next("POST", "/v1/sessions").await;
    let signed_out = service.sign_out();
    assert!(matches!(signed_out.session, SessionView::SignedOut { .. }));
    assert_eq!(service.inspect().revision, signed_out.revision);
    grant_request
        .reply
        .send((
            201,
            json!({"token":TOKEN,"session":{"id":ID,"created_at":100,"expires_at":200}})
                .to_string(),
        ))
        .unwrap();
    server
        .reply("DELETE", "/v1/session", 204, Value::Null)
        .await;
    assert_eq!(
        signing_in.await.unwrap().unwrap_err().code,
        ErrorCode::Stale
    );
    let output = serde_json::to_string(&service.inspect()).unwrap();
    assert!(!output.contains(TOKEN));
    assert!(!output.contains(PASSWORD));
    assert!(matches!(
        service.inspect().session,
        SessionView::SignedOut { .. }
    ));
}

#[tokio::test]
async fn sign_out_fences_queued_writes_and_old_unauthorized_results() {
    let mut server = Server::start().await;
    let service = LocalManager::new().unwrap();
    let signing_in = spawn_sign_in(&service, &server.endpoint);
    server.sign_in(json!([])).await;
    signing_in.await.unwrap().unwrap();
    let refreshing = {
        let service = service.clone();
        tokio::spawn(async move { service.refresh().await })
    };
    let old_owner = server.next("GET", "/v1/owner").await;
    let mut queued = Vec::new();
    for _ in 0..4 {
        let service = service.clone();
        queued.push(tokio::spawn(async move {
            service
                .create_bot(Uuid::new_v4(), "Queued bot".into())
                .await
        }));
    }
    tokio::task::yield_now().await;
    assert_eq!(service.refresh().await.unwrap_err().code, ErrorCode::Busy);
    let signed_out = service.sign_out();
    let signing_in = spawn_sign_in(&service, &server.endpoint);
    tokio::task::yield_now().await;
    old_owner
        .reply
        .send((401, "SYNTHETIC private response".into()))
        .unwrap();
    assert_eq!(
        refreshing.await.unwrap().unwrap_err().code,
        ErrorCode::Stale
    );
    for queued in queued {
        assert_eq!(queued.await.unwrap().unwrap_err().code, ErrorCode::Stale);
    }
    let result = signing_in.await.unwrap().unwrap_err();
    assert_eq!(result.code, ErrorCode::Busy);
    assert!(result.snapshot.generation >= signed_out.generation);
    let signing_in = spawn_sign_in(&service, &server.endpoint);
    server.sign_in(json!([])).await;
    let current = signing_in.await.unwrap().unwrap();
    assert!(matches!(current.session, SessionView::SignedIn { .. }));
    assert!(current.generation > signed_out.generation);
    assert!(server.requests.try_recv().is_err());
}

#[tokio::test]
async fn an_old_unauthorized_response_cannot_clear_a_new_sign_in() {
    let mut server = Server::start().await;
    let service = LocalManager::new().unwrap();
    let signing_in = spawn_sign_in(&service, &server.endpoint);
    server.sign_in(json!([])).await;
    signing_in.await.unwrap().unwrap();
    let refreshing = {
        let service = service.clone();
        tokio::spawn(async move { service.refresh().await })
    };
    let old_owner = server.next("GET", "/v1/owner").await;
    let out = service.sign_out();
    let signing_in = spawn_sign_in(&service, &server.endpoint);
    tokio::time::timeout(Duration::from_secs(1), async {
        while service.inspect().generation == out.generation {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let newer_generation = service.inspect().generation;
    old_owner
        .reply
        .send((401, "SYNTHETIC discarded response".into()))
        .unwrap();
    assert_eq!(
        refreshing.await.unwrap().unwrap_err().code,
        ErrorCode::Stale
    );
    assert_eq!(service.inspect().generation, newer_generation);
    server
        .reply("DELETE", "/v1/session", 204, Value::Null)
        .await;
    server.sign_in(json!([])).await;
    let current = signing_in.await.unwrap().unwrap();
    assert_eq!(current.generation, newer_generation);
    assert!(matches!(current.session, SessionView::SignedIn { .. }));
}

#[tokio::test]
async fn creation_can_retry_the_same_uuid_and_name_after_refresh_failure() {
    let mut server = Server::start().await;
    let service = LocalManager::new().unwrap();
    let signing_in = spawn_sign_in(&service, &server.endpoint);
    server.sign_in(json!([])).await;
    signing_in.await.unwrap().unwrap();
    let id = Uuid::parse_str(ID).unwrap();
    let create = {
        let service = service.clone();
        tokio::spawn(async move { service.create_bot(id, "Research".into()).await })
    };
    let request = server.next("PUT", &format!("/v1/bots/{id}")).await;
    assert_eq!(request.body, json!({"name":"Research"}));
    let bot = json!({"id":id,"name":"Research","created_at":101});
    request.reply.send((200, bot.to_string())).unwrap();
    server
        .reply(
            "GET",
            "/v1/owner",
            500,
            json!({"private":"must never escape"}),
        )
        .await;
    let error = create.await.unwrap().unwrap_err();
    assert_eq!(error.code, ErrorCode::ManagerError);
    assert!(
        !serde_json::to_string(&error)
            .unwrap()
            .contains("must never escape")
    );
    let retry = {
        let service = service.clone();
        tokio::spawn(async move { service.create_bot(id, "Research".into()).await })
    };
    let request = server.next("PUT", &format!("/v1/bots/{id}")).await;
    assert_eq!(request.body, json!({"name":"Research"}));
    request.reply.send((200, bot.to_string())).unwrap();
    server.snapshot(json!([bot])).await;
    let snapshot = retry.await.unwrap().unwrap();
    assert!(matches!(snapshot.session, SessionView::SignedIn { bots, .. } if bots.len() == 1));
}

#[tokio::test]
async fn transport_rejects_redirects_oversize_and_untrusted_addresses() {
    let service = LocalManager::new().unwrap();
    for endpoint in [
        "localhost:43120",
        "https://127.0.0.1:43120",
        "192.0.2.1:43120",
        "127.0.0.1:0",
        "[::]:43120",
    ] {
        assert_eq!(
            service
                .sign_in(endpoint.into(), Zeroizing::new(PASSWORD.into()))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput
        );
    }
    let mut server = Server::start().await;
    let signing_in = spawn_sign_in(&service, &server.endpoint);
    let mut redirect_target = Server::start().await;
    server
        .next("POST", "/v1/sessions")
        .await
        .reply
        .send((
            302,
            format!("http://{}/credential-target", redirect_target.endpoint),
        ))
        .unwrap();
    assert_eq!(
        signing_in.await.unwrap().unwrap_err().code,
        ErrorCode::ManagerError
    );
    assert!(server.requests.try_recv().is_err());
    assert!(redirect_target.requests.try_recv().is_err());
    let signing_in = spawn_sign_in(&service, &server.endpoint);
    server
        .next("POST", "/v1/sessions")
        .await
        .reply
        .send((201, "x".repeat(1024 * 1024 + 1)))
        .unwrap();
    assert_eq!(
        signing_in.await.unwrap().unwrap_err().code,
        ErrorCode::Protocol
    );
    let signing_in = spawn_sign_in(&service, &server.endpoint);
    server
        .reply(
            "POST",
            "/v1/sessions",
            401,
            json!({"error":"SYNTHETIC private body"}),
        )
        .await;
    let failure = signing_in.await.unwrap().unwrap_err();
    assert_eq!(failure.code, ErrorCode::InvalidCredentials);
    assert!(
        !serde_json::to_string(&failure)
            .unwrap()
            .contains("SYNTHETIC")
    );
    let signing_in = spawn_sign_in(&service, &server.endpoint);
    let _held = server.next("POST", "/v1/sessions").await;
    assert_eq!(
        signing_in.await.unwrap().unwrap_err().code,
        ErrorCode::Timeout
    );
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
#[ignore = "Build the manager, then run with --ignored --test-threads=1."]
async fn actual_manager_owner_bot_workers_and_sign_out() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let binary = root.join("target/debug/botstral-manager");
    assert!(binary.is_file(), "Build the real manager first.");
    let temp = tempfile::tempdir().unwrap();
    let state_path = temp.path().join("state");
    let mut child = ChildGuard(
        Command::new(binary)
            .args([
                "--listen",
                "127.0.0.1:0",
                "--setup-listen",
                "127.0.0.1:0",
                "--state-dir",
            ])
            .arg(&state_path)
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let addresses: Value = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            assert!(child.0.try_wait().unwrap().is_none());
            if let Ok(bytes) = std::fs::read(state_path.join("listeners.json")) {
                break serde_json::from_slice(&bytes).unwrap();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let bootstrap = std::fs::read_to_string(state_path.join("bootstrap.token")).unwrap();
    let setup = client.post(format!("http://{}/v1/owner", addresses["setup"].as_str().unwrap())).bearer_auth(bootstrap)
        .json(&json!({"name":"Synthetic owner","password":PASSWORD,"vault_passphrase":"SYNTHETIC separate vault phrase","acknowledge_no_escrow":true})).send().await.unwrap();
    assert_eq!(setup.status(), 201);
    let service = LocalManager::new().unwrap();
    let endpoint = addresses["listen"].as_str().unwrap().to_owned();
    let snapshot = service
        .sign_in(endpoint, Zeroizing::new(PASSWORD.into()))
        .await
        .unwrap();
    assert_eq!(snapshot.connection, Connection::Online);
    assert!(
        matches!(snapshot.session, SessionView::SignedIn { owner, bots, details, .. } if owner.name == "Synthetic owner" && bots.is_empty() && details.workers.is_empty())
    );
    let id = Uuid::new_v4();
    service.create_bot(id, "Research".into()).await.unwrap();
    let snapshot = service.create_bot(id, "Research".into()).await.unwrap();
    assert!(
        matches!(snapshot.session, SessionView::SignedIn { bots, .. } if bots.len() == 1 && bots[0].id == id && bots[0].name == "Research")
    );
    let observer: Value = client
        .post(format!(
            "http://{}/v1/sessions",
            addresses["listen"].as_str().unwrap()
        ))
        .json(&json!({"password":PASSWORD}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let observer_token = observer["token"].as_str().unwrap();
    let before = service.inspect();
    let out = service.sign_out();
    assert!(out.revision > before.revision && out.generation > before.generation);
    assert!(matches!(out.session, SessionView::SignedOut { .. }));
    assert_eq!(
        service
            .create_bot(Uuid::new_v4(), "Denied".into())
            .await
            .unwrap_err()
            .code,
        ErrorCode::Unauthorized
    );
    assert!(matches!(
        service.refresh().await.unwrap().session,
        SessionView::SignedOut { .. }
    ));
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let sessions: Vec<Value> = client
                .get(format!(
                    "http://{}/v1/sessions",
                    addresses["listen"].as_str().unwrap()
                ))
                .bearer_auth(observer_token)
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            if sessions.len() == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    println!(
        "Verified real manager sign-in, owner, idempotent bot creation, status, workers, and local sign-out through the production native service."
    );
}
