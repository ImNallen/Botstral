use super::*;
use std::{
    collections::BTreeMap,
    os::unix::fs::{DirBuilderExt, MetadataExt},
    process::Stdio,
    time::Duration,
};
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

fn scope_empty(group: &Path, pinned: Option<&fs::File>) -> io::Result<bool> {
    let check_identity = || -> io::Result<()> {
        match fs::metadata(group) {
            Ok(current) => {
                let original = pinned
                    .ok_or_else(|| io::Error::other("stopped scope cgroup appeared"))?
                    .metadata()?;
                if current.dev() != original.dev() || current.ino() != original.ino() {
                    return Err(io::Error::other("stopped scope cgroup replaced"));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        Ok(())
    };
    check_identity()?;
    let empty = match fs::read_to_string(group.join("cgroup.events")) {
        Ok(value) if value.lines().any(|line| line == "populated 0") => true,
        Ok(value) if value.lines().any(|line| line == "populated 1") => false,
        Ok(_) => return Err(io::Error::other("scope population unavailable")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if group.try_exists()? {
                return Err(io::Error::other("scope population unavailable"));
            }
            true
        }
        Err(error) => return Err(error),
    };
    check_identity()?;
    Ok(empty)
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
            connector: None,
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
        let remaining = manifest
            .expires_at
            .min(manifest.spec.deadline)
            .saturating_sub(now())
            .clamp(0, 30);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(remaining as u64);
        loop {
            if tokio::time::Instant::now() >= deadline
                || now() >= manifest.expires_at
                || now() >= manifest.spec.deadline
            {
                return Err(io::Error::other("helper readiness timeout"));
            }
            if directory.join("ready.json").try_exists()? {
                return self.observe(directory, manifest).await;
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
        self.stop_with(scope, async {
            command(
                "/usr/bin/systemctl",
                &["--user".into(), "stop".into(), scope.unit.clone()],
                5,
            )
            .await
        })
        .await
    }
    async fn stop_with(
        &self,
        scope: &ProcessScope,
        stop: impl std::future::Future<Output = io::Result<String>>,
    ) -> io::Result<EmptyScopeProof> {
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
        let group = cgroup(scope);
        let pinned = match fs::File::open(&group) {
            Ok(directory) => Some(directory),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        let stop_error = stop.await.err();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        loop {
            let empty = scope_empty(&group, pinned.as_ref())?;
            if empty {
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(io::Error::other(format!(
                    "scope still populated after stop; stop request error: {stop_error:?}"
                )));
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let stopped = show(&scope.unit).await?;
        let absent = stopped
            .get("LoadState")
            .is_some_and(|value| value == "not-found");
        let retained = stopped
            .get("LoadState")
            .is_some_and(|value| value == "loaded")
            && stopped.get("InvocationID") == Some(&scope.invocation)
            && stopped
                .get("ActiveState")
                .is_some_and(|state| matches!(state.as_str(), "inactive" | "failed"))
            && stopped
                .get("ControlGroup")
                .is_some_and(|value| value == &scope.cgroup || value.is_empty());
        if (!absent && !retained)
            || fs::read_to_string("/proc/sys/kernel/random/boot_id")?.trim() != scope.boot
        {
            return Err(io::Error::other("stopped scope identity changed"));
        }
        if !scope_empty(&group, pinned.as_ref())? {
            return Err(io::Error::other("scope populated during stop proof"));
        }
        Ok(EmptyScopeProof {
            scope: scope.clone(),
            limited: [&unit, &stopped].iter().any(|observation| {
                observation.get("Result").is_some_and(|value| {
                    matches!(value.as_str(), "timeout" | "oom-kill" | "resources")
                })
            }),
        })
    }
}

#[cfg(all(test, target_os = "linux"))]
mod stop_tests {
    use super::*;

    async fn fixture() -> (Backend, ProcessScope) {
        fixture_sleep("30").await
    }
    async fn fixture_sleep(duration: &str) -> (Backend, ProcessScope) {
        let backend = Backend::recovery(Configuration {
            profile: ProcessProfile::LinuxSystemdV1,
            account: ProcessAccount {
                uid: unsafe { libc::geteuid() },
                gid: unsafe { libc::getegid() },
            },
            ceilings: ProcessLimits {
                runtime_seconds: 30,
                output_bytes: 1024,
                processes: 8,
                memory_bytes: 67108864,
            },
        })
        .unwrap();
        let journal = JournalId::new();
        let execution = ExecutionId::new();
        let name = format!("botstral-{journal}-{execution}.service");
        command(
            "/usr/bin/systemd-run",
            &[
                "--user".into(),
                "--quiet".into(),
                format!("--unit={name}"),
                "--property=RemainAfterExit=yes".into(),
                "--property=KillMode=control-group".into(),
                "--property=TimeoutStopSec=2s".into(),
                "/usr/bin/sleep".into(),
                duration.into(),
            ],
            5,
        )
        .await
        .unwrap();
        let unit = show(&name).await.unwrap();
        let scope = ProcessScope {
            journal,
            execution,
            host: backend.host.clone(),
            boot: backend.boot.clone(),
            invocation: unit["InvocationID"].clone(),
            cgroup: unit["ControlGroup"].clone(),
            unit: name,
        };
        (backend, scope)
    }
    async fn stop(scope: &ProcessScope) -> io::Result<String> {
        command(
            "/usr/bin/systemctl",
            &["--user".into(), "stop".into(), scope.unit.clone()],
            5,
        )
        .await
    }
    #[tokio::test]
    #[ignore = "requires Linux systemd user manager and real cgroups"]
    async fn stop_error_after_kernel_stop_is_proved() {
        let (backend, scope) = fixture().await;
        let result = backend
            .stop_with(&scope, async {
                stop(&scope).await?;
                Err(io::Error::other(
                    "injected lost stop response after actual stop",
                ))
            })
            .await;
        assert!(
            result.is_ok(),
            "actual stopped scope was not proved: {:?}",
            result.err()
        );
    }
    #[tokio::test]
    #[ignore = "requires Linux systemd user manager and real cgroups"]
    async fn delayed_kernel_stop_is_proved() {
        let (backend, scope) = fixture().await;
        let delayed = scope.clone();
        let mut delayed_stop = None;
        let result = backend
            .stop_with(&scope, async {
                delayed_stop = Some(tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    stop(&delayed).await.unwrap();
                }));
                Ok(String::new())
            })
            .await;
        delayed_stop.unwrap().await.unwrap();
        assert!(
            result.is_ok(),
            "delayed empty scope was not proved: {:?}",
            result.err()
        );
    }
    #[tokio::test]
    #[ignore = "requires Linux systemd user manager and real cgroups"]
    async fn populated_kernel_scope_is_refused() {
        let (backend, scope) = fixture().await;
        let result = backend
            .stop_with(&scope, async {
                Err(io::Error::other("injected stop failure"))
            })
            .await;
        let events = fs::read_to_string(cgroup(&scope).join("cgroup.events")).unwrap();
        stop(&scope).await.unwrap();
        assert!(events.lines().any(|line| line == "populated 1"));
        assert!(result.is_err(), "populated scope received empty proof");
    }
    #[tokio::test]
    #[ignore = "requires Linux systemd user manager and real cgroups"]
    async fn wrong_invocation_is_refused_before_stop() {
        let (backend, mut scope) = fixture().await;
        scope.invocation = "0".repeat(32);
        let result = backend
            .stop_with(&scope, async { panic!("unauthenticated stop ran") })
            .await;
        let events = fs::read_to_string(cgroup(&scope).join("cgroup.events")).unwrap();
        stop(&scope).await.unwrap();
        assert!(events.lines().any(|line| line == "populated 1"));
        assert!(result.is_err(), "wrong invocation received proof");
    }
    #[tokio::test]
    #[ignore = "requires Linux systemd user manager and real cgroups"]
    async fn replacement_after_stop_is_refused() {
        let (backend, scope) = fixture().await;
        let result = backend
            .stop_with(&scope, async {
                stop(&scope).await?;
                command(
                    "/usr/bin/systemd-run",
                    &[
                        "--user".into(),
                        "--quiet".into(),
                        format!("--unit={}", scope.unit),
                        "--property=RemainAfterExit=yes".into(),
                        "/usr/bin/true".into(),
                    ],
                    5,
                )
                .await
            })
            .await;
        stop(&scope).await.unwrap();
        assert!(
            result.is_err(),
            "replacement invocation received empty proof"
        );
    }
    #[tokio::test]
    #[ignore = "requires Linux systemd user manager and real cgroups"]
    async fn wrong_boot_is_refused_before_stop() {
        let (backend, mut scope) = fixture().await;
        scope.boot = RequestId::new().to_string();
        let result = backend
            .stop_with(&scope, async { panic!("wrong boot stop ran") })
            .await;
        let events = fs::read_to_string(cgroup(&scope).join("cgroup.events")).unwrap();
        stop(&scope).await.unwrap();
        assert!(events.lines().any(|line| line == "populated 1"));
        assert!(result.is_err(), "wrong boot received proof");
    }
    #[tokio::test]
    #[ignore = "requires Linux systemd user manager and real cgroups"]
    async fn production_stop_proves_real_scope() {
        let (backend, scope) = fixture().await;
        let proof = backend.stop(&scope).await.unwrap();
        assert_eq!(proof.scope(), &scope);
        assert!(!cgroup(&scope).try_exists().unwrap());
    }
    #[test]
    fn unavailable_population_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let pinned = fs::File::open(directory.path()).unwrap();
        assert!(scope_empty(directory.path(), Some(&pinned)).is_err());
    }
    #[tokio::test]
    #[ignore = "requires Linux systemd user manager and real cgroups"]
    async fn active_empty_service_after_failed_stop_is_refused() {
        let (backend, scope) = fixture_sleep("0.5").await;
        let result = backend
            .stop_with(&scope, async {
                tokio::time::sleep(Duration::from_millis(700)).await;
                Err(io::Error::other("injected stop failure"))
            })
            .await;
        let observed = show(&scope.unit).await.unwrap();
        stop(&scope).await.unwrap();
        assert_eq!(observed["ActiveState"], "active");
        assert!(observed["ControlGroup"].is_empty());
        assert_eq!(observed["InvocationID"], scope.invocation);
        assert!(
            result.is_err(),
            "active service received stopped scope proof"
        );
    }
}
