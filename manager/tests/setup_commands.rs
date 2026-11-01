use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    os::unix::fs::OpenOptionsExt,
    path::Path,
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Process {
    _child: ChildGuard,
    listen: SocketAddr,
}

impl Process {
    fn start(root: &Path, state: &Path) -> Self {
        let _ = fs::remove_file(state.join("listeners.json"));
        let mut child = ChildGuard(manager(state, root).spawn().unwrap());
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
                return Self {
                    _child: child,
                    listen,
                };
            }
            if Instant::now() >= deadline {
                panic!("manager readiness deadline");
            }
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

fn manager(state: &Path, root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_botstral-manager"));
    command
        .args(["--state-dir"])
        .arg(state)
        .args(["--listen", "127.0.0.1:0", "--setup-listen", "127.0.0.1:0"])
        .stdout(Stdio::from(private_file(&root.join("stdout"))))
        .stderr(Stdio::from(private_file(&root.join("stderr"))));
    command
}

fn admin(state: &Path, args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_botstral-manager"))
        .args(args)
        .arg("--state-dir")
        .arg(state)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn request(
    address: SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Value,
) -> Value {
    let payload = serde_json::to_vec(&body).unwrap();
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(5)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        payload.len()
    );
    if let Some(token) = token {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).unwrap();
    stream.write_all(&payload).unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    let text = String::from_utf8(response).unwrap();
    let (_head, body) = text.split_once("\r\n\r\n").expect("short response");
    if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(body).unwrap()
    }
}

#[test]
fn fresh_install_reaches_unverified_model_without_loopback_scripts() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let state = root.join("state");
    let process = Process::start(root, &state);
    let password = "synthetic-login-password";
    let passphrase = "synthetic-vault-passphrase";
    let key = "synthetic-mistral-key";

    let setup = admin(
        &state,
        &["setup"],
        &format!("yes\nSynthetic owner\n{password}\n{password}\n{passphrase}\n{passphrase}\n"),
    );
    assert!(
        setup.status.success(),
        "setup: {}",
        String::from_utf8_lossy(&setup.stderr)
    );
    let stdout = String::from_utf8_lossy(&setup.stdout);
    assert!(stdout.contains("Synthetic owner"), "setup stdout: {stdout}");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let listeners: Value =
            serde_json::from_slice(&fs::read(state.join("listeners.json")).unwrap()).unwrap();
        if listeners["setup"].is_null() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "setup listener closes after owner creation"
        );
        thread::sleep(Duration::from_millis(20));
    }

    let unlock = admin(&state, &["unlock"], &format!("{password}\n{passphrase}\n"));
    assert!(
        unlock.status.success(),
        "unlock: {}",
        String::from_utf8_lossy(&unlock.stderr)
    );
    assert!(
        String::from_utf8_lossy(&unlock.stdout).contains("vault unlocked"),
        "unlock stdout: {}",
        String::from_utf8_lossy(&unlock.stdout)
    );

    let stored = admin(&state, &["provider-key"], &format!("{password}\n{key}\n"));
    assert!(
        stored.status.success(),
        "provider-key: {}",
        String::from_utf8_lossy(&stored.stderr)
    );
    let grant = request(
        process.listen,
        "POST",
        "/v1/sessions",
        None,
        json!({"password": password}),
    );
    let token = grant["token"].as_str().expect("issued token");
    let status = request(process.listen, "GET", "/v1/status", Some(token), json!({}));
    assert_eq!(status["vault"], "unlocked");
    assert_eq!(status["provider"]["provider"], "mistral");

    let configure = admin(
        &state,
        &[
            "configure",
            "Botstral",
            "--model",
            "mistral-small-2603",
            "--endpoint",
            "eu",
            "--input-price",
            "150000",
            "--output-price",
            "600000",
            "--context",
            "32768",
        ],
        &format!("{password}\n"),
    );
    assert!(
        !configure.status.success(),
        "configure should fail without a live Mistral key"
    );
    let stderr = String::from_utf8_lossy(&configure.stderr);
    assert!(
        stderr.contains("inference_unverified"),
        "configure stderr: {stderr}"
    );
}
