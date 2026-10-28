use super::*;
use std::{
    os::fd::AsRawFd,
    os::unix::process::ExitStatusExt,
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

static STOPPING: AtomicBool = AtomicBool::new(false);
extern "C" fn stop(_: libc::c_int) {
    STOPPING.store(true, Ordering::Relaxed);
}

struct Capture<R> {
    pipe: R,
    bytes: Vec<u8>,
    truncated: bool,
    complete: bool,
    limit: usize,
}
impl<R: Read + AsRawFd> Capture<R> {
    fn new(pipe: R, limit: usize) -> io::Result<Self> {
        let fd = pipe.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            pipe,
            bytes: Vec::with_capacity(limit),
            truncated: false,
            complete: false,
            limit,
        })
    }
    fn drain(&mut self) -> io::Result<()> {
        let mut buffer = [0; 4096];
        for _ in 0..16 {
            match self.pipe.read(&mut buffer) {
                Ok(0) => {
                    self.complete = true;
                    break;
                }
                Ok(n) => {
                    let keep = n.min(self.limit - self.bytes.len());
                    self.bytes.extend_from_slice(&buffer[..keep]);
                    self.truncated |= keep < n;
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
    fn output(&self) -> CapturedOutput {
        use std::fmt::Write;
        let mut hex = String::with_capacity(self.bytes.len() * 2);
        for byte in &self.bytes {
            write!(&mut hex, "{byte:02x}").expect("string");
        }
        CapturedOutput {
            hex,
            truncated: self.truncated,
            complete: self.complete,
        }
    }
}
fn empty() -> CapturedOutput {
    CapturedOutput {
        hex: String::new(),
        truncated: false,
        complete: true,
    }
}

pub(super) fn run(directory: &Path) -> io::Result<()> {
    if !cfg!(target_os = "linux") {
        return Err(io::Error::other("Linux required"));
    }
    unsafe {
        libc::signal(libc::SIGTERM, stop as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, stop as *const () as libc::sighandler_t);
    }
    let manifest: Manifest = read(&directory.join("manifest.json"))?;
    if let Some(computer) = &manifest.computer {
        computer.verify(&manifest.spec)?;
    } else if let Some(connector) = &manifest.connector {
        connector.verify(&manifest.spec)?;
    } else if let Some(vibe) = &manifest.vibe {
        vibe.verify(&manifest.spec)?;
    } else {
        manifest.spec.validate().map_err(io::Error::other)?;
    }
    if manifest.spec.account.uid != unsafe { libc::geteuid() }
        || manifest.spec.account.gid != unsafe { libc::getegid() }
    {
        return Err(io::Error::other("helper account mismatch"));
    }
    let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id")?
        .trim()
        .to_string();
    let cgroups = fs::read_to_string("/proc/self/cgroup")?;
    let cgroup = cgroups
        .lines()
        .find_map(|l| l.strip_prefix("0::"))
        .ok_or_else(|| io::Error::other("cgroup v2 required"))?
        .to_string();
    let scope = ProcessScope {
        journal: manifest.journal,
        execution: manifest.execution,
        host: CertificateFingerprint::from_der(&fs::read("/etc/machine-id")?),
        boot,
        invocation: std::env::var("INVOCATION_ID").map_err(io::Error::other)?,
        unit: manifest.unit(),
        cgroup,
    };
    scope.validate().map_err(io::Error::other)?;
    if scope.host != manifest.host || scope.boot != manifest.boot {
        return Err(io::Error::other("helper boot mismatch"));
    }
    let ready = Ready {
        scope,
        manifest: manifest.digest()?,
    };
    write(&directory.join("ready.json"), &ready)?;
    let start = Instant::now();
    loop {
        if STOPPING.load(Ordering::Relaxed) {
            return Ok(());
        }
        if start.elapsed() > Duration::from_secs(5)
            || now() >= manifest.expires_at
            || now() >= manifest.spec.deadline
        {
            return Ok(());
        }
        if directory.join("release.json").try_exists()? {
            let release: Ready = read(&directory.join("release.json"))?;
            if release != ready {
                return Err(io::Error::other("release identity mismatch"));
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut observation = Observation {
        ready,
        evidence: ProcessEvidence {
            effect: ProcessEffect::NotReleased,
            result: Some(ProcessResult {
                exit: ProcessExit::Expired,
                stdout: empty(),
                stderr: empty(),
            }),
        },
    };
    if !STOPPING.load(Ordering::Relaxed)
        && now() < manifest.expires_at
        && now() < manifest.spec.deadline
    {
        let child = Command::new(&manifest.spec.executable)
            .args(&manifest.spec.args)
            .env_clear()
            .envs(&manifest.spec.environment)
            .current_dir(&manifest.spec.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        match child {
            Err(_) => observation.evidence.result.as_mut().unwrap().exit = ProcessExit::SpawnFailed,
            Ok(mut child) => {
                observation.evidence.effect = ProcessEffect::Observed;
                let mut stdout = Capture::new(
                    child.stdout.take().unwrap(),
                    manifest.spec.limits.output_bytes as usize,
                )?;
                let mut stderr = Capture::new(
                    child.stderr.take().unwrap(),
                    manifest.spec.limits.output_bytes as usize,
                )?;
                let mut exited = None;
                let mut last_snapshot = Instant::now() - Duration::from_secs(1);
                loop {
                    stdout.drain()?;
                    stderr.drain()?;
                    if exited.is_none()
                        && let Some(status) = child.try_wait()?
                    {
                        exited = Some((status, Instant::now()));
                    }
                    observation.evidence.result = Some(ProcessResult {
                        exit: exited.map_or(ProcessExit::Unobserved, |(s, _)| {
                            s.code().map_or(
                                ProcessExit::Signalled {
                                    signal: s.signal().unwrap_or(0),
                                },
                                |code| ProcessExit::Exited { code },
                            )
                        }),
                        stdout: stdout.output(),
                        stderr: stderr.output(),
                    });
                    if last_snapshot.elapsed() >= Duration::from_millis(250) {
                        write(&directory.join("capture.json"), &observation)?;
                        last_snapshot = Instant::now();
                    }
                    if now() >= manifest.spec.deadline {
                        return Err(io::Error::other("process deadline"));
                    }
                    if exited.is_some_and(|(_, at)| {
                        (stdout.complete && stderr.complete)
                            || at.elapsed() >= Duration::from_millis(200)
                    }) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        }
    }
    write(&directory.join("result.json"), &observation)?;
    while !STOPPING.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}
