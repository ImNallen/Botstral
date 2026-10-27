use botstral_manager::{LocalState, WorkerTls, protect_process, serve_with_worker};
use std::{net::SocketAddr, path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    if run().is_err() {
        eprintln!("manager startup or shutdown failed");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    protect_process()?;
    let mut directory = PathBuf::from(".state");
    let mut listen: SocketAddr = "127.0.0.1:43120".parse()?;
    let mut setup_listen: SocketAddr = "127.0.0.1:43121".parse()?;
    let mut worker_listen = None;
    let mut worker_cert = None;
    let mut worker_key = None;
    let mut worker_ca = None;
    let mut arguments = std::env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--help" {
            println!(
                "botstral-manager [--state-dir PATH] [--listen LOOPBACK:PORT] [--setup-listen LOOPBACK:PORT] [--worker-listen ADDRESS:PORT --worker-tls-cert PATH --worker-tls-key PATH --worker-client-ca PATH]"
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
        } else if argument == "--worker-tls-cert" || argument == "--public-tls-cert" {
            if worker_cert.is_some() {
                return Err("duplicate public certificate option".into());
            }
            worker_cert = Some(PathBuf::from(value));
        } else if argument == "--worker-tls-key" || argument == "--public-tls-key" {
            if worker_key.is_some() {
                return Err("duplicate public key option".into());
            }
            worker_key = Some(PathBuf::from(value));
        } else if argument == "--worker-client-ca" || argument == "--public-worker-ca" {
            if worker_ca.is_some() {
                return Err("duplicate public worker CA option".into());
            }
            worker_ca = Some(PathBuf::from(value));
        } else {
            return Err("unknown option".into());
        }
    }
    if !listen.ip().is_loopback() || !setup_listen.ip().is_loopback() {
        return Err("unsafe bind address".into());
    }
    let worker_tls = match (worker_listen, worker_cert, worker_key, worker_ca) {
        (None, None, None, None) => None,
        (Some(listen), Some(certificate), Some(key), Some(client_ca)) => Some(WorkerTls {
            listen,
            certificate,
            key,
            client_ca,
        }),
        _ => return Err("worker TLS requires listener, certificate, key, and client CA".into()),
    };
    let state = LocalState::open(&directory)?;
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(serve_with_worker(state, listen, setup_listen, worker_tls))?;
    Ok(())
}
