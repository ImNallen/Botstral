use botstral_link::tls;
use botstral_manager::{LocalState, WorkerTls, protect_process, serve_with_worker};
use std::{
    io::{BufRead, IsTerminal, Read, Write},
    net::{SocketAddr, TcpStream},
    path::PathBuf,
    process::ExitCode,
    time::Duration,
};
use zeroize::Zeroizing;

fn main() -> ExitCode {
    if botstral_link::release::print_version("manager") {
        return ExitCode::SUCCESS;
    }
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if std::env::args().nth(1).as_deref() == Some("invite") => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
        Err(_) => {
            eprintln!("manager startup or shutdown failed");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    protect_process()?;
    let mut arguments = std::env::args_os().skip(1);
    if arguments.next().as_deref() == Some("invite".as_ref()) {
        return invite(arguments);
    }
    let mut directory = PathBuf::from(".state");
    let mut listen: SocketAddr = "127.0.0.1:43120".parse()?;
    let mut setup_listen: SocketAddr = "127.0.0.1:43121".parse()?;
    let mut worker_listen = None;
    let mut arguments = std::env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--help" {
            println!(
                "botstral-manager [--state-dir PATH] [--listen LOOPBACK:PORT] [--setup-listen LOOPBACK:PORT] [--worker-listen ADDRESS:PORT]\nbotstral-manager invite worker [--state-dir PATH]\nbotstral-manager invite device --label LABEL [--state-dir PATH]"
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

fn invite(
    arguments: impl Iterator<Item = std::ffi::OsString>,
) -> Result<(), Box<dyn std::error::Error>> {
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
    let listeners: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("listeners.json"))?)?;
    let address: SocketAddr = listeners["listen"]
        .as_str()
        .ok_or("manager is not running")?
        .parse()?;
    let password = read_password()?;
    let (status, body) = http_json(
        address,
        "POST",
        "/v1/sessions",
        None,
        &serde_json::json!({"password": password.as_str()}),
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
    let (status, body) = http_json(address, "POST", path, Some(token), &payload)?;
    let signed_out = http_json(
        address,
        "DELETE",
        "/v1/session",
        Some(token),
        &serde_json::json!({}),
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

fn read_password() -> Result<Zeroizing<String>, Box<dyn std::error::Error>> {
    eprint!("Password: ");
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
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
    Ok(Zeroizing::new(line))
}

fn http_json(
    address: SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: &serde_json::Value,
) -> Result<(u16, serde_json::Value), Box<dyn std::error::Error>> {
    let payload = serde_json::to_vec(body)?;
    let mut stream = TcpStream::connect(address)?;
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    stream.set_write_timeout(Some(Duration::from_secs(15)))?;
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        payload.len()
    );
    if let Some(token) = token {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes())?;
    stream.write_all(&payload)?;
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
