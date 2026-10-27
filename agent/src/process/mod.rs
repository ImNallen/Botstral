mod helper;
mod systemd;

use botstral_link::*;
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};
pub(crate) use systemd::{Backend, EmptyScopeProof};

const FILE_LIMIT: usize = 8192;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Configuration {
    pub profile: ProcessProfile,
    pub account: ProcessAccount,
    pub ceilings: ProcessLimits,
}
impl Configuration {
    pub fn read(directory: &Path) -> io::Result<Option<Self>> {
        let path = directory.join("process.json");
        if !path.try_exists()? {
            return Ok(None);
        }
        let value: Self = read(&path)?;
        value.ceilings.validate().map_err(io::Error::other)?;
        if value.account.uid == 0
            || value.account.uid != unsafe { libc::geteuid() }
            || value.account.gid != unsafe { libc::getegid() }
        {
            return Err(io::Error::other("process account mismatch"));
        }
        Ok(Some(value))
    }
    pub fn permits(&self, spec: &ProcessSpec) -> bool {
        self.profile == spec.profile
            && self.account == spec.account
            && self.ceilings.permits(&spec.limits)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    pub journal: JournalId,
    pub execution: ExecutionId,
    pub host: CertificateFingerprint,
    pub boot: String,
    pub spec: ProcessSpec,
    pub expires_at: i64,
}
impl Manifest {
    pub fn unit(&self) -> String {
        format!("botstral-{}-{}.service", self.journal, self.execution)
    }
    pub fn digest(&self) -> io::Result<CertificateFingerprint> {
        Ok(CertificateFingerprint::from_der(&serde_json::to_vec(self)?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Ready {
    pub scope: ProcessScope,
    pub manifest: CertificateFingerprint,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Observation {
    pub ready: Ready,
    pub evidence: ProcessEvidence,
}

pub(crate) fn read<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<T> {
    tls::check_private_file(path)?;
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let mut bytes = Vec::new();
    (&mut file)
        .take((FILE_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > FILE_LIMIT {
        return Err(io::Error::other("process file bound"));
    }
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}
pub(crate) fn write<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > FILE_LIMIT {
        return Err(io::Error::other("process file bound"));
    }
    let temporary = path.with_extension("tmp");
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&temporary)?;
    f.write_all(&bytes)?;
    f.sync_all()?;
    fs::rename(temporary, path)?;
    fs::File::open(
        path.parent()
            .ok_or_else(|| io::Error::other("process path"))?,
    )?
    .sync_all()
}
pub(crate) fn directory(root: &Path, execution: ExecutionId) -> PathBuf {
    root.join("processes").join(execution.to_string())
}
pub(crate) fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64
}
pub(crate) fn helper(path: &Path) -> io::Result<()> {
    helper::run(path)
}
