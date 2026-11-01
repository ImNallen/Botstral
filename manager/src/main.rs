use botstral_link::tls;
use botstral_manager::{LocalState, WorkerTls, protect_process, serve_with_worker};
use std::{
    ffi::OsString,
    io::{BufRead, IsTerminal, Read, Write},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::ExitCode,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use zeroize::Zeroizing;

const ADMIN_TIMEOUT: Duration = Duration::from_secs(30);
const INVITE_TIMEOUT: Duration = Duration::from_secs(15);

fn main() -> ExitCode {
    if botstral_link::release::print_version("manager") {
        return ExitCode::SUCCESS;
    }
    if run().is_err() {
        eprintln!("manager startup or shutdown failed");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    protect_process()?;
    let mut arguments = std::env::args_os().skip(1);
    match arguments.next().as_deref() {
        Some(command) if command == "invite" => return invite(arguments),
        Some(command) if command == "setup" => return setup(arguments),
        Some(command) if command == "unlock" => return unlock(arguments),
        Some(command) if command == "provider-key" => return provider_key(arguments),
        Some(command) if command == "configure" => return configure(arguments),
        _ => {}
    }
    let mut directory = PathBuf::from(".state");
    let mut listen: SocketAddr = "127.0.0.1:43120".parse()?;
    let mut setup_listen: SocketAddr = "127.0.0.1:43121".parse()?;
    let mut worker_listen = None;
    let mut arguments = std::env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--help" {
            println!(
                "botstral-manager [--state-dir PATH] [--listen LOOPBACK:PORT] [--setup-listen LOOPBACK:PORT] [--worker-listen ADDRESS:PORT]\nbotstral-manager invite worker [--state-dir PATH]\nbotstral-manager invite device --label LABEL [--state-dir PATH]\nbotstral-manager setup [--state-dir PATH]\nbotstral-manager unlock [--state-dir PATH]\nbotstral-manager provider-key [--state-dir PATH]\nbotstral-manager configure NAME --model ID [--endpoint eu] --input-price N --output-price N [--expected-revision N] [--context N] [--state-dir PATH]"
            );
            return Ok(());
        }
        let value = arguments.next().ok_or("missing option value")?;
        if argument == "--state-dir" {
            directory = PathBuf::from(value);
        } else if argument == "--listen" {
            listen = value.to_str().ok_or("invalid address")?.parse()?;
        } else if argument == "--setup-listen" {
            setup_listen = value.to_str().ok_or("invalid address")?.parse()?;
        } else if argument == "--worker-listen" || argument == "--public-listen" {
            if worker_listen.is_some() {
                return Err("duplicate public listener option".into());
            }
            worker_listen = Some(value.to_str().ok_or("invalid address")?.parse()?);
        } else {
            return Err("unknown option".into());
        }
    }
    if !listen.ip().is_loopback() || !setup_listen.ip().is_loopback() {
        return Err("unsafe bind address".into());
    }
    let state = LocalState::open(&directory)?;
    let worker_tls = match worker_listen {
        None => None,
        Some(listen) => {
            let identity = tls::ensure_identity(&directory, "manager-identity")?;
            Some(WorkerTls {
                listen,
                certificate: identity.certificate,
                key: identity.key,
            })
        }
    };
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(serve_with_worker(state, listen, setup_listen, worker_tls))?;
    Ok(())
}

fn invite(arguments: impl Iterator<Item = OsString>) -> Result<(), Box<dyn std::error::Error>> {
    let mut directory = PathBuf::from(".state");
    let mut kind = None;
    let mut label = None;
    let mut arguments = arguments.peekable();
    while let Some(argument) = arguments.next() {
        if argument == "--state-dir" {
            directory = PathBuf::from(arguments.next().ok_or("missing option value")?);
        } else if argument == "--label" {
            label = Some(
                arguments
                    .next()
                    .ok_or("missing option value")?
                    .into_string()
                    .map_err(|_| "invalid label")?,
            );
        } else if kind.is_none() && argument == "worker" {
            kind = Some("worker");
        } else if kind.is_none() && argument == "device" {
            kind = Some("device");
        } else {
            return Err("unknown option".into());
        }
    }
    let kind = kind.ok_or("missing invite target")?;
    if kind == "worker" && label.is_some() {
        return Err("unknown option".into());
    }
    if kind == "device" && label.as_ref().is_none_or(|v| v.is_empty()) {
        return Err("missing device label".into());
    }
    let address = listen_address(&directory)?;
    let password = read_password("Password: ")?;
    let (status, body) = http_json(
        address,
        "POST",
        "/v1/sessions",
        None,
        Some(&serde_json::json!({"password": password.as_str()})),
        INVITE_TIMEOUT,
    )?;
    if status != 201 {
        return Err("sign-in failed".into());
    }
    let token = body["token"].as_str().ok_or("sign-in failed")?;
    let (path, payload) = if kind == "worker" {
        (
            "/v1/worker-invitations",
            serde_json::json!({"lifetime_seconds": 600}),
        )
    } else {
        (
            "/v1/device-invitations",
            serde_json::json!({"label": label.unwrap(), "lifetime_seconds": 600}),
        )
    };
    let (status, body) = http_json(
        address,
        "POST",
        path,
        Some(token),
        Some(&payload),
        INVITE_TIMEOUT,
    )?;
    let signed_out = http_json(
        address,
        "DELETE",
        "/v1/session",
        Some(token),
        Some(&serde_json::json!({})),
        INVITE_TIMEOUT,
    );
    if status != 201 {
        return Err("invitation failed".into());
    }
    signed_out?;
    let code = body["code"].as_str().ok_or("invitation failed")?;
    println!("{code}");
    if kind == "device" {
        let qr = qrcode::QrCode::new(code.as_bytes()).map_err(|_| "qr")?;
        println!(
            "{}",
            qr.render()
                .quiet_zone(true)
                .module_dimensions(2, 1)
                .dark_color('#')
                .light_color(' ')
                .build()
        );
    }
    Ok(())
}

fn setup(arguments: impl Iterator<Item = OsString>) -> Result<(), Box<dyn std::error::Error>> {
    let directory = only_state_dir(arguments)?;
    let listeners = listeners(&directory)?;
    let Some(setup) = listeners["setup"].as_str() else {
        eprintln!("setup is closed");
        return Err("setup is closed".into());
    };
    let address: SocketAddr = setup.parse()?;
    let token = std::fs::read_to_string(directory.join("bootstrap.token"))?;
    eprintln!("There is no escrow. Keep an offline copy of the vault passphrase.");
    if read_line("Type yes to accept: ")? != "yes" {
        eprintln!("not accepted");
        return Err("not accepted".into());
    }
    let name = read_line("Owner name: ")?;
    let password = read_password("New sign-in password: ")?;
    if read_password("Repeat sign-in password: ")?.as_str() != password.as_str() {
        eprintln!("mismatch");
        return Err("mismatch".into());
    }
    let vault = read_password("Vault passphrase: ")?;
    if read_password("Repeat vault passphrase: ")?.as_str() != vault.as_str() {
        eprintln!("mismatch");
        return Err("mismatch".into());
    }
    let body = expect_json(
        address,
        "POST",
        "/v1/owner",
        Some(token.trim()),
        Some(&serde_json::json!({
            "name": name,
            "password": password.as_str(),
            "vault_passphrase": vault.as_str(),
            "acknowledge_no_escrow": true
        })),
        201,
    )?;
    println!("{body}");
    Ok(())
}

fn unlock(arguments: impl Iterator<Item = OsString>) -> Result<(), Box<dyn std::error::Error>> {
    let directory = only_state_dir(arguments)?;
    let address = listen_address(&directory)?;
    let token = sign_in(address)?;
    let passphrase = read_password("Vault passphrase: ")?;
    let result = expect_json(
        address,
        "POST",
        "/v1/vault/unlock",
        Some(token.as_str()),
        Some(&serde_json::json!({"passphrase": passphrase.as_str()})),
        204,
    );
    let signed_out = sign_out(address, token.as_str());
    result?;
    signed_out?;
    println!("vault unlocked");
    Ok(())
}

fn provider_key(
    arguments: impl Iterator<Item = OsString>,
) -> Result<(), Box<dyn std::error::Error>> {
    let directory = only_state_dir(arguments)?;
    let address = listen_address(&directory)?;
    let token = sign_in(address)?;
    let secret = read_password("Mistral API key: ")?;
    let body = expect_json(
        address,
        "PUT",
        "/v1/provider-keys/mistral",
        Some(token.as_str()),
        Some(&serde_json::json!({"secret": secret.as_str()})),
        200,
    );
    let signed_out = sign_out(address, token.as_str());
    let body = body?;
    signed_out?;
    println!("{body}");
    Ok(())
}

struct Configure {
    name: String,
    model: String,
    endpoint: String,
    input_price: u64,
    output_price: u64,
    expected_revision: u64,
    context: Option<u32>,
}

fn configure(arguments: impl Iterator<Item = OsString>) -> Result<(), Box<dyn std::error::Error>> {
    let mut directory = PathBuf::from(".state");
    let mut name = None;
    let mut model = None;
    let mut endpoint = "eu".to_string();
    let mut input_price = None;
    let mut output_price = None;
    let mut expected_revision = 0u64;
    let mut context = None;
    let mut arguments = arguments.peekable();
    while let Some(argument) = arguments.next() {
        if argument == "--state-dir" {
            directory = PathBuf::from(arguments.next().ok_or("missing option value")?);
        } else if argument == "--model" {
            model = Some(required_string(arguments.next(), "invalid model")?);
        } else if argument == "--endpoint" {
            endpoint = required_string(arguments.next(), "invalid endpoint")?;
        } else if argument == "--input-price" {
            input_price = Some(required_parse(arguments.next(), "invalid price")?);
        } else if argument == "--output-price" {
            output_price = Some(required_parse(arguments.next(), "invalid price")?);
        } else if argument == "--expected-revision" {
            expected_revision = required_parse(arguments.next(), "invalid revision")?;
        } else if argument == "--context" {
            context = Some(required_parse(arguments.next(), "invalid context")?);
        } else if name.is_none() {
            name = Some(argument.into_string().map_err(|_| "invalid name")?);
        } else {
            return Err("unknown option".into());
        }
    }
    let request = Configure {
        name: name.ok_or("missing bot name")?,
        model: model.ok_or("missing model")?,
        endpoint,
        input_price: input_price.ok_or("missing input price")?,
        output_price: output_price.ok_or("missing output price")?,
        expected_revision,
        context,
    };
    if !matches!(request.endpoint.as_str(), "eu" | "global" | "us") {
        return Err("unknown endpoint".into());
    }
    let address = listen_address(&directory)?;
    let token = sign_in(address)?;
    let outcome = configure_bot(address, token.as_str(), &request);
    let signed_out = sign_out(address, token.as_str());
    outcome?;
    signed_out?;
    Ok(())
}

fn configure_bot(
    address: SocketAddr,
    token: &str,
    request: &Configure,
) -> Result<(), Box<dyn std::error::Error>> {
    let bots = expect_json(address, "GET", "/v1/bots", Some(token), None, 200)?;
    let bot = bots
        .as_array()
        .ok_or("bots")?
        .iter()
        .find(|bot| bot["name"].as_str() == Some(request.name.as_str()))
        .cloned();
    let bot = match bot {
        Some(bot) => bot,
        None => expect_json(
            address,
            "PUT",
            &format!("/v1/bots/{}", uuid::Uuid::new_v4()),
            Some(token),
            Some(&serde_json::json!({"name": request.name})),
            200,
        )?,
    };
    let bot_id = bot["id"].as_str().ok_or("bot")?.to_owned();
    let context_tokens = match request.context {
        Some(tokens) => tokens,
        None => {
            let key = read_password("Mistral API key: ")?;
            mistral_context(&request.endpoint, &request.model, key.as_str())?
        }
    };
    let now = unix_seconds();
    let configured = expect_json(
        address,
        "PUT",
        "/v1/inference-configurations",
        Some(token),
        Some(&serde_json::json!({
            "bot": bot_id,
            "expected_revision": request.expected_revision,
            "policy": {
                "endpoint": request.endpoint,
                "model_id": request.model,
                "context_tokens": context_tokens,
                "acknowledgment": {
                    "model_developer": "Mistral AI",
                    "developer_terms_url": "https://legal.mistral.ai/terms/commercial-terms-of-service/",
                    "privacy_terms_url": "https://legal.mistral.ai/terms/privacy-policy/",
                    "acknowledged_at": now,
                    "release": {"type": "stable"}
                },
                "prices": {
                    "input_microusd_per_million_tokens": request.input_price,
                    "output_microusd_per_million_tokens": request.output_price,
                    "effective_at": now - 60,
                    "expires_at": now + 30 * 86400,
                    "billing": "text_tokens_only_at_acknowledged_ceilings"
                }
            }
        })),
        200,
    )?;
    println!("configured {configured}");
    let revision = configured["revision"].as_u64().ok_or("revision")?;
    let (status, body) = http_json(
        address,
        "POST",
        "/v1/inference-configurations/verify",
        Some(token),
        Some(&serde_json::json!({"bot": bot_id, "revision": revision})),
        ADMIN_TIMEOUT,
    )?;
    if status != 200 {
        match body["error"].as_str() {
            Some(code) => eprintln!("{code}"),
            None => eprintln!("http {status}"),
        }
        return Err("request failed".into());
    }
    println!("{body}");
    Ok(())
}

fn only_state_dir(
    arguments: impl Iterator<Item = OsString>,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let mut directory = PathBuf::from(".state");
    let mut arguments = arguments.peekable();
    while let Some(argument) = arguments.next() {
        if argument == "--state-dir" {
            directory = PathBuf::from(arguments.next().ok_or("missing option value")?);
        } else {
            return Err("unknown option".into());
        }
    }
    Ok(directory)
}

fn listeners(directory: &Path) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_slice(&std::fs::read(
        directory.join("listeners.json"),
    )?)?)
}

fn listen_address(directory: &Path) -> Result<SocketAddr, Box<dyn std::error::Error>> {
    listeners(directory)?["listen"]
        .as_str()
        .ok_or("manager is not running")?
        .parse()
        .map_err(Into::into)
}

fn sign_in(address: SocketAddr) -> Result<Zeroizing<String>, Box<dyn std::error::Error>> {
    let password = read_password("Password: ")?;
    let body = expect_json(
        address,
        "POST",
        "/v1/sessions",
        None,
        Some(&serde_json::json!({"password": password.as_str()})),
        201,
    )?;
    Ok(Zeroizing::new(
        body["token"].as_str().ok_or("sign-in failed")?.to_owned(),
    ))
}

fn sign_out(address: SocketAddr, token: &str) -> Result<(), Box<dyn std::error::Error>> {
    http_json(
        address,
        "DELETE",
        "/v1/session",
        Some(token),
        Some(&serde_json::json!({})),
        ADMIN_TIMEOUT,
    )?;
    Ok(())
}

fn required_string(
    value: Option<OsString>,
    error: &'static str,
) -> Result<String, Box<dyn std::error::Error>> {
    Ok(value
        .ok_or("missing option value")?
        .into_string()
        .map_err(|_| error)?)
}

fn required_parse<T: std::str::FromStr>(
    value: Option<OsString>,
    error: &'static str,
) -> Result<T, Box<dyn std::error::Error>> {
    value
        .ok_or("missing option value")?
        .to_str()
        .ok_or(error)?
        .parse()
        .map_err(|_| error.into())
}

fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

fn mistral_context(
    endpoint: &str,
    model: &str,
    key: &str,
) -> Result<u32, Box<dyn std::error::Error>> {
    let host = match endpoint {
        "eu" => "api.eu.mistral.ai",
        "global" => "api.mistral.ai",
        "us" => "api.us.mistral.ai",
        _ => return Err("unknown endpoint".into()),
    };
    let url = format!("https://{host}/v1/models/{model}");
    let value = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            reqwest::Client::builder()
                .https_only(true)
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(15))
                .build()?
                .get(url)
                .bearer_auth(key)
                .send()
                .await?
                .error_for_status()?
                .json::<serde_json::Value>()
                .await
        })?;
    let tokens = value["max_context_length"]
        .as_u64()
        .ok_or("missing max_context_length")?;
    u32::try_from(tokens).map_err(|_| "invalid context".into())
}

fn read_line(prompt: &str) -> Result<String, Box<dyn std::error::Error>> {
    eprint!("{prompt}");
    std::io::stderr().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    trim_line(&mut line);
    Ok(line)
}

fn read_password(prompt: &str) -> Result<Zeroizing<String>, Box<dyn std::error::Error>> {
    eprint!("{prompt}");
    std::io::stderr().flush()?;
    let mut line = String::new();
    if std::io::stdin().is_terminal() {
        let fd = libc::STDIN_FILENO;
        let mut old = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(fd, &mut old) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut quiet = old;
        quiet.c_lflag &= !libc::ECHO;
        if unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &quiet) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let read = std::io::stdin().lock().read_line(&mut line);
        unsafe {
            libc::tcsetattr(fd, libc::TCSANOW, &old);
        }
        eprintln!();
        read?;
    } else {
        std::io::stdin().lock().read_line(&mut line)?;
    }
    trim_line(&mut line);
    Ok(Zeroizing::new(line))
}

fn trim_line(line: &mut String) {
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
}

fn expect_json(
    address: SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&serde_json::Value>,
    want: u16,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let (status, value) = http_json(address, method, path, token, body, ADMIN_TIMEOUT)?;
    if status != want {
        match value["error"].as_str() {
            Some(code) => eprintln!("{code}"),
            None => eprintln!("http {status}"),
        }
        return Err("request failed".into());
    }
    Ok(value)
}

fn http_json(
    address: SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&serde_json::Value>,
    timeout: Duration,
) -> Result<(u16, serde_json::Value), Box<dyn std::error::Error>> {
    let payload = body.map(serde_json::to_vec).transpose()?;
    let mut stream = TcpStream::connect(address)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let mut request =
        format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n");
    if let Some(payload) = &payload {
        request.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            payload.len()
        ));
    }
    if let Some(token) = token {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes())?;
    if let Some(payload) = payload {
        stream.write_all(&payload)?;
    }
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    let text = String::from_utf8(response)?;
    let (head, body) = text.split_once("\r\n\r\n").ok_or("short response")?;
    let status = head
        .split_whitespace()
        .nth(1)
        .ok_or("short response")?
        .parse()?;
    let value = if body.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(body)?
    };
    Ok((status, value))
}
