use rusqlite::Connection;
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    os::fd::AsRawFd,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt, symlink},
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Process {
    child: ChildGuard,
    listen: SocketAddr,
    setup: Option<SocketAddr>,
}
impl Process {
    fn start(root: &Path, state: &Path, generation: u8) -> Self {
        let _ = fs::remove_file(state.join("listeners.json"));
        let mut child = ChildGuard(command(state, root, generation).spawn().unwrap());
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "manager stays alive during startup"
            );
            if let Ok(bytes) = fs::read(state.join("listeners.json"))
                && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
            {
                let listen = value["listen"].as_str().unwrap().parse().unwrap();
                let setup = value["setup"].as_str().map(|value| value.parse().unwrap());
                return Self {
                    child,
                    listen,
                    setup,
                };
            }
            if Instant::now() >= deadline {
                panic!("manager readiness deadline");
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
    fn stop(&mut self, force: bool) {
        if force {
            self.child.0.kill().unwrap();
        } else {
            assert_eq!(
                unsafe { libc::kill(self.child.0.id() as i32, libc::SIGTERM) },
                0
            );
        }
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = self.child.0.try_wait().unwrap() {
                assert_eq!(status.success(), !force, "expected process exit kind");
                println!("Manager stopped with {status}.");
                return;
            }
            assert!(Instant::now() < deadline, "manager shutdown deadline");
            thread::sleep(Duration::from_millis(20));
        }
    }
}

fn private_file(path: &Path) -> File {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap()
}
fn command(state: &Path, root: &Path, generation: u8) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_botstral-manager"));
    command
        .args(["--state-dir"])
        .arg(state)
        .args(["--listen", "127.0.0.1:0", "--setup-listen", "127.0.0.1:0"])
        .stdout(Stdio::from(private_file(
            &root.join(format!("stdout-{generation}")),
        )))
        .stderr(Stdio::from(private_file(
            &root.join(format!("stderr-{generation}")),
        )));
    command
}

fn refused(command: &mut Command) {
    let mut child = ChildGuard(command.spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(
                !status.success(),
                "startup refuses unsafe state or configuration"
            );
            println!("Refused startup with {status}.");
            return;
        }
        if Instant::now() >= deadline {
            panic!("startup refusal deadline");
        }
        thread::sleep(Duration::from_millis(20));
    }
}

struct Response {
    status: u16,
    body: Value,
    bytes: Vec<u8>,
}
fn connect(address: SocketAddr) -> TcpStream {
    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(5)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
}

fn stalled_bot_list(process: &Process, state: &Path, token: &str) -> TcpStream {
    let db = Connection::open(state.join("manager.sqlite3")).unwrap();
    db.execute(
        "WITH RECURSIVE sequence(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM sequence WHERE n < 50000)
         INSERT INTO bots(id, name, created_at)
         SELECT printf('00000000-0000-0000-0000-%012x', n), ?1, 0 FROM sequence",
        ["x".repeat(100)],
    )
    .unwrap();
    drop(db);
    let mut stream = connect(process.listen);
    let receive_buffer: libc::c_int = 1024;
    assert_eq!(
        unsafe {
            libc::setsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_RCVBUF,
                (&receive_buffer as *const libc::c_int).cast(),
                std::mem::size_of_val(&receive_buffer) as libc::socklen_t,
            )
        },
        0
    );
    stream
        .write_all(&request_bytes(
            process.listen,
            "GET",
            "/v1/bots",
            Some(token),
            b"{}",
            "",
        ))
        .unwrap();
    let mut status = [0; 12];
    stream.read_exact(&mut status).unwrap();
    assert_eq!(&status, b"HTTP/1.1 200");
    stream
}
fn request_bytes(
    address: SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: &[u8],
    extra: &str,
) -> Vec<u8> {
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        body.len()
    );
    if let Some(token) = token {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    request.push_str(extra);
    request.push_str("\r\n");
    let mut bytes = request.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}
fn receive(mut stream: TcpStream) -> Response {
    let mut bytes = Vec::new();
    loop {
        let mut chunk = [0; 4096];
        let count = stream.read(&mut chunk).unwrap();
        assert!(count > 0, "complete HTTP response before connection closes");
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(separator) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            let headers = std::str::from_utf8(&bytes[..separator]).unwrap();
            let length = headers.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            });
            let status = headers.split_whitespace().nth(1).unwrap();
            let length = length.unwrap_or_else(|| {
                assert!(
                    ["204", "304"].contains(&status),
                    "response body has a length"
                );
                0
            });
            if bytes.len() >= separator + 4 + length {
                break;
            }
        }
    }
    let separator = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .expect("HTTP response");
    let headers = std::str::from_utf8(&bytes[..separator]).unwrap();
    let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
    assert!(
        headers
            .to_ascii_lowercase()
            .contains("cache-control: no-store"),
        "HTTP responses disable caching"
    );
    assert!(
        !headers
            .to_ascii_lowercase()
            .contains("access-control-allow-origin"),
        "no browser grants"
    );
    let body = if bytes.len() == separator + 4 {
        Value::Null
    } else {
        serde_json::from_slice(&bytes[separator + 4..]).expect("JSON response")
    };
    Response {
        status,
        body,
        bytes,
    }
}
fn raw(
    address: SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: &[u8],
    extra: &str,
) -> Response {
    let mut stream = connect(address);
    stream
        .write_all(&request_bytes(address, method, path, token, body, extra))
        .unwrap();
    receive(stream)
}
fn request(
    address: SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Value,
) -> Response {
    raw(
        address,
        method,
        path,
        token,
        &serde_json::to_vec(&body).unwrap(),
        "",
    )
}
fn expect(response: Response, status: u16) -> Value {
    assert_eq!(response.status, status, "HTTP outcome");
    response.body
}
fn sign_in(address: SocketAddr, password: &str, tokens: &mut Vec<String>) -> Value {
    let grant = expect(
        request(
            address,
            "POST",
            "/v1/sessions",
            None,
            json!({"password": password}),
        ),
        201,
    );
    let token = grant["token"].as_str().expect("issued token");
    assert_eq!(token.len(), 43);
    tokens.push(token.to_owned());
    grant
}
fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|part| part == needle)
}
fn scan(root: &Path, secrets: &[String]) {
    use base64::Engine;
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            scan(&path, secrets);
            continue;
        }
        let bytes = fs::read(&path).unwrap();
        assert!(
            !contains(&bytes, b"AGE-SECRET-KEY-"),
            "no plaintext identity in generated files"
        );
        for secret in secrets {
            let encoded = serde_json::to_string(secret).unwrap();
            let base64 = base64::engine::general_purpose::STANDARD.encode(secret);
            for needle in [
                secret.as_bytes(),
                &encoded.as_bytes()[1..encoded.len() - 1],
                base64.as_bytes(),
            ] {
                assert!(
                    !contains(&bytes, needle),
                    "no synthetic secret in generated files or captured output"
                );
            }
        }
    }
}
fn permissions(state: &Path) {
    let metadata = fs::symlink_metadata(state).unwrap();
    assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
    for entry in fs::read_dir(state).unwrap() {
        let metadata = entry.unwrap().metadata().unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
    }
}

#[test]
fn m01_real_binary_over_tcp() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let state = root.join("state");
    let password = format!("synthetic-login-{}-\"\\", Uuid::new_v4());
    let passphrase = format!("synthetic-vault-{}-\n", Uuid::new_v4());
    let key = format!("synthetic-provider-{}-\"\\\n", Uuid::new_v4());
    let replacement = format!("synthetic-replacement-{}", Uuid::new_v4());
    let mut tokens = Vec::new();
    let mut process = Process::start(root, &state, 0);
    let stale_capability = fs::read_to_string(state.join("bootstrap.token")).unwrap();
    process.stop(false);
    let mut process = Process::start(root, &state, 10);
    let setup = process.setup.expect("dedicated setup listener");
    let capability = fs::read_to_string(state.join("bootstrap.token")).unwrap();
    assert!(
        capability != stale_capability,
        "unconfigured restart rotates bootstrap proof"
    );
    let setup_body = json!({"name":"Synthetic owner", "password":password, "vault_passphrase":passphrase, "acknowledge_no_escrow":true});
    permissions(&state);
    expect(
        request(setup, "POST", "/v1/owner", None, setup_body.clone()),
        401,
    );
    expect(
        request(
            setup,
            "POST",
            "/v1/owner",
            Some(&"x".repeat(43)),
            setup_body.clone(),
        ),
        401,
    );
    expect(
        request(
            setup,
            "POST",
            "/v1/owner",
            Some(&stale_capability),
            setup_body.clone(),
        ),
        401,
    );
    expect(
        raw(
            setup,
            "POST",
            "/v1/owner",
            Some(&capability),
            &serde_json::to_vec(&setup_body).unwrap(),
            "Origin: null\r\n",
        ),
        403,
    );
    let mut rejected = setup_body.clone();
    rejected["acknowledge_no_escrow"] = json!(false);
    expect(
        request(setup, "POST", "/v1/owner", Some(&capability), rejected),
        400,
    );
    let mut rejected = setup_body.clone();
    rejected["extra"] = json!(key);
    expect(
        request(setup, "POST", "/v1/owner", Some(&capability), rejected),
        400,
    );
    expect(
        raw(
            process.listen,
            "POST",
            "/v1/owner",
            Some(&capability),
            &serde_json::to_vec(&setup_body).unwrap(),
            "X-Forwarded-For: 127.0.0.1\r\n",
        ),
        404,
    );
    expect(request(setup, "GET", "/v1/bots", None, json!({})), 404);
    let mut first = connect(setup);
    let mut second = connect(setup);
    let bytes = request_bytes(
        setup,
        "POST",
        "/v1/owner",
        Some(&capability),
        &serde_json::to_vec(&setup_body).unwrap(),
        "",
    );
    first.write_all(&bytes).unwrap();
    second.write_all(&bytes).unwrap();
    let first = receive(first);
    let second = receive(second);
    assert_eq!(
        [first.status, second.status]
            .into_iter()
            .filter(|status| *status == 201)
            .count(),
        1,
        "one concurrent setup commits"
    );
    assert!(
        [first.status, second.status]
            .into_iter()
            .all(|status| [201, 409, 503].contains(&status))
    );
    assert!(!state.join("bootstrap.token").exists());
    let deadline = Instant::now() + Duration::from_secs(5);
    while TcpStream::connect(setup).is_ok() {
        assert!(
            Instant::now() < deadline,
            "setup listener closes after commit"
        );
        thread::sleep(Duration::from_millis(10));
    }
    println!("Setup is atomic, capability protected, and isolated from the regular listener.");

    let id = Uuid::new_v4();
    let bot_path = format!("/v1/bots/{id}");
    let protected = [
        ("GET", "/v1/bots", json!({})),
        ("GET", bot_path.as_str(), json!({})),
        ("GET", "/v1/owner", json!({})),
        ("GET", "/v1/status", json!({})),
        ("GET", "/v1/sessions", json!({})),
        ("POST", "/v1/vault/unlock", json!({"passphrase":passphrase})),
        ("POST", "/v1/vault/lock", json!({})),
        ("PUT", "/v1/provider-keys/mistral", json!({"secret":key})),
        ("PUT", bot_path.as_str(), json!({"name":"Protected bot"})),
        ("DELETE", "/v1/session", json!({})),
        (
            "DELETE",
            "/v1/sessions/00000000-0000-0000-0000-000000000000",
            json!({}),
        ),
    ];
    for (method, path, body) in protected {
        for token in [None, Some("x".repeat(43))] {
            let response = request(process.listen, method, path, token.as_deref(), body.clone());
            assert_eq!(response.status, 401, "authentication protects {path}");
            assert_eq!(response.body, json!({"error":"unauthorized"}));
        }
    }
    expect(
        request(
            process.listen,
            "POST",
            "/v1/sessions",
            None,
            json!({"password":"synthetic incorrect password"}),
        ),
        401,
    );
    let first = sign_in(process.listen, &password, &mut tokens);
    let second = sign_in(process.listen, &password, &mut tokens);
    let first_token = first["token"].as_str().unwrap();
    let token = second["token"].as_str().unwrap();
    assert_eq!(
        second["session"]["expires_at"].as_i64().unwrap()
            - second["session"]["created_at"].as_i64().unwrap(),
        24 * 60 * 60
    );
    assert_eq!(
        expect(
            request(process.listen, "GET", "/v1/owner", Some(token), json!({})),
            200
        ),
        json!({"name":"Synthetic owner"})
    );
    let original_bot = expect(
        request(
            process.listen,
            "PUT",
            &bot_path,
            Some(token),
            json!({"name":"Persistent bot å"}),
        ),
        200,
    );
    assert_eq!(original_bot["id"], id.to_string());
    assert_eq!(original_bot["name"], "Persistent bot å");
    assert!(original_bot["created_at"].as_i64().unwrap() > 0);
    assert_eq!(
        expect(
            request(
                process.listen,
                "PUT",
                &bot_path,
                Some(token),
                json!({"name":"Persistent bot å"})
            ),
            200
        ),
        original_bot
    );
    expect(
        request(
            process.listen,
            "PUT",
            &bot_path,
            Some(token),
            json!({"name":"Conflicting bot"}),
        ),
        409,
    );
    assert_eq!(
        expect(
            request(process.listen, "GET", "/v1/bots", Some(token), json!({})),
            200
        ),
        json!([original_bot])
    );
    expect(
        request(
            process.listen,
            "GET",
            "/v1/bots/00000000-0000-0000-0000-000000000000",
            Some(token),
            json!({}),
        ),
        404,
    );

    let status = expect(
        request(process.listen, "GET", "/v1/status", Some(token), json!({})),
        200,
    );
    assert_eq!(
        status,
        json!({"vault":"locked","provider":null,"worker":"not_implemented","inference":"single_turn_text","remote":"disabled"})
    );
    expect(
        request(
            process.listen,
            "PUT",
            "/v1/provider-keys/mistral",
            Some(token),
            json!({"secret":key}),
        ),
        409,
    );
    expect(
        request(
            process.listen,
            "POST",
            "/v1/vault/unlock",
            Some(token),
            json!({"passphrase":password}),
        ),
        401,
    );
    assert_eq!(
        expect(
            request(process.listen, "GET", "/v1/status", Some(token), json!({})),
            200
        )["vault"],
        "locked"
    );
    expect(
        request(
            process.listen,
            "POST",
            "/v1/vault/unlock",
            Some(token),
            json!({"passphrase":passphrase}),
        ),
        204,
    );
    expect(
        request(
            process.listen,
            "POST",
            "/v1/vault/unlock",
            Some(token),
            json!({"passphrase":passphrase}),
        ),
        409,
    );
    let metadata = expect(
        request(
            process.listen,
            "PUT",
            "/v1/provider-keys/mistral",
            Some(token),
            json!({"secret":key}),
        ),
        200,
    );
    assert_eq!(metadata.as_object().unwrap().len(), 2);
    assert_eq!(metadata["provider"], "mistral");
    let metadata = expect(
        request(
            process.listen,
            "PUT",
            "/v1/provider-keys/mistral",
            Some(token),
            json!({"secret":replacement}),
        ),
        200,
    );
    for path in [
        "/v1/provider-keys/mistral",
        "/v1/provider-keys/mistral/export",
        "/v1/provider-keys",
        "/v1/vault/export",
    ] {
        let response = request(process.listen, "GET", path, Some(token), json!({}));
        assert_eq!(response.status, 404);
        assert!(!contains(&response.bytes, key.as_bytes()));
        assert!(!contains(&response.bytes, replacement.as_bytes()));
    }
    let sessions = expect(
        request(
            process.listen,
            "GET",
            "/v1/sessions",
            Some(token),
            json!({}),
        ),
        200,
    );
    assert_eq!(sessions.as_array().unwrap().len(), 2);
    expect(
        request(
            process.listen,
            "DELETE",
            &format!("/v1/sessions/{}", first["session"]["id"].as_str().unwrap()),
            Some(token),
            json!({}),
        ),
        204,
    );
    expect(
        request(
            process.listen,
            "GET",
            &bot_path,
            Some(first_token),
            json!({}),
        ),
        401,
    );
    assert_eq!(
        expect(
            request(process.listen, "GET", "/v1/status", Some(token), json!({})),
            200
        )["vault"],
        "unlocked"
    );
    let third = sign_in(process.listen, &password, &mut tokens);
    let third_token = third["token"].as_str().unwrap();
    expect(
        request(
            process.listen,
            "DELETE",
            "/v1/session",
            Some(third_token),
            json!({}),
        ),
        204,
    );
    expect(
        request(
            process.listen,
            "GET",
            "/v1/owner",
            Some(third_token),
            json!({}),
        ),
        401,
    );
    assert_eq!(
        expect(
            request(process.listen, "GET", "/v1/status", Some(token), json!({})),
            200
        )["vault"],
        "unlocked"
    );
    expect(
        request(
            process.listen,
            "POST",
            "/v1/vault/lock",
            Some(token),
            json!({}),
        ),
        204,
    );
    expect(
        request(
            process.listen,
            "POST",
            "/v1/vault/lock",
            Some(token),
            json!({}),
        ),
        204,
    );
    expect(
        request(
            process.listen,
            "PUT",
            "/v1/provider-keys/mistral",
            Some(token),
            json!({"secret":key}),
        ),
        409,
    );
    expect(
        request(
            process.listen,
            "POST",
            "/v1/vault/unlock",
            Some(token),
            json!({"passphrase":passphrase}),
        ),
        204,
    );
    println!(
        "Bots, sessions, revocation, explicit locking, protected replacement, and read refusal pass over TCP."
    );

    for extra in [
        "Origin: null\r\n",
        "Origin: https://hostile.example\r\n",
        "Sec-Fetch-Site: same-origin\r\n",
    ] {
        expect(
            raw(
                process.listen,
                "GET",
                "/v1/status",
                Some(token),
                b"{}",
                extra,
            ),
            403,
        );
    }
    let mut stream = connect(process.listen);
    stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: hostile.example\r\nConnection: close\r\n\r\n")
        .unwrap();
    expect(receive(stream), 403);
    let oversized = vec![b'x'; 16385];
    expect(
        raw(
            process.listen,
            "PUT",
            "/v1/provider-keys/mistral",
            Some(token),
            &oversized,
            "",
        ),
        413,
    );
    let mut stream = connect(process.listen);
    let body = serde_json::to_vec(&json!({"secret":key})).unwrap();
    let wire = format!(
        "PUT /v1/provider-keys/mistral HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Type: text/plain\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\n\r\n",
        process.listen,
        body.len()
    );
    stream.write_all(wire.as_bytes()).unwrap();
    stream.write_all(&body).unwrap();
    expect(receive(stream), 415);
    for body in [
        format!("{{\"secret\":\"{}\",\"unexpected\":1}}", replacement).into_bytes(),
        format!("{{\"secret\":{{\"{}\":1}}}}", replacement).into_bytes(),
        b"{bad json".to_vec(),
        serde_json::to_vec(&json!([replacement])).unwrap(),
    ] {
        let response = raw(
            process.listen,
            "PUT",
            "/v1/provider-keys/mistral",
            Some(token),
            &body,
            "",
        );
        assert_eq!(response.status, 400);
        assert!(!contains(&response.bytes, replacement.as_bytes()));
    }
    expect(
        request(
            process.listen,
            "PUT",
            &bot_path,
            Some(token),
            json!({"name":"bad\nname"}),
        ),
        400,
    );
    expect(
        request(
            process.listen,
            "GET",
            "/v1/bots/%FF",
            Some(token),
            json!({}),
        ),
        400,
    );
    expect(
        request(
            process.listen,
            "POST",
            "/v1/vault/lock",
            Some(token),
            json!([]),
        ),
        400,
    );
    expect(
        raw(
            process.listen,
            "GET",
            "/health",
            None,
            b"{}",
            &format!("X-Padding: {}\r\n", "x".repeat(8500)),
        ),
        431,
    );
    let mut stream = connect(process.listen);
    let wire = format!(
        "POST /v1/sessions HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 500\r\n\r\n{{",
        process.listen
    );
    stream.write_all(wire.as_bytes()).unwrap();
    expect(receive(stream), 408);
    expect(
        request(
            process.listen,
            "POST",
            "/v1/sessions",
            None,
            json!({"password":"synthetic incorrect password"}),
        ),
        401,
    );
    expect(
        request(
            process.listen,
            "POST",
            "/v1/sessions",
            None,
            json!({"password":password}),
        ),
        429,
    );
    expect(
        request(
            process.listen,
            "POST",
            "/v1/vault/lock",
            Some(token),
            json!({}),
        ),
        204,
    );
    for _ in 0..2 {
        expect(
            request(
                process.listen,
                "POST",
                "/v1/vault/unlock",
                Some(token),
                json!({"passphrase":password}),
            ),
            401,
        );
    }
    expect(
        request(
            process.listen,
            "POST",
            "/v1/vault/unlock",
            Some(token),
            json!({"passphrase":passphrase}),
        ),
        429,
    );
    println!(
        "Browser boundaries, strict JSON, body and header limits, deadlines, and credential limits pass."
    );

    refused(&mut command(&state, root, 1));
    permissions(&state);
    let db = Connection::open(state.join("manager.sqlite3")).unwrap();
    assert_eq!(
        db.query_row::<i64, _, _>("PRAGMA user_version", [], |row| row.get(0))
            .unwrap(),
        8
    );
    assert_eq!(
        db.query_row::<i64, _, _>("SELECT count(*) FROM owner", [], |row| row.get(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row::<i64, _, _>("SELECT count(*) FROM vault", [], |row| row.get(0))
            .unwrap(),
        1
    );
    let hashes = db
        .prepare("SELECT token_hash FROM sessions")
        .unwrap()
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(hashes.iter().all(|hash| hash.len() == 32));
    drop(db);
    let mut secrets = vec![
        password.clone(),
        passphrase.clone(),
        key.clone(),
        replacement.clone(),
    ];
    secrets.extend(tokens.clone());
    scan(root, &secrets);
    process.stop(true);
    let mut process = Process::start(root, &state, 2);
    assert!(process.setup.is_none());
    assert!(!state.join("bootstrap.token").exists());
    expect(
        request(process.listen, "GET", &bot_path, Some(token), json!({})),
        401,
    );
    let grant = sign_in(process.listen, &password, &mut tokens);
    let token = grant["token"].as_str().unwrap();
    assert_eq!(
        expect(
            request(process.listen, "GET", &bot_path, Some(token), json!({})),
            200
        ),
        original_bot
    );
    assert_eq!(
        expect(
            request(process.listen, "GET", "/v1/status", Some(token), json!({})),
            200
        ),
        json!({"vault":"locked", "provider":metadata, "worker":"not_implemented","inference":"single_turn_text","remote":"disabled"})
    );
    expect(
        request(
            process.listen,
            "POST",
            "/v1/vault/unlock",
            Some(token),
            json!({"passphrase":passphrase}),
        ),
        204,
    );
    let stalled = stalled_bot_list(&process, &state, token);
    process.stop(false);
    drop(stalled);
    println!("Graceful shutdown completes with an unread bot-list response.");
    let db = Connection::open(state.join("manager.sqlite3")).unwrap();
    let mut ciphertext: Vec<u8> = db
        .query_row("SELECT identity_ciphertext FROM vault", [], |row| {
            row.get(0)
        })
        .unwrap();
    let last = ciphertext.len() - 1;
    ciphertext[last] ^= 1;
    db.execute("UPDATE vault SET identity_ciphertext=?1", [ciphertext])
        .unwrap();
    drop(db);
    let mut process = Process::start(root, &state, 3);
    let grant = sign_in(process.listen, &password, &mut tokens);
    let token = grant["token"].as_str().unwrap();
    expect(
        request(
            process.listen,
            "POST",
            "/v1/vault/unlock",
            Some(token),
            json!({"passphrase":passphrase}),
        ),
        401,
    );
    assert_eq!(
        expect(
            request(process.listen, "GET", "/v1/status", Some(token), json!({})),
            200
        )["vault"],
        "locked"
    );
    assert_eq!(
        expect(
            request(process.listen, "GET", &bot_path, Some(token), json!({})),
            200
        ),
        original_bot
    );
    process.stop(false);
    secrets.extend(tokens);
    scan(root, &secrets);
    println!(
        "A forced restart preserves the full bot and encrypted metadata, invalidates sessions, and locks the vault. Tampering fails unlock."
    );

    for (generation, option, address) in [
        (4, "--listen", "0.0.0.0:0"),
        (5, "--setup-listen", "[::]:0"),
        (6, "--listen", "localhost:0"),
    ] {
        refused(command(&state, root, generation).args([option, address]));
    }
    let future = root.join("future");
    fs::create_dir(&future).unwrap();
    fs::set_permissions(&future, fs::Permissions::from_mode(0o700)).unwrap();
    private_file(&future.join("manager.sqlite3"));
    let db = Connection::open(future.join("manager.sqlite3")).unwrap();
    db.pragma_update(None, "user_version", 9).unwrap();
    drop(db);
    refused(&mut command(&future, root, 7));
    let db = Connection::open(future.join("manager.sqlite3")).unwrap();
    assert_eq!(
        db.query_row::<i64, _, _>("PRAGMA user_version", [], |row| row.get(0))
            .unwrap(),
        9
    );
    drop(db);
    fs::set_permissions(
        state.join("manager.sqlite3"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    refused(&mut command(&state, root, 8));
    fs::set_permissions(
        state.join("manager.sqlite3"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let linked = root.join("linked-state");
    symlink(&state, &linked).unwrap();
    refused(&mut command(&linked, root, 9));
    fs::remove_file(linked).unwrap();
    scan(root, &secrets);
    println!(
        "Private permissions, second-process exclusion, unsafe binds and state rejection, migration refusal, and complete secret scans pass."
    );
}
