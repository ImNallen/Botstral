use crate::process::{self, Backend, Manifest};
use botstral_link::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Configuration {
    pub account: ProcessAccount,
    pub runtime: ComputerRuntime,
    pub directory: PathBuf,
    pub ceilings: ProcessLimits,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Launch {
    pub spec: ComputerSpec,
    pub runtime_directory: PathBuf,
    pub profile: PathBuf,
    pub outputs: PathBuf,
    pub specification: PathBuf,
    pub probe: bool,
}
impl Configuration {
    pub fn read(root: &Path) -> io::Result<Option<Self>> {
        let file = root.join("computer.json");
        if !file.try_exists()? {
            return Ok(None);
        }
        let config: Self = process::read(&file)?;
        if config.account.uid == 0
            || config.account.uid != unsafe { libc::geteuid() }
            || config.account.gid != unsafe { libc::getegid() }
            || !config.directory.is_absolute()
        {
            return Err(io::Error::other("computer account or runtime mismatch"));
        }
        Ok(Some(config))
    }
    pub fn permits(&self, spec: &ComputerSpec) -> bool {
        self.account == spec.account
            && self.runtime == spec.runtime
            && self.ceilings.permits(&spec.limits)
    }
    pub fn verify(&self) -> io::Result<()> {
        verify(&self.directory, &self.runtime)
    }
    pub fn launch(
        &self,
        spec: &ComputerSpec,
        root: &Path,
        directory: &Path,
        probe: bool,
    ) -> Launch {
        Launch {
            spec: spec.clone(),
            runtime_directory: self.directory.clone(),
            profile: if probe {
                directory.join("profile")
            } else {
                root.join("automation-profile")
            },
            outputs: directory.join("outputs"),
            specification: directory.join("computer.json"),
            probe,
        }
    }
    pub async fn probe(
        &self,
        backend: &Backend,
        root: &Path,
        journal: JournalId,
    ) -> io::Result<()> {
        self.verify()?;
        verify_profile(&root.join("automation-profile"))?;
        let directory = root.join("computer-probe");
        if directory.try_exists()? {
            let manifest: Manifest = process::read(&directory.join("manifest.json"))?;
            let ready = backend.observe(&directory, &manifest).await?;
            backend.stop(&ready.scope).await?;
            fs::remove_dir_all(&directory)?;
        }
        fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let spec = ComputerSpec {
            profile: ComputerProfile::LinuxPlaywrightV1,
            account: self.account,
            runtime: self.runtime.clone(),
            deadline: process::now() + 30,
            limits: self.ceilings.clone(),
            viewport: Viewport {
                width: 800,
                height: 600,
            },
            operations: vec![ComputerOperation::Screenshot {
                output: "probe".into(),
            }],
            outputs: vec![OutputDeclaration {
                slot: "probe".into(),
                name: "probe.png".into(),
                media_type: MediaType::Png,
                max_bytes: MAX_ARTIFACT_BYTES,
            }],
        };
        spec.validate().map_err(io::Error::other)?;
        let launch = self.launch(&spec, root, &directory, true);
        launch.prepare()?;
        let manifest = Manifest {
            journal,
            execution: ExecutionId::new(),
            host: backend.host.clone(),
            boot: backend.boot.clone(),
            spec: launch.lower(),
            computer: Some(launch),
            expires_at: process::now() + 30,
        };
        process::write(&directory.join("manifest.json"), &manifest)?;
        let ready = backend.start(&directory, &manifest).await?;
        backend.release(&directory, &ready)?;
        let until = tokio::time::Instant::now() + Duration::from_secs(20);
        while !directory.join("result.json").try_exists()? && tokio::time::Instant::now() < until {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let result = backend.observation(&directory, &ready, true)?;
        backend.stop(&ready.scope).await?;
        if !result.is_some_and(|e| {
            e.result
                .is_some_and(|r| r.exit == ProcessExit::Exited { code: 0 })
        }) {
            return Err(io::Error::other("packaged browser probe failed"));
        }
        fs::remove_dir_all(directory)?;
        Ok(())
    }
}
impl Launch {
    pub fn prepare(&self) -> io::Result<()> {
        verify_profile(&self.profile)?;
        fs::DirBuilder::new().mode(0o700).create(&self.outputs)?;
        if !self.profile.try_exists()? {
            fs::DirBuilder::new().mode(0o700).create(&self.profile)?;
        }
        for directory in [
            self.profile.join(".cache"),
            self.profile.join(".cache/fontconfig"),
        ] {
            if !directory.try_exists()? {
                fs::DirBuilder::new().mode(0o700).create(&directory)?;
            }
        }
        process::write(&self.specification, &self.spec)
    }
    pub fn lower(&self) -> ProcessSpec {
        ProcessSpec {
            profile: ProcessProfile::LinuxSystemdV1,
            account: self.spec.account,
            executable: self
                .runtime_directory
                .join("node/bin/node")
                .to_string_lossy()
                .into_owned(),
            args: vec![
                self.runtime_directory
                    .join("run.mjs")
                    .to_string_lossy()
                    .into_owned(),
                if self.probe { "probe" } else { "run" }.into(),
                self.specification.to_string_lossy().into_owned(),
                self.profile.to_string_lossy().into_owned(),
                self.outputs.to_string_lossy().into_owned(),
            ],
            environment: BTreeMap::from([
                (
                    "PLAYWRIGHT_BROWSERS_PATH".into(),
                    self.runtime_directory
                        .join("browsers")
                        .to_string_lossy()
                        .into_owned(),
                ),
                ("HOME".into(), self.profile.to_string_lossy().into_owned()),
                (
                    "XDG_CACHE_HOME".into(),
                    self.profile.join(".cache").to_string_lossy().into_owned(),
                ),
                ("LANG".into(), "C.UTF-8".into()),
            ]),
            cwd: self.runtime_directory.to_string_lossy().into_owned(),
            deadline: self.spec.deadline,
            limits: self.spec.limits.clone(),
        }
    }
    pub fn verify(&self, lowered: &ProcessSpec) -> io::Result<()> {
        self.spec.validate().map_err(io::Error::other)?;
        if self.lower() != *lowered {
            return Err(io::Error::other("computer launch mismatch"));
        }
        verify(&self.runtime_directory, &self.spec.runtime)
    }
}
fn verify(root: &Path, identity: &ComputerRuntime) -> io::Result<()> {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return Err(io::Error::other("unsupported computer host"));
    }
    let path = root.join("runtime.json");
    let meta = fs::symlink_metadata(&path)?;
    if !meta.is_file() || meta.len() > 1024 * 1024 {
        return Err(io::Error::other("runtime manifest bound"));
    }
    let bytes = fs::read(&path)?;
    if CertificateFingerprint::from_der(&bytes) != identity.sha256 {
        return Err(io::Error::other("runtime identity mismatch"));
    }
    let files: Vec<(String, CertificateFingerprint)> = serde_json::from_slice(&bytes)?;
    if files.len() > 8192 {
        return Err(io::Error::other("runtime file count bound"));
    }
    for (name, digest) in files {
        if Path::new(&name).is_absolute()
            || Path::new(&name)
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(io::Error::other("runtime path"));
        }
        let mut file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(root.join(name))?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::other("runtime regular file required"));
        }
        let mut hash = Sha256::new();
        let mut buffer = [0; 65536];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        if format!("{:x}", hash.finalize()) != digest.as_str() {
            return Err(io::Error::other("runtime content changed"));
        }
    }
    if fs::read(root.join("run.mjs"))? != include_bytes!("../runtime/run.mjs")
        || fs::read(root.join("package-lock.json"))?
            != include_bytes!("../runtime/package-lock.json")
    {
        return Err(io::Error::other("packaged runtime mismatch"));
    }
    Ok(())
}

fn verify_profile(profile: &Path) -> io::Result<()> {
    if !profile.try_exists()? {
        return Ok(());
    }
    if !fs::symlink_metadata(profile)?.is_dir() {
        return Err(io::Error::other("automation profile directory required"));
    }
    for name in ["Local State", "Default/Preferences"] {
        let path = profile.join(name);
        if path.try_exists()? {
            let mut file = fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(&path)?;
            if !file.metadata()?.is_file() || file.metadata()?.len() > 8 * 1024 * 1024 {
                return Err(io::Error::other("profile metadata bound"));
            }
            let mut bytes = Vec::new();
            file.by_ref()
                .take(8 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            serde_json::from_slice::<serde_json::Value>(&bytes).map_err(io::Error::other)?;
        }
    }
    for name in ["Default/Cookies", "Default/Network/Cookies"] {
        let path = profile.join(name);
        if path.try_exists()? {
            if !fs::symlink_metadata(&path)?.is_file() {
                return Err(io::Error::other("profile cookies regular file required"));
            }
            let db = rusqlite::Connection::open_with_flags(
                path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                    | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
            .map_err(io::Error::other)?;
            let check: String = db
                .query_row("PRAGMA quick_check(1)", [], |r| r.get(0))
                .map_err(io::Error::other)?;
            if check != "ok" {
                return Err(io::Error::other("profile cookies corrupt"));
            }
        }
    }
    Ok(())
}
