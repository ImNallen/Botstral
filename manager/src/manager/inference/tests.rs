use super::*;
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State as HttpState},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};
use std::sync::{Mutex, atomic::AtomicUsize};
use tokio::net::TcpListener;

const KEY: &str = "synthetic-m04-provider-secret";
const PASSWORD: &str = "synthetic M04 login password";
const PASSPHRASE: &str = "synthetic M04 vault passphrase";
const MODEL: &str = include_str!("../../../tests/fixtures/m04/model.json");
const COMPLETION: &str = include_str!("../../../tests/fixtures/m04/completion.json");
const ERROR: &str = include_str!("../../../tests/fixtures/m04/error.json");

#[derive(Clone)]
struct Reply {
    status: StatusCode,
    body: String,
    gate: Option<Arc<Semaphore>>,
    interrupted: bool,
    chunked: bool,
}
impl Default for Reply {
    fn default() -> Self {
        Self {
            status: StatusCode::OK,
            body: COMPLETION.into(),
            gate: None,
            interrupted: false,
            chunked: false,
        }
    }
}
struct FixtureState {
    reply: Mutex<Reply>,
    model: Mutex<String>,
    chats: AtomicUsize,
    models: AtomicUsize,
    requests: Mutex<Vec<Value>>,
    coding_path: Mutex<Option<String>>,
    model_gate: Mutex<Option<Arc<Semaphore>>>,
    desktop: std::sync::atomic::AtomicBool,
    production: Mutex<Option<Value>>,
}
struct Fixture {
    state: Arc<FixtureState>,
    endpoint: String,
    task: JoinHandle<()>,
}
impl Fixture {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(FixtureState {
            reply: Mutex::new(Reply::default()),
            model: Mutex::new(MODEL.into()),
            chats: AtomicUsize::new(0),
            models: AtomicUsize::new(0),
            requests: Mutex::new(Vec::new()),
            coding_path: Mutex::new(None),
            model_gate: Mutex::new(None),
            desktop: std::sync::atomic::AtomicBool::new(false),
            production: Mutex::new(None),
        });
        let app = Router::new()
            .route(
                "/page",
                get(|| async {
                    axum::response::Html("<html><body>Recorded browser page</body></html>")
                }),
            )
            .route("/v1/models/{model}", get(model))
            .route("/v1/chat/completions", post(chat))
            .with_state(state.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            state,
            endpoint,
            task,
        }
    }
    fn reply(&self, reply: Reply) {
        *self.state.reply.lock().unwrap() = reply;
    }
    fn chats(&self) -> usize {
        self.state.chats.load(Ordering::SeqCst)
    }
    async fn reached(&self, n: usize) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while self.chats() < n {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("provider call reached fixture");
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn model(
    HttpState(state): HttpState<Arc<FixtureState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    assert_eq!(headers["authorization"], format!("Bearer {KEY}"));
    assert_eq!(id, "mistral-small-2506");
    state.models.fetch_add(1, Ordering::SeqCst);
    let gate = state.model_gate.lock().unwrap().clone();
    let _permit = match gate {
        Some(gate) => Some(gate.acquire_owned().await.unwrap()),
        None => None,
    };
    (
        [("content-type", "application/json")],
        state.model.lock().unwrap().clone(),
    )
        .into_response()
}
async fn chat(
    HttpState(state): HttpState<Arc<FixtureState>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    assert_eq!(headers["authorization"], format!("Bearer {KEY}"));
    assert_eq!(body["model"], "mistral-small-2506");
    assert!(matches!(
        body["messages"][0]["role"].as_str(),
        Some("user" | "system")
    ));
    assert_eq!(body["service_tier"], "standard_only");
    assert_eq!(body["stream"], false);
    assert_eq!(body["n"], 1);
    if body.get("tools").is_some() {
        assert_eq!(body["tool_choice"], "auto");
        assert_eq!(body["parallel_tool_calls"], false);
    } else if state.production.lock().unwrap().is_none() {
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        assert_eq!(body["tool_choice"], "none");
    }
    let coding_path = state.coding_path.lock().unwrap().clone();
    let mut reply = state.reply.lock().unwrap().clone();
    if let Some(path) = coding_path {
        let edited = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["role"] == "tool");
        let mut value: Value = serde_json::from_str(COMPLETION).unwrap();
        if !edited {
            value["choices"][0]["finish_reason"] = json!("tool_calls");
            value["choices"][0]["message"] = json!({"role":"assistant","content":null,"tool_calls":[{"id":"recorded-vibe-edit","type":"function","function":{"name":"edit","arguments":json!({"file_path":path,"old_string":"return 0","new_string":"return 42"}).to_string()}}]});
        }
        reply.body = value.to_string();
    }
    if state.desktop.load(Ordering::SeqCst) {
        let prompt = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "user")
            .and_then(|m| m["content"].as_str())
            .unwrap_or("")
            .to_lowercase();
        let tool_done = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["role"] == "tool");
        let mut value: Value = serde_json::from_str(COMPLETION).unwrap();
        if !tool_done
            && (prompt.contains("file") || prompt.contains("browser") || prompt.contains("cancel"))
        {
            let (name, arguments) = if prompt.contains("cancel") {
                (
                    "process",
                    json!({"executable":"/usr/bin/sleep","args":["30"],"cwd":"/tmp"}),
                )
            } else {
                (
                    "computer",
                    json!({"operations":[{"type":"write_file","path":"/tmp/c02-note.txt","text":"Synthetic C02 file"},{"type":"read_file","path":"/tmp/c02-note.txt","output":"text"}],"outputs":["text"]}),
                )
            };
            value["choices"][0]["finish_reason"] = json!("tool_calls");
            value["choices"][0]["message"] = json!({"role":"assistant","content":"The recorded provider proposes this bounded action. Review it before approval.","tool_calls":[{"id":"c02-recorded-tool","type":"function","function":{"name":name,"arguments":arguments.to_string()}}]});
        } else {
            value["choices"][0]["message"]["content"] = json!(if tool_done {
                "The manager received the synthetic worker result. The durable run is complete."
            } else {
                "This is a durable recorded assistant response from the real manager. No paid inference was used."
            });
        }
        reply.body = value.to_string();
    }
    if let Some(config) = state.production.lock().unwrap().as_ref() {
        reply.body = run_tests::production_fixture::response(&body, config);
    }
    state.requests.lock().unwrap().push(body);
    state.chats.fetch_add(1, Ordering::SeqCst);
    if let Some(gate) = reply.gate {
        let _ = gate.acquire().await.unwrap();
    }
    if reply.interrupted {
        return Response::builder()
            .body(Body::from_stream(futures_util::stream::once(async {
                Err::<String, _>(std::io::Error::other("fixture interrupted"))
            })))
            .unwrap();
    }
    let body = if reply.chunked {
        Body::from_stream(futures_util::stream::iter(
            reply
                .body
                .into_bytes()
                .chunks(1024)
                .map(|s| Ok::<_, std::io::Error>(s.to_vec()))
                .collect::<Vec<_>>(),
        ))
    } else {
        Body::from(reply.body)
    };
    Response::builder()
        .status(reply.status)
        .header("content-type", "application/json")
        .header("x-request-id", "fixture-http-request")
        .header("location", "/v1/chat/completions")
        .body(body)
        .unwrap()
}

struct App {
    manager: Manager,
    runtime: Runtime,
    address: String,
    token: String,
    bot: BotId,
    stop: watch::Sender<bool>,
    server: JoinHandle<std::io::Result<()>>,
}
impl App {
    async fn start(directory: PathBuf, fixture: &Fixture) -> Self {
        let runtime = Manager::start_with_provider(
            directory.clone(),
            Provider::fixture(fixture.endpoint.clone()),
        )
        .unwrap();
        if let Some(setup) = &runtime.setup {
            setup
                .create_owner(
                    BootstrapToken::new(
                        std::fs::read_to_string(directory.join("bootstrap.token")).unwrap(),
                    )
                    .unwrap(),
                    OwnerSetup {
                        name: OwnerName::new("M04 owner".into()).unwrap(),
                        login_password: LoginPassword::new(PASSWORD.into()).unwrap(),
                        vault_passphrase: VaultPassphrase::new(PASSPHRASE.into()).unwrap(),
                        recovery: RecoveryAcknowledgment::OfflinePassphraseCopy,
                    },
                )
                .await
                .unwrap();
        }
        let manager = runtime.manager.clone();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let (stop, stopped) = watch::channel(false);
        let server = tokio::spawn(crate::http::serve_listener(
            listener,
            crate::http::regular_router(manager.clone()),
            stopped,
            None,
        ));
        let mut app = Self {
            manager,
            runtime,
            address,
            token: String::new(),
            bot: BotId::new(),
            stop,
            server,
        };
        let grant = app
            .call("POST", "/v1/sessions", json!({"password":PASSWORD}), 201)
            .await;
        app.token = grant["token"].as_str().unwrap().into();
        app
    }
    async fn call(&self, method: &str, path: &str, body: Value, status: u16) -> Value {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap();
        let mut request = client
            .request(method.parse().unwrap(), format!("{}{path}", self.address))
            .header("content-type", "application/json");
        if !self.token.is_empty() {
            request = request.bearer_auth(&self.token);
        }
        if method != "GET" {
            request = request.body(serde_json::to_vec(&body).unwrap());
        }
        let response = request.send().await.unwrap();
        let actual = response.status().as_u16();
        let bytes = response.bytes().await.unwrap();
        assert!(
            !bytes.windows(KEY.len()).any(|w| w == KEY.as_bytes()),
            "credential leaked in client response"
        );
        assert_eq!(
            actual,
            status,
            "{method} {path}: {}",
            String::from_utf8_lossy(&bytes)
        );
        if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        }
    }
    async fn initialize(&self) {
        self.call(
            "PUT",
            &format!("/v1/bots/{}", self.bot),
            json!({"name":"Inference bot"}),
            200,
        )
        .await;
        self.call(
            "POST",
            "/v1/vault/unlock",
            json!({"passphrase":PASSPHRASE}),
            204,
        )
        .await;
        self.call(
            "PUT",
            "/v1/provider-keys/mistral",
            json!({"secret":KEY}),
            200,
        )
        .await;
        self.configure(0).await;
        self.verify(1, 200).await;
    }
    async fn configure(&self, expected_revision: u64) -> Value {
        self.call(
            "PUT",
            "/v1/inference-configurations",
            policy(self.bot, expected_revision),
            200,
        )
        .await
    }
    async fn verify(&self, revision: u64, status: u16) -> Value {
        self.call(
            "POST",
            "/v1/inference-configurations/verify",
            json!({"bot":self.bot,"revision":revision}),
            status,
        )
        .await
    }
    fn request(&self, revision: u64) -> SubmitInference {
        SubmitInference {
            id: InferenceId::new(),
            bot: self.bot,
            configuration_revision: revision,
            prompt: "Complete one fixture turn.".into(),
            deadline: now() + 110,
            max_output_tokens: 64,
            max_total_tokens: 32832,
            max_cost_microusd: 32896,
        }
    }
    async fn submit(&self, input: &SubmitInference, status: u16) -> Value {
        self.call("POST", "/v1/inferences", json!(input), status)
            .await
    }
    async fn view(&self, id: InferenceId) -> Value {
        self.call("GET", &format!("/v1/inferences/{id}"), Value::Null, 200)
            .await
    }
    async fn settled(&self, id: InferenceId) -> Value {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let view = self.view(id).await;
                if view["phase"]["type"] != "admitted" && view["phase"]["type"] != "sending" {
                    return view;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("inference reached a durable outcome")
    }
    async fn shutdown(self) {
        self.stop.send_replace(true);
        self.server.await.unwrap().unwrap();
        drop(self.manager);
        let Runtime {
            manager,
            setup,
            thread,
            inference,
        } = self.runtime;
        drop(manager);
        drop(setup);
        tokio::time::timeout(Duration::from_secs(3), inference.shutdown())
            .await
            .expect("bounded inference shutdown")
            .unwrap();
        tokio::task::spawn_blocking(move || thread.join().unwrap())
            .await
            .unwrap();
    }
    async fn crash(mut self) {
        if let Some(task) = self.runtime.inference.run_task.take() {
            task.abort();
            let _ = task.await;
        }
        self.stop.send_replace(true);
        self.server.await.unwrap().unwrap();
        if let Some(task) = self.runtime.inference.task.take() {
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
        }
        drop(self.manager);
        let Runtime {
            manager,
            setup,
            thread,
            inference,
        } = self.runtime;
        drop(inference);
        drop(manager);
        drop(setup);
        tokio::task::spawn_blocking(move || thread.join().unwrap())
            .await
            .unwrap();
    }
    fn session(&self) -> SessionToken {
        SessionToken::new(self.token.clone()).unwrap()
    }
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
fn policy(bot: BotId, expected_revision: u64) -> Value {
    json!({"bot":bot,"expected_revision":expected_revision,"policy":{
        "endpoint":"eu","model_id":"mistral-small-2506","context_tokens":32768,
        "acknowledgment":{"model_developer":"Mistral AI","developer_terms_url":"https://mistral.ai/terms/","privacy_terms_url":"https://mistral.ai/privacy/","acknowledged_at":now(),"release":{"type":"stable"}},
        "prices":{"input_microusd_per_million_tokens":1000000,"output_microusd_per_million_tokens":2000000,"effective_at":now()-1,"expires_at":now()+86400,"billing":"text_tokens_only_at_acknowledged_ceilings"}
    }})
}
fn private_local(root: &tempfile::TempDir) -> crate::LocalState {
    static PROTECT: std::sync::Once = std::sync::Once::new();
    PROTECT.call_once(|| crate::protect_process().unwrap());
    crate::LocalState::open(&root.path().join("state")).unwrap()
}
fn no_plaintext(directory: &std::path::Path) {
    for file in std::fs::read_dir(directory).unwrap() {
        let path = file.unwrap().path();
        if path.is_file() {
            let bytes = std::fs::read(&path).unwrap();
            assert!(
                !bytes.windows(KEY.len()).any(|w| w == KEY.as_bytes()),
                "plaintext credential in {}",
                path.display()
            );
        }
    }
}

#[tokio::test]
async fn m05_provider_verification_keeps_other_owner_operations_available() {
    let root = tempfile::tempdir().unwrap();
    let local = private_local(&root);
    let fixture = Fixture::start().await;
    let app = App::start(local.directory().into(), &fixture).await;
    app.initialize().await;
    let credential = crypto::random_token();
    let grant = app
        .manager
        .invite_device(
            app.session(),
            PairingRequest {
                fingerprint: DeviceFingerprint::from_credential(&credential),
                label: "Provider control".into(),
            },
            600,
        )
        .await
        .unwrap();
    let (_, first) = app
        .manager
        .connect_owner(
            OwnerHello::Enroll {
                version: OWNER_VERSION,
                manager: grant.manager,
                role: DeviceRole::Owner,
                credential: credential.clone(),
                invitation: grant.secret.expose_secret().into(),
            },
            watch::channel(false).0,
        )
        .await
        .unwrap();
    let (_, second) = app
        .manager
        .connect_owner(
            OwnerHello::Authenticate {
                version: OWNER_VERSION,
                manager: grant.manager,
                role: DeviceRole::Owner,
                credential,
            },
            watch::channel(false).0,
        )
        .await
        .unwrap();
    let gate = Arc::new(Semaphore::new(0));
    *fixture.state.model_gate.lock().unwrap() = Some(gate.clone());
    let manager = app.manager.clone();
    let input = VerifyInference {
        bot: app.bot,
        revision: 1,
    };
    let pending = tokio::spawn(async move {
        let permit = Arc::new(manager.snapshots.clone().acquire_owned().await.unwrap());
        manager
            .owner_operation(first, OwnerOperation::VerifyInference { input }, permit)
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), async {
        while fixture.state.models.load(Ordering::SeqCst) < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("Verification reached the blocked provider");
    let permit = Arc::new(
        app.manager
            .snapshots
            .clone()
            .try_acquire_owned()
            .expect("Provider verification blocked another owner operation"),
    );
    let reply = app
        .manager
        .owner_operation(
            second,
            OwnerOperation::Resume {
                cursor: 0,
                limit: 64,
            },
            permit,
        )
        .await
        .unwrap();
    assert!(matches!(reply.reply, OwnerReply::Replay(_)));
    drop(reply);
    gate.add_permits(1);
    let verified = tokio::time::timeout(Duration::from_secs(3), pending)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(matches!(verified.reply, OwnerReply::Configuration(_)));
    drop(verified);
    app.shutdown().await;
}

#[tokio::test]
async fn m04_documented_token_details_settle_and_reject_invalid_subsets() {
    let root = tempfile::tempdir().unwrap();
    let local = private_local(&root);
    let fixture = Fixture::start().await;
    let app = App::start(local.directory().into(), &fixture).await;
    app.initialize().await;
    for (cached, reasoning) in [(0, 0), (12, 8)] {
        let mut completion: Value = serde_json::from_str(COMPLETION).unwrap();
        completion["usage"]["prompt_tokens_details"] = json!({"cached_tokens":cached});
        completion["usage"]["completion_tokens_details"] = json!({"reasoning_tokens":reasoning});
        fixture.reply(Reply {
            body: completion.to_string(),
            ..Reply::default()
        });
        let input = app.request(1);
        app.submit(&input, 200).await;
        let view = app.settled(input.id).await;
        assert_eq!(view["phase"]["type"], "completed");
        assert_eq!(
            view["usage"],
            json!({"prompt_tokens":12,"completion_tokens":8,"total_tokens":20})
        );
        assert_eq!(
            view["accounting"],
            json!({"type":"settled","charged_microusd":28})
        );
    }
    for details in [
        json!({"prompt_tokens_details":{"cached_tokens":13}}),
        json!({"completion_tokens_details":{"reasoning_tokens":9}}),
        json!({"prompt_tokens_details":{"cached_tokens":0,"write_tokens":1}}),
        json!({"completion_tokens_details":{"reasoning_tokens":0,"audio_tokens":1}}),
        json!({"prompt_tokens_details":{"cached_tokens":-1}}),
        json!({"completion_tokens_details":{"reasoning_tokens":1.5}}),
    ] {
        let mut completion: Value = serde_json::from_str(COMPLETION).unwrap();
        for (key, value) in details.as_object().unwrap() {
            completion["usage"][key] = value.clone();
        }
        fixture.reply(Reply {
            body: completion.to_string(),
            ..Reply::default()
        });
        let input = app.request(1);
        app.submit(&input, 200).await;
        let view = app.settled(input.id).await;
        assert_eq!(view["phase"]["reason"], "invalid_usage");
        assert_eq!(view["accounting"]["type"], "held");
    }
    assert_eq!(fixture.chats(), 8);
    no_plaintext(local.directory());
    app.shutdown().await;
}

#[tokio::test]
async fn m04_http_success_idempotency_replay_and_restart() {
    let root = tempfile::tempdir().unwrap();
    let local = private_local(&root);
    let fixture = Fixture::start().await;
    let mut app = App::start(local.directory().into(), &fixture).await;
    let denied = reqwest::Client::new()
        .get(format!(
            "{}/v1/inferences/{}",
            app.address,
            InferenceId::new()
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 401);
    app.initialize().await;
    let token = std::mem::take(&mut app.token);
    for (method, path, body) in [
        (
            "PUT",
            "/v1/inference-configurations".into(),
            policy(app.bot, 1),
        ),
        (
            "GET",
            format!("/v1/inference-configurations/{}", app.bot),
            Value::Null,
        ),
        (
            "POST",
            "/v1/inference-configurations/verify".into(),
            json!({"bot":app.bot,"revision":1}),
        ),
        ("POST", "/v1/inferences".into(), json!(app.request(1))),
        (
            "POST",
            format!("/v1/inferences/{}/cancel", InferenceId::new()),
            json!({}),
        ),
    ] {
        app.call(method, &path, body, 401).await;
    }
    app.token = token;
    let input = app.request(1);
    app.submit(&input, 200).await;
    let view = app.settled(input.id).await;
    assert_eq!(view["phase"]["type"], "completed");
    assert_eq!(
        view["usage"],
        json!({"prompt_tokens":12,"completion_tokens":8,"total_tokens":20})
    );
    assert_eq!(
        view["accounting"],
        json!({"type":"settled","charged_microusd":28})
    );
    assert_eq!(
        view["reservation"],
        json!({"total_tokens":32832,"cost_microusd":32896})
    );
    assert_eq!(view["output"], "The fixture completed one text turn.");
    assert_eq!(view["provider_request_id"], "cmpl-fixture-m04");
    assert_eq!(app.submit(&input, 200).await, view);
    let mut changed = input.clone();
    changed.prompt.push('!');
    app.submit(&changed, 409).await;
    assert_eq!(fixture.chats(), 1);
    let snapshot = app.call("GET", "/v1/events/0/64", Value::Null, 200).await;
    assert_eq!(snapshot["state"]["inferences"][0], view);
    assert_eq!(
        snapshot["state"]["inference_configurations"][0]["revision"],
        1
    );
    let cursor = snapshot["cursor"].as_u64().unwrap();
    app.configure(1).await;
    let events = app
        .call("GET", &format!("/v1/events/{cursor}/64"), Value::Null, 200)
        .await;
    assert_eq!(events["entries"][0]["kind"], "inference_configured");
    assert_eq!(app.view(input.id).await["configuration"]["revision"], 1);
    no_plaintext(local.directory());
    app.shutdown().await;
    let app = App::start(local.directory().into(), &fixture).await;
    assert_eq!(app.view(input.id).await, view);
    assert_eq!(app.submit(&input, 200).await, view);
    assert_eq!(fixture.chats(), 1);
    app.shutdown().await;
    no_plaintext(local.directory());
}

#[tokio::test]
async fn m04_admission_configuration_vault_and_budget_refusals() {
    let root = tempfile::tempdir().unwrap();
    let local = private_local(&root);
    let fixture = Fixture::start().await;
    let app = App::start(local.directory().into(), &fixture).await;
    app.initialize().await;
    let input = app.request(1);
    for invalid in [
        json!({"max_total_tokens":1}),
        json!({"max_cost_microusd":1}),
        json!({"deadline":now()-1}),
        json!({"deadline":now()+3600}),
        json!({"max_output_tokens":0}),
        json!({"prompt":"x".repeat(MAX_INFERENCE_PROMPT_BYTES+1)}),
    ] {
        let mut request = json!(input);
        request
            .as_object_mut()
            .unwrap()
            .extend(invalid.as_object().unwrap().clone());
        let status = if invalid.get("max_total_tokens").is_some()
            || invalid.get("max_cost_microusd").is_some()
        {
            409
        } else {
            400
        };
        app.call("POST", "/v1/inferences", request, status).await;
    }
    app.call("POST", "/v1/vault/lock", json!({}), 204).await;
    assert_eq!(app.submit(&input, 409).await["error"], "vault_locked");
    app.verify(1, 409).await;
    app.call(
        "POST",
        "/v1/vault/unlock",
        json!({"passphrase":PASSPHRASE}),
        204,
    )
    .await;
    app.configure(1).await;
    assert_eq!(
        app.submit(&input, 409).await["error"],
        "inference_unverified"
    );
    let request = app.request(2);
    app.submit(&request, 409).await;
    for replacement in [
        json!({"id":"different-model"}),
        json!({"max_context_length":100}),
        json!({"capabilities":{"completion_chat":false}}),
        json!({"archived":true}),
    ] {
        let mut model: Value = serde_json::from_str(MODEL).unwrap();
        model
            .as_object_mut()
            .unwrap()
            .extend(replacement.as_object().unwrap().clone());
        *fixture.state.model.lock().unwrap() = model.to_string();
        assert_eq!(app.verify(2, 409).await["error"], "inference_unverified");
        app.submit(&request, 409).await;
    }
    *fixture.state.model.lock().unwrap() = MODEL.into();
    app.verify(2, 200).await;
    let mut invalid = policy(app.bot, 2);
    invalid["policy"]["endpoint"] = json!("http://127.0.0.1:9");
    app.call("PUT", "/v1/inference-configurations", invalid, 400)
        .await;
    let mut invalid = policy(app.bot, 2);
    invalid["policy"]["prices"]["billing"] = json!("audio");
    app.call("PUT", "/v1/inference-configurations", invalid, 400)
        .await;
    let mut invalid = policy(app.bot, 2);
    invalid["policy"]["model_id"] = json!("mistral-small-latest");
    app.call("PUT", "/v1/inference-configurations", invalid, 400)
        .await;
    app.call(
        "PUT",
        "/v1/provider-keys/mistral",
        json!({"secret":KEY}),
        200,
    )
    .await;
    app.submit(&request, 409).await;
    assert_eq!(fixture.chats(), 0);
    app.shutdown().await;
    no_plaintext(local.directory());
}

#[tokio::test]
async fn m04_unknown_responses_retain_reservations_without_retry() {
    let root = tempfile::tempdir().unwrap();
    let local = private_local(&root);
    let fixture = Fixture::start().await;
    let app = App::start(local.directory().into(), &fixture).await;
    app.initialize().await;
    let completion: Value = serde_json::from_str(COMPLETION).unwrap();
    let mut missing = completion.clone();
    missing.as_object_mut().unwrap().remove("usage");
    let mut usage = completion.clone();
    usage["usage"]["total_tokens"] = json!(999);
    let mut extra_usage = completion.clone();
    extra_usage["usage"]["audio_tokens"] = json!(1);
    let mut priority_usage = completion.clone();
    priority_usage["usage"]["service_tier"] = json!("priority");
    let mut unknown_tier = completion.clone();
    unknown_tier["usage"]["service_tier"] = json!("unknown");
    let mut mismatch = completion.clone();
    mismatch["model"] = json!("different-model");
    let mut tools = completion.clone();
    tools["choices"][0]["message"]["tool_calls"] = json!([{"id":"tool"}]);
    let mut reflected = completion.clone();
    reflected["choices"][0]["message"]["content"] = json!(KEY);
    let encoded_secret = reflected
        .to_string()
        .replace(KEY, &KEY.replace("secret", "\\u0073ecret"));
    let mut large_output = completion.clone();
    large_output["choices"][0]["message"]["content"] =
        json!("x".repeat(MAX_INFERENCE_OUTPUT_BYTES + 1));
    let replies = vec![
        Reply {
            status: StatusCode::TOO_MANY_REQUESTS,
            body: ERROR.into(),
            ..Reply::default()
        },
        Reply {
            status: StatusCode::TEMPORARY_REDIRECT,
            ..Reply::default()
        },
        Reply {
            body: missing.to_string(),
            ..Reply::default()
        },
        Reply {
            body: usage.to_string(),
            ..Reply::default()
        },
        Reply {
            body: extra_usage.to_string(),
            ..Reply::default()
        },
        Reply {
            body: priority_usage.to_string(),
            ..Reply::default()
        },
        Reply {
            body: unknown_tier.to_string(),
            ..Reply::default()
        },
        Reply {
            body: mismatch.to_string(),
            ..Reply::default()
        },
        Reply {
            body: tools.to_string(),
            ..Reply::default()
        },
        Reply {
            body: reflected.to_string(),
            ..Reply::default()
        },
        Reply {
            body: encoded_secret,
            ..Reply::default()
        },
        Reply {
            body: large_output.to_string(),
            ..Reply::default()
        },
        Reply {
            body: "x".repeat(MAX_PROVIDER_BODY + 1),
            ..Reply::default()
        },
        Reply {
            body: "x".repeat(MAX_PROVIDER_BODY + 1),
            chunked: true,
            ..Reply::default()
        },
        Reply {
            interrupted: true,
            ..Reply::default()
        },
        Reply {
            body: "malformed".into(),
            ..Reply::default()
        },
    ];
    let count = replies.len() + 1;
    for reply in replies {
        fixture.reply(reply);
        let input = app.request(1);
        app.submit(&input, 200).await;
        let view = app.settled(input.id).await;
        assert_eq!(view["phase"]["type"], "awaiting_attention");
        assert_eq!(view["accounting"]["type"], "held");
        if view["phase"]["reason"] != "interrupted" {
            assert!(view["provider_request_id"].is_string());
        }
        assert!(view["output"].is_null());
        assert_eq!(app.submit(&input, 200).await, view);
    }
    fixture.reply(Reply {
        gate: Some(Arc::new(Semaphore::new(0))),
        ..Reply::default()
    });
    let mut input = app.request(1);
    input.deadline = now() + 2;
    app.submit(&input, 200).await;
    let view = app.settled(input.id).await;
    assert_eq!(view["phase"]["reason"], "interrupted");
    assert_eq!(view["accounting"]["type"], "held");
    assert_eq!(fixture.chats(), count);
    app.shutdown().await;
    let app = App::start(local.directory().into(), &fixture).await;
    assert_eq!(fixture.chats(), count);
    app.shutdown().await;
    no_plaintext(local.directory());
}

#[tokio::test]
async fn m04_capacity_wait_expires_without_sending() {
    let root = tempfile::tempdir().unwrap();
    let local = private_local(&root);
    let fixture = Fixture::start().await;
    let app = App::start(local.directory().into(), &fixture).await;
    app.initialize().await;
    let capacity = app
        .manager
        .provider
        .calls
        .clone()
        .acquire_many_owned(MAX_INFERENCE_CALLS as u32)
        .await
        .unwrap();
    let mut input = app.request(1);
    input.deadline = now() + 2;
    app.submit(&input, 200).await;
    let view = app.settled(input.id).await;
    assert_eq!(view["phase"]["type"], "not_sent");
    assert_eq!(view["phase"]["reason"], "deadline");
    assert_eq!(view["accounting"]["type"], "released");
    assert_eq!(fixture.chats(), 0);
    drop(capacity);
    assert_eq!(app.submit(&input, 200).await, view);
    assert_eq!(fixture.chats(), 0);
    app.shutdown().await;
}

#[tokio::test]
async fn m04_concurrency_cancellation_disconnect_and_shutdown() {
    let root = tempfile::tempdir().unwrap();
    let local = private_local(&root);
    let fixture = Fixture::start().await;
    let app = App::start(local.directory().into(), &fixture).await;
    app.initialize().await;
    let gate = Arc::new(Semaphore::new(0));
    fixture.reply(Reply {
        gate: Some(gate.clone()),
        ..Reply::default()
    });
    let first = app.request(1);
    let second = app.request(1);
    app.submit(&first, 200).await;
    app.submit(&second, 200).await;
    fixture.reached(2).await;
    assert_eq!(app.submit(&app.request(1), 503).await["error"], "busy");
    let view = app
        .call(
            "POST",
            &format!("/v1/inferences/{}/cancel", first.id),
            json!({}),
            200,
        )
        .await;
    assert_eq!(view["phase"]["type"], "sending");
    assert_eq!(view["accounting"]["type"], "held");
    tokio::time::timeout(
        Duration::from_secs(1),
        app.call("GET", "/v1/owner", Value::Null, 200),
    )
    .await
    .expect("state queue remains responsive during HTTP");
    gate.add_permits(2);
    let completed = app.settled(first.id).await;
    app.settled(second.id).await;
    assert_eq!(completed["phase"]["type"], "completed");
    assert!(!completed["cancellation_requested_at"].is_null());
    assert_eq!(completed["accounting"]["charged_microusd"], 28);
    assert_eq!(
        app.call(
            "POST",
            &format!("/v1/inferences/{}/cancel", first.id),
            json!({}),
            200
        )
        .await,
        completed
    );
    let gate = Arc::new(Semaphore::new(0));
    fixture.reply(Reply {
        gate: Some(gate),
        ..Reply::default()
    });
    let disconnected = app.request(1);
    let host = app.address.strip_prefix("http://").unwrap();
    let mut stream = tokio::net::TcpStream::connect(host).await.unwrap();
    use tokio::io::AsyncWriteExt;
    let body = serde_json::to_string(&disconnected).unwrap();
    stream.write_all(format!("POST /v1/inferences HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", app.token, body.len()).as_bytes()).await.unwrap();
    fixture.reached(3).await;
    drop(stream);
    assert_eq!(app.view(disconnected.id).await["phase"]["type"], "sending");
    app.shutdown().await;
    let app = App::start(local.directory().into(), &fixture).await;
    let view = app.view(disconnected.id).await;
    assert_eq!(view["phase"]["type"], "awaiting_attention");
    assert_eq!(view["phase"]["reason"], "shutdown");
    assert_eq!(view["accounting"]["type"], "held");
    assert_eq!(app.submit(&disconnected, 200).await, view);
    assert_eq!(fixture.chats(), 3);
    app.shutdown().await;
    no_plaintext(local.directory());
}

#[tokio::test]
async fn m04_before_sending_cancellation_and_authority_rechecks() {
    let root = tempfile::tempdir().unwrap();
    let local = private_local(&root);
    let fixture = Fixture::start().await;
    let app = App::start(local.directory().into(), &fixture).await;
    app.initialize().await;
    let input = app.request(1);
    let id = input.id;
    let token = app.session();
    let cancel_token = app.session();
    let cancelled = app
        .manager
        .queue
        .call(false, move |s| {
            s.submit_inference(token, input)?;
            let view = s.cancel_inference(cancel_token, id)?;
            assert!(s.claim_inference()?.is_none());
            Ok(view)
        })
        .await
        .unwrap();
    assert_eq!(cancelled.phase, InferencePhase::Cancelled);
    assert_eq!(cancelled.accounting, InferenceAccounting::Released);
    assert_eq!(app.view(id).await, json!(cancelled));
    let input = app.request(1);
    let id = input.id;
    let token = app.session();
    let lock_token = app.session();
    app.manager
        .queue
        .call(false, move |s| {
            s.submit_inference(token, input)?;
            s.lock_vault(lock_token)?;
            assert!(s.claim_inference()?.is_none());
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        app.view(id).await["phase"],
        json!({"type":"not_sent","reason":"vault_locked"})
    );
    app.call(
        "POST",
        "/v1/vault/unlock",
        json!({"passphrase":PASSPHRASE}),
        204,
    )
    .await;
    let input = app.request(1);
    let id = input.id;
    let token = app.session();
    let config_token = app.session();
    let config: ConfigureInference = serde_json::from_value(policy(app.bot, 1)).unwrap();
    app.manager
        .queue
        .call(false, move |s| {
            s.submit_inference(token, input)?;
            s.configure_inference(config_token, config)?;
            assert!(s.claim_inference()?.is_none());
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        app.view(id).await["phase"],
        json!({"type":"not_sent","reason":"configuration_changed"})
    );
    app.verify(2, 200).await;
    let mut input = app.request(2);
    input.deadline = now() + 1;
    let id = input.id;
    let token = app.session();
    app.manager
        .queue
        .call(false, move |s| s.submit_inference(token, input))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(1100)).await;
    app.manager
        .queue
        .call(false, |s| {
            assert!(s.claim_inference()?.is_none());
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        app.view(id).await["phase"],
        json!({"type":"not_sent","reason":"deadline"})
    );
    assert_eq!(fixture.chats(), 0);
    app.shutdown().await;
}

#[tokio::test]
async fn m04_completion_waits_for_state_queue_capacity() {
    let root = tempfile::tempdir().unwrap();
    let local = private_local(&root);
    let fixture = Fixture::start().await;
    let app = App::start(local.directory().into(), &fixture).await;
    app.initialize().await;
    let gate = Arc::new(Semaphore::new(0));
    fixture.reply(Reply {
        gate: Some(gate.clone()),
        ..Reply::default()
    });
    let input = app.request(1);
    app.submit(&input, 200).await;
    fixture.reached(1).await;
    let (entered_tx, entered_rx) = oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let queue = app.manager.queue.clone();
    let blocker = tokio::spawn(async move {
        queue
            .call(false, move |_| {
                let _ = entered_tx.send(());
                release_rx.recv().unwrap();
                Ok(())
            })
            .await
    });
    entered_rx.await.unwrap();
    while app.manager.queue.sender.try_send(Box::new(|_| {})).is_ok() {}
    assert_eq!(app.manager.queue.sender.capacity(), 0);
    gate.add_permits(1);
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(app.manager.queue.sender.capacity(), 0);
    assert_eq!(
        app.manager.owner(app.session()).await,
        Err(OperationError::Busy)
    );
    release_tx.send(()).unwrap();
    blocker.await.unwrap().unwrap();
    app.manager.queue.call_wait(|_| Ok(())).await.unwrap();
    let view = app.settled(input.id).await;
    assert_eq!(view["phase"]["type"], "completed");
    assert_eq!(view["accounting"]["charged_microusd"], 28);
    assert_eq!(fixture.chats(), 1);
    app.shutdown().await;
}

#[tokio::test]
async fn m04_crash_marks_sending_uncertain_and_never_replaces_call() {
    let root = tempfile::tempdir().unwrap();
    let local = private_local(&root);
    let fixture = Fixture::start().await;
    let mut app = App::start(local.directory().into(), &fixture).await;
    app.initialize().await;
    fixture.reply(Reply {
        gate: Some(Arc::new(Semaphore::new(0))),
        ..Reply::default()
    });
    let sending = app.request(1);
    app.submit(&sending, 200).await;
    fixture.reached(1).await;
    let task = app.runtime.inference.task.take().unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let admitted = app.request(1);
    let input = admitted.clone();
    let token = app.session();
    app.manager
        .queue
        .call(false, move |s| s.submit_inference(token, input))
        .await
        .unwrap();
    app.crash().await;
    let app = App::start(local.directory().into(), &fixture).await;
    let uncertain = app.view(sending.id).await;
    assert_eq!(uncertain["phase"]["type"], "awaiting_attention");
    assert_eq!(uncertain["phase"]["reason"], "restart");
    assert_eq!(uncertain["accounting"]["type"], "held");
    let not_sent = app.view(admitted.id).await;
    assert_eq!(
        not_sent["phase"],
        json!({"type":"not_sent","reason":"restart"})
    );
    assert_eq!(not_sent["accounting"]["type"], "released");
    assert_eq!(app.submit(&sending, 200).await, uncertain);
    assert_eq!(app.submit(&admitted, 200).await, not_sent);
    assert_eq!(fixture.chats(), 1);
    app.shutdown().await;
    no_plaintext(local.directory());
}

#[path = "run_tests.rs"]
mod run_tests;

#[path = "coding_production.rs"]
mod coding_production;

#[path = "integrated_fixture.rs"]
mod integrated_fixture;
