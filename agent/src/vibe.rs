use crate::process;
use botstral_link::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read},
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Configuration {
    pub account: ProcessAccount,
    pub directory: PathBuf,
    pub manifest: PathBuf,
    pub runtime: CertificateFingerprint,
    pub ceilings: ProcessLimits,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Launch {
    pub spec: VibeSpec,
    pub task: TaskId,
    pub execution: ExecutionId,
    pub runtime: PathBuf,
    pub manifest: PathBuf,
    pub home: PathBuf,
    pub outputs: PathBuf,
    pub transport: PathBuf,
    pub directory: PathBuf,
    #[serde(default)]
    pub connector: Option<crate::connector::Configuration>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeManifest {
    version: String,
    distributions: BTreeMap<String, String>,
    files: Vec<(String, CertificateFingerprint)>,
}
impl Configuration {
    pub fn read(root: &Path) -> io::Result<Option<Self>> {
        let path = root.join("vibe.json");
        if !path.try_exists()? {
            return Ok(None);
        }
        let config: Self = process::read(&path)?;
        config.ceilings.validate().map_err(io::Error::other)?;
        if config.account.uid == 0
            || config.account.uid != unsafe { libc::geteuid() }
            || config.account.gid != unsafe { libc::getegid() }
            || !config.directory.is_absolute()
            || !config.manifest.is_absolute()
        {
            return Err(io::Error::other("coding configuration mismatch"));
        }
        Ok(Some(config))
    }
    pub fn verify(&self) -> io::Result<()> {
        verify(&self.directory, &self.manifest, &self.runtime)
    }
    pub fn permits(&self, spec: &VibeSpec) -> bool {
        self.account == spec.policy.account
            && self.runtime == spec.policy.runtime
            && self.ceilings.permits(&spec.policy.resources)
    }
    pub fn launch(
        &self,
        spec: &VibeSpec,
        root: &Path,
        directory: &Path,
        body: &CommandBody,
    ) -> Launch {
        Launch {
            spec: spec.clone(),
            task: body.task,
            execution: body.execution,
            runtime: self.directory.clone(),
            manifest: self.manifest.clone(),
            home: root
                .join("coding")
                .join(spec.policy.workspace.id.to_string()),
            outputs: directory.join("outputs"),
            transport: root.join("config.json"),
            directory: directory.to_owned(),
            connector: crate::connector::Configuration::read(root).ok().flatten(),
        }
    }
}
impl Launch {
    pub fn prepare(&self) -> io::Result<()> {
        let workspace = Path::new(&self.spec.policy.workspace.path);
        if workspace.canonicalize()? != workspace || !workspace.is_dir() {
            return Err(io::Error::other("coding workspace is not canonical"));
        }
        let root = self.home.parent().unwrap();
        if !root.try_exists()? {
            fs::DirBuilder::new().mode(0o700).create(root)?;
        }
        fs::write(
            self.directory.join("connector-runtime.py"),
            crate::connector::SCRIPT,
        )?;
        fs::write(
            self.directory.join("vibe-runtime.py"),
            include_bytes!("../vibe/runtime.py"),
        )?;
        process::write(&self.directory.join("vibe-launch.json"), self)
    }
    pub fn lower(&self) -> ProcessSpec {
        ProcessSpec {
            profile: ProcessProfile::LinuxSystemdV1,
            account: self.spec.policy.account,
            executable: self
                .runtime
                .join("bin/python3")
                .to_string_lossy()
                .into_owned(),
            args: vec![
                self.directory
                    .join("vibe-runtime.py")
                    .to_string_lossy()
                    .into_owned(),
                self.directory
                    .join("vibe-launch.json")
                    .to_string_lossy()
                    .into_owned(),
            ],
            environment: BTreeMap::from([
                ("LANG".into(), "C.UTF-8".into()),
                ("PATH".into(), "/usr/bin:/bin".into()),
                ("PYTHONDONTWRITEBYTECODE".into(), "1".into()),
            ]),
            cwd: self.spec.policy.workspace.path.clone(),
            deadline: self.spec.limits.deadline,
            limits: self.spec.policy.resources.clone(),
        }
    }
    pub fn verify(&self, lowered: &ProcessSpec) -> io::Result<()> {
        self.spec.validate().map_err(io::Error::other)?;
        if self.lower() != *lowered
            || fs::read(self.directory.join("vibe-runtime.py"))?
                != include_bytes!("../vibe/runtime.py")
        {
            return Err(io::Error::other("coding launch mismatch"));
        }
        let stored: Self = process::read(&self.directory.join("vibe-launch.json"))?;
        if serde_json::to_vec(&stored)? != serde_json::to_vec(self)? {
            return Err(io::Error::other("coding launch changed"));
        }
        if fs::read(self.directory.join("connector-runtime.py"))? != crate::connector::SCRIPT {
            return Err(io::Error::other("connector helper changed"));
        }
        if let Some(policy) = &self.spec.policy.connectors {
            let config = self
                .connector
                .as_ref()
                .ok_or_else(|| io::Error::other("connector configuration missing"))?;
            if !config.permits(&policy.runner) {
                return Err(io::Error::other("connector policy mismatch"));
            }
            config.verify()?;
        }
        verify(&self.runtime, &self.manifest, &self.spec.policy.runtime)
    }
}
fn verify(root: &Path, path: &Path, expected: &CertificateFingerprint) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > 2 * 1024 * 1024 {
        return Err(io::Error::other("coding manifest bound"));
    }
    let bytes = fs::read(path)?;
    if CertificateFingerprint::from_der(&bytes) != *expected {
        return Err(io::Error::other("coding runtime identity"));
    }
    let manifest: RuntimeManifest = serde_json::from_slice(&bytes)?;
    if manifest.version != VIBE_VERSION
        || manifest
            .distributions
            .get("mistral-vibe")
            .map(String::as_str)
            != Some(VIBE_VERSION)
        || manifest.files.is_empty()
        || manifest.files.len() > 16000
    {
        return Err(io::Error::other("coding runtime version"));
    }
    for (name, digest) in manifest.files {
        if Path::new(&name).is_absolute()
            || Path::new(&name)
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(io::Error::other("coding runtime path"));
        }
        let mut file = fs::File::open(root.join(name))?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::other("coding runtime file"));
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
            return Err(io::Error::other("coding runtime changed"));
        }
    }
    Ok(())
}
