use super::*;
use std::{collections::BTreeMap, os::unix::fs::DirBuilderExt, process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, process::Command};

#[derive(Clone)]
pub(crate) struct Backend {
    pub configuration: Configuration,
    pub host: CertificateFingerprint,
    pub boot: String,
    pub probed: bool,
    binary: PathBuf,
}
pub(crate) struct EmptyScopeProof {
    scope: ProcessScope,
    limited: bool,
}
impl EmptyScopeProof {
    pub fn limited(&self) -> bool {
        self.limited
    }
    pub fn scope(&self) -> &ProcessScope {
        &self.scope
    }
}

async fn command(binary: &str, args: &[String], seconds: u64) -> io::Result<String> {
    let uid = unsafe { libc::geteuid() };
    let mut child = Command::new(binary)
        .args(args)
        .env_clear()
        .env("XDG_RUNTIME_DIR", format!("/run/user/{uid}"))
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            format!("unix:path=/run/user/{uid}/bus"),
        )
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let mut stdout = child.stdout.take().unwrap();
    tokio::time::timeout(Duration::from_secs(seconds), async {
        let mut output = Vec::new();
        (&mut stdout).take(16385).read_to_end(&mut output).await?;
        if output.len() > 16384 {
            return Err(io::Error::other("systemd output bound"));
        }
        if !child.wait().await?.success() {
            return Err(io::Error::other("systemd request failed"));
        }
        String::from_utf8(output).map_err(io::Error::other)
    })
    .await
    .map_err(io::Error::other)?
}
async fn show(unit: &str) -> io::Result<BTreeMap<String, String>> {
    let output = command("/usr/bin/systemctl", &["--user".into(), "show".into(), unit.into(), "--property=LoadState,ActiveState,Result,InvocationID,ControlGroup,KillMode,Restart,TimeoutStopUSec,TasksMax,MemoryMax,RuntimeMaxUSec".into()], 3).await?;
    Ok(output
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect())
}
fn same(unit: &BTreeMap<String, String>, scope: &ProcessScope) -> bool {
    unit.get("LoadState").is_some_and(|s| s == "loaded")
        && unit.get("InvocationID") == Some(&scope.invocation)
        && unit.get("ControlGroup") == Some(&scope.cgroup)
}
fn cgroup(scope: &ProcessScope) -> PathBuf {
    Path::new("/sys/fs/cgroup").join(scope.cgroup.trim_start_matches('/'))
}

pub(crate) async fn discard_probe(directory: &Path) -> io::Result<()> {
    let released = directory.join("release.json").try_exists()?;
    if directory.join("manifest.json").try_exists()? {
        let manifest: Manifest = read(&directory.join("manifest.json"))?;
        if released
            && !manifest
                .computer
                .as_ref()
                .is_some_and(|launch| launch.probe)
        {
            return Err(io::Error::other("probe unexpectedly released"));
        }
        let name = manifest.unit();
        let unit = show(&name).await?;
        match unit.get("LoadState").map(String::as_str) {
            Some("not-found") => {}
            Some("loaded") => {
                if unit.get("KillMode").map(String::as_str) != Some("control-group")
                    || unit.get("Restart").map(String::as_str) != Some("no")
                {
                    return Err(io::Error::other("probe supervision mismatch"));
                }
                let group = unit.get("ControlGroup").filter(|value| !value.is_empty());
                if let Some(group) = group
                    && (!group.starts_with('/')
                        || group.split('/').any(|part| matches!(part, "." | ".."))
                        || !group.ends_with(&name))
                {
                    return Err(io::Error::other("probe scope mismatch"));
                }
                command(
                    "/usr/bin/systemctl",
                    &["--user".into(), "stop".into(), name],
                    5,
                )
                .await?;
                if let Some(group) = group {
                    match fs::read_to_string(
                        Path::new("/sys/fs/cgroup")
                            .join(group.trim_start_matches('/'))
                            .join("cgroup.events"),
                    ) {
                        Ok(events) if events.lines().any(|line| line == "populated 0") => {}
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                        _ => return Err(io::Error::other("probe scope still populated")),
                    }
                }
            }
            _ => return Err(io::Error::other("probe service state unavailable")),
        }
    } else if released {
        return Err(io::Error::other("released probe manifest missing"));
    }
    fs::remove_dir_all(directory)
}

impl Backend {
    pub async fn probe(
        root: &Path,
        journal: JournalId,
        configuration: Configuration,
    ) -> io::Result<Self> {
        if !cfg!(target_os = "linux") {
            return Err(io::Error::other("unsupported process host"));
        }
        let version = command("/usr/bin/systemd-run", &["--version".into()], 3).await?;
        if !version.starts_with("systemd 255 ") {
            return Err(io::Error::other("systemd 255 required"));
        }
        let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id")?
            .trim()
            .to_string();
        let binary = std::env::current_exe()?.canonicalize()?;
        for path in [&binary, &root.to_path_buf()] {
            if !path
                .as_os_str()
                .as_encoded_bytes()
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(b))
            {
                return Err(io::Error::other("unsupported helper path"));
            }
        }
        let backend = Self {
            configuration,
            host: CertificateFingerprint::from_der(&fs::read("/etc/machine-id")?),
            probed: true,
            boot,
            binary,
        };
        let probes = root.join("process-probe");
        if probes.try_exists()? {
            discard_probe(&probes).await?;
        }
        fs::DirBuilder::new().mode(0o700).create(&probes)?;
        let manifest = Manifest {
            journal,
            execution: ExecutionId::new(),
            host: backend.host.clone(),
            boot: backend.boot.clone(),
            spec: ProcessSpec {
                profile: ProcessProfile::LinuxSystemdV1,
                account: backend.configuration.account,
                executable: "/usr/bin/true".into(),
                args: vec![],
                environment: BTreeMap::new(),
                cwd: "/tmp".into(),
                deadline: now() + 30,
                limits: backend.configuration.ceilings.clone(),
            },
            computer: None,
            vibe: None,
            expires_at: now() + 30,
        };
        write(&probes.join("manifest.json"), &manifest)?;
        let ready = backend.start(&probes, &manifest).await?;
        backend.stop(&ready.scope).await?;
        fs::remove_dir_all(probes)?;
        let processes = root.join("processes");
        if !processes.try_exists()? {
            fs::DirBuilder::new().mode(0o700).create(&processes)?;
        }
        Ok(backend)
    }
    pub fn recovery(configuration: Configuration) -> io::Result<Self> {
        Ok(Self {
            configuration,
            host: CertificateFingerprint::from_der(&fs::read("/etc/machine-id")?),
            probed: false,
            boot: fs::read_to_string("/proc/sys/kernel/random/boot_id")?
                .trim()
                .into(),
            binary: std::env::current_exe()?,
        })
    }
    pub async fn start(&self, directory: &Path, manifest: &Manifest) -> io::Result<Ready> {
        let limits = &manifest.spec.limits;
        let args = vec![
            "--user".into(),
            "--quiet".into(),
            "--expand-environment=no".into(),
            format!("--unit={}", manifest.unit()),
            "--property=Type=exec".into(),
            "--property=Restart=no".into(),
            "--property=RemainAfterExit=yes".into(),
            "--property=KillMode=control-group".into(),
            "--property=SendSIGKILL=yes".into(),
            "--property=TimeoutStopSec=2s".into(),
            "--property=TimeoutStartSec=5s".into(),
            format!(
                "--property=RuntimeMaxSec={}s",
                i64::from(limits.runtime_seconds).min((manifest.spec.deadline - now()).max(1))
            ),
            format!("--property=TasksMax={}", limits.processes),
            format!("--property=MemoryMax={}", limits.memory_bytes),
            "--property=MemorySwapMax=0".into(),
            "--property=OOMPolicy=kill".into(),
            "--property=StandardInput=null".into(),
            "--property=StandardOutput=null".into(),
            "--property=StandardError=null".into(),
            "--property=UMask=0077".into(),
            "--".into(),
            self.binary.to_string_lossy().into_owned(),
            "process-helper".into(),
            directory.to_string_lossy().into_owned(),
        ];
        command("/usr/bin/systemd-run", &args, 6).await?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        loop {
            if directory.join("ready.json").try_exists()? {
                return self.observe(directory, manifest).await;
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(io::Error::other("helper readiness timeout"));
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    pub async fn observe(&self, directory: &Path, manifest: &Manifest) -> io::Result<Ready> {
        let ready: Ready = read(&directory.join("ready.json"))?;
        ready.scope.validate().map_err(io::Error::other)?;
        if ready.manifest != manifest.digest()?
            || ready.scope.journal != manifest.journal
            || ready.scope.execution != manifest.execution
            || ready.scope.host != self.host
            || manifest.host != self.host
            || ready.scope.boot != self.boot
            || manifest.boot != self.boot
        {
            return Err(io::Error::other("helper identity mismatch"));
        }
        let unit = show(&manifest.unit()).await?;
        if !same(&unit, &ready.scope)
            || unit.get("KillMode").map(String::as_str) != Some("control-group")
            || unit.get("Restart").map(String::as_str) != Some("no")
            || unit.get("TimeoutStopUSec").map(String::as_str) != Some("2s")
        {
            return Err(io::Error::other("supervision properties mismatch"));
        }
        let path = cgroup(&ready.scope);
        for (file, expected) in [
            ("memory.max", manifest.spec.limits.memory_bytes),
            ("pids.max", manifest.spec.limits.processes as u64),
            ("memory.swap.max", 0),
        ] {
            if fs::read_to_string(path.join(file))?.trim() != expected.to_string() {
                return Err(io::Error::other("required resource controller unavailable"));
            }
        }
        Ok(ready)
    }
    pub fn release(&self, directory: &Path, ready: &Ready) -> io::Result<()> {
        if directory.join("release.json").try_exists()? {
            return Err(io::Error::other("release cannot repeat"));
        }
        write(&directory.join("release.json"), ready)
    }
    pub fn observation(
        &self,
        directory: &Path,
        ready: &Ready,
        finished: bool,
    ) -> io::Result<Option<ProcessEvidence>> {
        let path = directory.join(if finished {
            "result.json"
        } else {
            "capture.json"
        });
        if !path.try_exists()? {
            return Ok(None);
        }
        let observed: Observation = read(&path)?;
        if observed.ready != *ready {
            return Err(io::Error::other("process observation identity mismatch"));
        }
        observed.evidence.validate().map_err(io::Error::other)?;
        Ok(Some(observed.evidence))
    }
    pub async fn stop(&self, scope: &ProcessScope) -> io::Result<EmptyScopeProof> {
        scope.validate().map_err(io::Error::other)?;
        if scope.host != self.host || scope.boot != self.boot {
            return Err(io::Error::other("scope boot changed"));
        }
        let unit = show(&scope.unit).await?;
        let automatically_stopped = unit.get("LoadState").is_some_and(|v| v == "loaded")
            && unit.get("InvocationID") == Some(&scope.invocation)
            && unit.get("ControlGroup").is_some_and(String::is_empty)
            && unit
                .get("ActiveState")
                .is_some_and(|v| matches!(v.as_str(), "inactive" | "failed"));
        if !same(&unit, scope) && !automatically_stopped {
            return Err(io::Error::other("cannot prove stopped scope identity"));
        }
        command(
            "/usr/bin/systemctl",
            &["--user".into(), "stop".into(), scope.unit.clone()],
            5,
        )
        .await?;
        let path = cgroup(scope).join("cgroup.events");
        match fs::read_to_string(path) {
            Ok(value) if value.lines().any(|l| l == "populated 0") => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            _ => return Err(io::Error::other("scope still populated")),
        }
        Ok(EmptyScopeProof {
            scope: scope.clone(),
            limited: unit
                .get("Result")
                .is_some_and(|v| matches!(v.as_str(), "timeout" | "oom-kill" | "resources")),
        })
    }
}
