use botstral_link::tls;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::ExitCode,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub manager: String,
    pub pin: botstral_link::CertificateFingerprint,
    pub client_cert: PathBuf,
    pub client_key: PathBuf,
}

struct StateDir {
    path: PathBuf,
    _lock: File,
}
impl StateDir {
    fn open(path: PathBuf) -> io::Result<Self> {
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                fs::DirBuilder::new().mode(0o700).create(&path)?
            }
            Err(e) => return Err(e),
            Ok(_) => {}
        }
        let meta = fs::symlink_metadata(&path)?;
        if !meta.is_dir()
            || meta.mode() & 0o7777 != 0o700
            || meta.uid() != unsafe { libc::geteuid() }
        {
            return Err(io::Error::other("unsafe state directory"));
        }
        let lock_path = path.join("agent.lock");
        let lock = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&lock_path)?;
        tls::private_bytes(&lock_path)?;
        lock.try_lock()?;
        Ok(Self {
            path: path.canonicalize()?,
            _lock: lock,
        })
    }
    fn save(&self, config: &Config) -> io::Result<()> {
        let path = self.path.join("config.json.tmp");
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)?;
        tls::private_bytes(&path)?;
        file.write_all(&serde_json::to_vec(config)?)?;
        file.sync_all()?;
        fs::rename(path, self.path.join("config.json"))
    }
}

pub fn entry(test_worker: bool) -> ExitCode {
    unsafe {
        libc::umask(0o077);
        let limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if libc::setrlimit(libc::RLIMIT_CORE, &limit) != 0 {
            return ExitCode::FAILURE;
        }
    }
    match run_cli(test_worker) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => {
            eprintln!("worker operation failed");
            ExitCode::FAILURE
        }
    }
}

fn run_cli(test_worker: bool) -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let command = args.next().ok_or("missing command")?;
    if command == "process-helper" {
        let path = args.next().ok_or("missing helper directory")?;
        if args.next().is_some() {
            return Err("extra helper argument".into());
        }
        return crate::process::helper(Path::new(&path)).map_err(Into::into);
    }
    if command == "--help" {
        println!(
            "botstral-agent identity --client-cert PATH\nbotstral-agent idle --state-dir PATH\nbotstral-agent join --state-dir PATH CODE\nbotstral-agent run|check --state-dir PATH"
        );
        return Ok(());
    }
    if command == "idle" {
        let mut state_dir = None;
        while let Some(key) = args.next() {
            if key == "--state-dir" {
                if state_dir
                    .replace(args.next().ok_or("missing option value")?)
                    .is_some()
                {
                    return Err("duplicate option".into());
                }
            } else {
                return Err("unknown option".into());
            }
        }
        match crate::journal::host_idle(Path::new(&state_dir.ok_or("missing option")?))? {
            None => println!("{{\"idle\":true}}"),
            Some(reason) => println!("{{\"idle\":false,\"reason\":\"{reason}\"}}"),
        }
        return Ok(());
    }
    if command == "identity" {
        if args.next().as_deref() != Some("--client-cert") {
            return Err("missing option".into());
        }
        let cert = args.next().ok_or("missing option value")?;
        if args.next().is_some() {
            return Err("extra argument".into());
        }
        let certs = tls::certificates(Path::new(&cert))?;
        let identity = botstral_link::IdentityPublicKey::from_certificate(certs[0].as_ref())
            .map_err(|_| "invalid identity")?;
        println!("{}", identity.fingerprint().as_str());
        return Ok(());
    }
    let mut state_dir = None;
    let mut code = None;
    if command == "join" {
        while let Some(key) = args.next() {
            if key == "--state-dir" {
                if state_dir
                    .replace(args.next().ok_or("missing option value")?)
                    .is_some()
                {
                    return Err("duplicate option".into());
                }
            } else if code.is_none() && !key.starts_with('-') {
                code = Some(key);
            } else {
                return Err("unknown option".into());
            }
        }
    } else {
        while let Some(key) = args.next() {
            if key != "--state-dir" {
                return Err("unknown option".into());
            }
            if state_dir
                .replace(args.next().ok_or("missing option value")?)
                .is_some()
            {
                return Err("duplicate option".into());
            }
        }
    }
    let state = StateDir::open(state_dir.ok_or("missing option")?.into())?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    if command == "join" {
        let parsed = botstral_link::JoinCode::parse(&code.ok_or("missing join code")?)
            .map_err(|_| "invalid invitation")?;
        if parsed.role() != botstral_link::PeerRole::Worker {
            return Err("worker join code required".into());
        }
        let identity = tls::ensure_identity(&state.path, "worker-identity")?;
        let config = Config {
            manager: parsed.endpoint().to_wss(),
            pin: parsed.pin().fingerprint().clone(),
            client_cert: identity.certificate,
            client_key: identity.key,
        };
        crate::transport::endpoint(&config)?;
        state.save(&config)?;
        let secret = parsed.secret_text();
        runtime.block_on(crate::transport::enroll(&config, secret.as_str()))?;
        println!("worker enrolled");
    } else if command == "run" || command == "check" {
        let config: Config =
            serde_json::from_slice(&tls::private_bytes(&state.path.join("config.json"))?)?;
        runtime.block_on(crate::transport::run(
            config,
            &state.path,
            test_worker,
            command == "check",
        ))?;
    } else {
        return Err("unknown command".into());
    }
    Ok(())
}
