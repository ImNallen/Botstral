use botstral_manager::{LocalState, protect_process, serve};
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
    let mut arguments = std::env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--help" {
            println!(
                "botstral-manager [--state-dir PATH] [--listen LOOPBACK:PORT] [--setup-listen LOOPBACK:PORT]"
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
        } else {
            return Err("unknown option".into());
        }
    }
    if !listen.ip().is_loopback() || !setup_listen.ip().is_loopback() {
        return Err("unsafe bind address".into());
    }
    let state = LocalState::open(&directory)?;
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(serve(state, listen, setup_listen))?;
    Ok(())
}
