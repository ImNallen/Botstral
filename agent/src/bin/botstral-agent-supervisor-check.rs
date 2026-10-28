use std::{path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("process-helper") {
        return botstral_agent::entry(false);
    }
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--scratch")) {
        eprintln!("botstral-agent-supervisor-check --scratch NEW_ABSOLUTE_PATH");
        return ExitCode::FAILURE;
    }
    let Some(path) = args.next().map(PathBuf::from) else {
        return ExitCode::FAILURE;
    };
    if args.next().is_some() {
        return ExitCode::FAILURE;
    }
    let result = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .and_then(|runtime| runtime.block_on(botstral_agent::maintenance::check_supervisor(&path)));
    match result {
        Ok(()) => {
            println!("production systemd backend launch and empty-scope stop proved");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("supervisor check refused: {error}");
            ExitCode::FAILURE
        }
    }
}
