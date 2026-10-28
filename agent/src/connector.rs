use crate::process;
use botstral_link::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

pub(crate) const SCRIPT: &[u8] = include_bytes!("../connector/runtime.py");
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Configuration {
    pub python: PathBuf,
    pub python_sha256: CertificateFingerprint,
    pub runtime: CertificateFingerprint,
    pub account: ProcessAccount,
    pub ceilings: ProcessLimits,
}
impl Configuration {
    pub fn read(root: &Path) -> io::Result<Option<Self>> {
        let path = root.join("connector.json");
        if !path.try_exists()? {
            return Ok(None);
        }
        let config: Self = process::read(&path)?;
        config.ceilings.validate().map_err(io::Error::other)?;
        if config.account.uid == 0
            || config.account.uid != unsafe { libc::geteuid() }
            || config.account.gid != unsafe { libc::getegid() }
            || !config.python.is_absolute()
        {
            return Err(io::Error::other("connector configuration mismatch"));
        }
        config.verify()?;
        Ok(Some(config))
    }
    pub fn verify(&self) -> io::Result<()> {
        let mut file = fs::File::open(&self.python)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::other("connector interpreter"));
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
        let mut identity = Sha256::new();
        identity.update(SCRIPT);
        identity.update(self.python_sha256.as_str().as_bytes());
        if format!("{:x}", hash.finalize()) != self.python_sha256.as_str()
            || format!("{:x}", identity.finalize()) != self.runtime.as_str()
        {
            return Err(io::Error::other("connector runtime changed"));
        }
        Ok(())
    }
    pub fn permits(&self, runner: &ConnectorRunner) -> bool {
        self.account == runner.account
            && self.runtime == runner.runtime
            && self.ceilings.permits(&runner.limits)
    }
    pub fn launch(
        &self,
        spec: &ConnectorSpec,
        root: &Path,
        directory: &Path,
        body: &CommandBody,
    ) -> Launch {
        Launch {
            spec: spec.clone(),
            configuration: self.clone(),
            task: body.task,
            execution: body.execution,
            directory: directory.into(),
            transport: root.join("config.json"),
            outputs: directory.join("outputs"),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Launch {
    pub spec: ConnectorSpec,
    pub configuration: Configuration,
    pub task: TaskId,
    pub execution: ExecutionId,
    pub directory: PathBuf,
    pub transport: PathBuf,
    pub outputs: PathBuf,
}
impl Launch {
    pub fn prepare(&self) -> io::Result<()> {
        fs::write(self.directory.join("connector-runtime.py"), SCRIPT)?;
        process::write(&self.directory.join("connector-launch.json"), self)
    }
    pub fn lower(&self) -> ProcessSpec {
        ProcessSpec {
            profile: ProcessProfile::LinuxSystemdV1,
            account: self.spec.runner.account,
            executable: self.configuration.python.to_string_lossy().into(),
            args: vec![
                "-I".into(),
                self.directory
                    .join("connector-runtime.py")
                    .to_string_lossy()
                    .into(),
                self.directory
                    .join("connector-launch.json")
                    .to_string_lossy()
                    .into(),
            ],
            environment: BTreeMap::from([
                ("PATH".into(), "/usr/bin:/bin".into()),
                ("LANG".into(), "C.UTF-8".into()),
            ]),
            cwd: self.directory.to_string_lossy().into(),
            deadline: self.spec.deadline,
            limits: self.spec.runner.limits.clone(),
        }
    }
    pub fn verify(&self, lowered: &ProcessSpec) -> io::Result<()> {
        self.spec.validate().map_err(io::Error::other)?;
        if self.lower() != *lowered
            || fs::read(self.directory.join("connector-runtime.py"))? != SCRIPT
        {
            return Err(io::Error::other("connector launch mismatch"));
        }
        let saved: Self = process::read(&self.directory.join("connector-launch.json"))?;
        if serde_json::to_vec(&saved)? != serde_json::to_vec(self)? {
            return Err(io::Error::other("connector launch changed"));
        }
        self.configuration.verify()
    }
}

pub(crate) struct Settlement {
    pub task: TaskId,
    pub execution: ExecutionId,
    pub id: ConnectorCallId,
    pub outcome: ConnectorOutcome,
    pub acknowledgment: PathBuf,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Captured {
    digest: CertificateFingerprint,
    phase: String,
    outcome: Option<ConnectorOutcome>,
}
pub(crate) fn captured(directory: &Path, body: &CommandBody) -> io::Result<Vec<Settlement>> {
    let directory = directory.join("connector-calls");
    if !directory.try_exists()? {
        return Ok(vec![]);
    }
    let mut settlements = vec![];
    for entry in fs::read_dir(directory)?.take(64) {
        let path = entry?.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let acknowledgment = path.with_extension("ack");
        if acknowledgment.try_exists()? {
            continue;
        }
        tls::check_private_file(&path)?;
        let mut file = fs::File::open(&path)?;
        let mut bytes = Vec::new();
        file.by_ref()
            .take((MAX_CONNECTOR_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_CONNECTOR_BYTES {
            return Err(io::Error::other("connector evidence bound"));
        }
        let saved: Captured = serde_json::from_slice(&bytes)?;
        let Some(outcome) = saved.outcome else {
            continue;
        };
        if saved.phase != "captured" {
            continue;
        }
        if let WorkSpec::Connector { spec } = &body.work
            && spec.operation.digest() != saved.digest
        {
            return Err(io::Error::other("connector evidence identity"));
        }
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| io::Error::other("connector evidence name"))?
            .parse()
            .map_err(io::Error::other)?;
        settlements.push(Settlement {
            task: body.task,
            execution: body.execution,
            id,
            outcome,
            acknowledgment,
        });
        if settlements.len() == MAX_CONNECTOR_CALLS as usize {
            break;
        }
    }
    Ok(settlements)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn recovery_returns_only_saved_outcomes_bound_to_the_original_command() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("connector-calls");
        fs::create_dir(&directory).unwrap();
        let operation = ConnectorOperation::Discover {
            connection: ConnectionVersion {
                connection: ConnectionId::new(),
                revision: 1,
                credential_epoch: 1,
            },
        };
        let body = CommandBody {
            id: CommandId::new(),
            owner: OwnerId::new(),
            task: TaskId::new(),
            bot: BotId::new(),
            worker: WorkerId::new(),
            session: ComputerSessionId::new(),
            operation: OperationId::new(),
            execution: ExecutionId::new(),
            work: WorkSpec::Connector {
                spec: Box::new(ConnectorSpec {
                    operation: operation.clone(),
                    runner: ConnectorRunner {
                        runtime: CertificateFingerprint::from_der(SCRIPT),
                        account: ProcessAccount {
                            uid: 1000,
                            gid: 1000,
                        },
                        limits: ProcessLimits {
                            runtime_seconds: 30,
                            output_bytes: 1024,
                            processes: 8,
                            memory_bytes: 67108864,
                        },
                    },
                    deadline: 1234,
                    result: connector_output(4096),
                }),
            },
        };
        let id = ConnectorCallId::new();
        let path = directory.join(format!("{id}.json"));
        let write = |value: serde_json::Value| {
            fs::write(&path, value.to_string()).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        };
        for phase in ["requested", "dispatch_possible"] {
            write(json!({"digest":operation.digest(),"phase":phase}));
            assert!(captured(root.path(), &body).unwrap().is_empty());
        }
        let outcome = ConnectorOutcome::Finished {
            result: ConnectorResult::Catalog { tools: vec![] },
        };
        write(json!({"digest":operation.digest(),"phase":"captured","outcome":outcome}));
        let saved = captured(root.path(), &body).unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(
            (saved[0].task, saved[0].execution, saved[0].id),
            (body.task, body.execution, id)
        );
        assert_eq!(saved[0].outcome, outcome);
        process::write(&saved[0].acknowledgment, &true).unwrap();
        assert!(captured(root.path(), &body).unwrap().is_empty());
        fs::remove_file(&saved[0].acknowledgment).unwrap();
        write(
            json!({"digest":CertificateFingerprint::from_der(b"different"),"phase":"captured","outcome":outcome}),
        );
        assert!(captured(root.path(), &body).is_err());
    }

    #[test]
    fn runtime_identity_binds_the_helper_and_exact_interpreter_bytes() {
        let root = tempfile::tempdir().unwrap();
        let python = root.path().join("python");
        fs::write(&python, b"synthetic interpreter").unwrap();
        let python_sha256 = CertificateFingerprint::from_der(b"synthetic interpreter");
        let mut bytes = SCRIPT.to_vec();
        bytes.extend_from_slice(python_sha256.as_str().as_bytes());
        let config = Configuration {
            python: python.clone(),
            python_sha256,
            runtime: CertificateFingerprint::from_der(&bytes),
            account: ProcessAccount {
                uid: 1000,
                gid: 1000,
            },
            ceilings: ProcessLimits {
                runtime_seconds: 30,
                output_bytes: 1024,
                processes: 8,
                memory_bytes: 67108864,
            },
        };
        config.verify().unwrap();
        fs::write(python, b"replacement interpreter").unwrap();
        assert!(config.verify().is_err());
    }
}
