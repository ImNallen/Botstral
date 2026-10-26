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
    pub manager_ca: PathBuf,
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
    if command == "--help" {
        println!(
            "botstral-agent identity --client-cert PATH\nbotstral-agent enroll --state-dir PATH --manager wss://HOST:PORT --manager-ca PATH --client-cert PATH --client-key PATH --invitation-file PATH\nbotstral-agent run|check --state-dir PATH"
        );
        return Ok(());
    }
    let mut values = std::collections::HashMap::new();
    while let Some(key) = args.next() {
        if ![
            "--state-dir",
            "--manager",
            "--manager-ca",
            "--client-cert",
            "--client-key",
            "--invitation-file",
        ]
        .contains(&key.as_str())
        {
            return Err("unknown option".into());
        }
        if values
            .insert(key, args.next().ok_or("missing option value")?)
            .is_some()
        {
            return Err("duplicate option".into());
        }
    }
    let required = |key: &str| values.get(key).cloned().ok_or("missing option");
    if command == "identity" {
        let certs = tls::certificates(Path::new(&required("--client-cert")?))?;
        println!(
            "{}",
            botstral_link::CertificateFingerprint::from_der(certs[0].as_ref()).as_str()
        );
        return Ok(());
    }
    let state = StateDir::open(required("--state-dir")?.into())?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    if command == "enroll" {
        let config = Config {
            manager: required("--manager")?,
            manager_ca: fs::canonicalize(required("--manager-ca")?)?,
            client_cert: fs::canonicalize(required("--client-cert")?)?,
            client_key: fs::canonicalize(required("--client-key")?)?,
        };
        crate::transport::endpoint(&config)?;
        state.save(&config)?;
        let secret = tls::private_bytes(Path::new(&required("--invitation-file")?))?;
        runtime.block_on(crate::transport::enroll(&config, &secret))?;
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
