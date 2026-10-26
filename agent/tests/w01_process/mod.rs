use super::*;

fn epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
fn limits() -> Value {
    json!({"runtime_seconds":30,"output_bytes":1024,"processes":32,"memory_bytes":268435456})
}
fn request(session: &ComputerSession, executable: &str, args: &[&str], cwd: &Path) -> Value {
    let deadline = epoch() + 120;
    json!({"id":TaskId::new(),"session_id":session.id,"conversation_id":null,
        "settings":{"model":"unavailable","max_turns":1,"max_tokens":1,"deadline":deadline,"max_cost_micros":0},
        "work":{"type":"process","spec":{"profile":"linux_systemd_v1","account":{"uid":unsafe { libc::geteuid() },"gid":unsafe { libc::getegid() }},
            "executable":executable,"args":args,"environment":{"LITERAL":"$HOME %n"},"cwd":cwd,"deadline":deadline,"limits":limits()}}})
}
async fn task(f: &Fixture, original: &Value, state: &str) -> Value {
    let path = format!("/v1/tasks/{}", original["id"].as_str().unwrap());
    let end = Instant::now() + Duration::from_secs(15);
    loop {
        let (status, value) = f.owner("GET", &path, json!({})).await;
        assert_eq!(status, 200);
        if value["state"] == state {
            return value;
        }
        assert!(Instant::now() < end, "task did not reach {state}: {value}");
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
}
async fn approve(f: &Fixture, original: &Value) {
    let response = f
        .owner(
            "PUT",
            &format!("/v1/tasks/{}/approval", original["id"].as_str().unwrap()),
            json!({"expires_at":epoch()+90}),
        )
        .await;
    assert_eq!(response.0, 200, "{response:?}");
}
fn production_agent(f: &Fixture, name: &str, enabled: bool) -> Process {
    let state = f.root.path().join("production-agent");
    if !state.exists() {
        fs::create_dir(&state).unwrap();
        fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
        private(
            &state.join("config.json"),
            &serde_json::to_vec(&f.worker).unwrap(),
        );
    }
    if enabled && !state.join("process.json").exists() {
        private(&state.join("process.json"), &serde_json::to_vec(&json!({"profile":"linux_systemd_v1","account":{"uid":unsafe { libc::geteuid() },"gid":unsafe { libc::getegid() }},"ceilings":limits()})).unwrap());
    }
    let binary = std::env::var_os("BOTSTRAL_PROCESS_AGENT_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| agent_binary().into());
    launch(
        &binary,
        &["run", "--state-dir", state.to_str().unwrap()],
        f.root.path(),
        name,
    )
}
fn output(value: &Value, name: &str) -> Vec<u8> {
    let hex = value["process"]["result"][name]["hex"].as_str().unwrap();
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}
fn evidence(name: &str, value: &Value) {
    if let Some(directory) = std::env::var_os("BOTSTRAL_PROCESS_EVIDENCE") {
        let path = PathBuf::from(directory);
        fs::create_dir_all(&path).unwrap();
        fs::write(
            path.join(format!("{name}.json")),
            serde_json::to_vec_pretty(value).unwrap(),
        )
        .unwrap();
    }
}

#[tokio::test]
#[ignore = "Runs the opt-in production process contract on Linux with a systemd user manager."]
async fn w01_production_contract() {
    let mut f = Fixture::new().await;
    let _cleanup = UnitCleanup(f.root.path().join("production-agent"));
    let session = f.durable_session().await;
    let script = f.root.path().join("guest.py");
    private(&script, br#"import os,sys,json
with open('invocations','a') as f: f.write('one\n')
with open('effect.json','w') as f: json.dump({'args':sys.argv[1:],'cwd':os.getcwd(),'env':dict(os.environ)},f)
os.write(1,b'actual stdout\x00\xff')
os.write(2,b'actual stderr')
"#);
    let input = request(
        &session,
        "/usr/bin/python3",
        &[script.to_str().unwrap(), "$HOME", "%n", "a b"],
        f.root.path(),
    );
    let submitted = f.owner("POST", "/v1/tasks", input.clone()).await;
    assert_eq!(
        submitted.0, 200,
        "W01 production process submission is accepted: {}",
        submitted.1
    );
    let original = submitted.1;
    let mut daemon = production_agent(&f, "disabled", false);
    f.wait_phase("ready", 1).await;
    approve(&f, &original).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let refused = task(&f, &original, "queued").await;
    assert_eq!(
        refused["approval"]["consumed"], false,
        "disabled capability refuses a valid approval"
    );
    assert!(!f.root.path().join("invocations").exists());
    assert_eq!(
        f.owner(
            "DELETE",
            &format!("/v1/tasks/{}/approval", original["id"].as_str().unwrap()),
            json!({})
        )
        .await
        .0,
        200
    );
    daemon.stop().await;
    let mut daemon = production_agent(&f, "enabled", true);
    f.wait_phase("ready", 2).await;
    task(&f, &original, "awaiting_approval").await;
    assert!(
        !f.root.path().join("invocations").exists(),
        "approval gates launch"
    );
    let mut changed = input.clone();
    changed["work"]["spec"]["args"] = json!(["changed"]);
    assert_eq!(f.owner("POST", "/v1/tasks", changed).await.0, 409);
    approve(&f, &original).await;
    let completed = task(&f, &original, "completed").await;
    assert_eq!(output(&completed, "stdout"), b"actual stdout\0\xff");
    assert_eq!(output(&completed, "stderr"), b"actual stderr");
    assert_eq!(
        completed["process"]["result"]["exit"],
        json!({"type":"exited","code":0})
    );
    assert_eq!(completed["outcome"]["type"], "process_finished");
    let effect: Value =
        serde_json::from_slice(&fs::read(f.root.path().join("effect.json")).unwrap()).unwrap();
    assert_eq!(effect["args"], json!(["$HOME", "%n", "a b"]));
    assert_eq!(effect["cwd"], f.root.path().to_str().unwrap());
    assert_eq!(effect["env"]["LITERAL"], "$HOME %n");
    assert!(effect["env"].get("HOME").is_none());
    assert_eq!(
        fs::read_to_string(f.root.path().join("invocations")).unwrap(),
        "one\n"
    );
    assert_eq!(
        f.owner("POST", "/v1/tasks", input).await.1["command"],
        original["command"]
    );
    let journal =
        rusqlite::Connection::open(f.root.path().join("production-agent/agent.sqlite3")).unwrap();
    let reserved: i64 = journal
        .query_row(
            "SELECT remaining FROM reservations WHERE execution=?1",
            [original["command"]["execution"].as_str().unwrap()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        reserved, 1,
        "completion retains capacity for a cancellation racing its report"
    );
    drop(journal);
    evidence("success", &json!({"task":completed,"effect":effect}));
    exercise_processes(&mut f, &session, &mut daemon).await;
    daemon.stop().await;
    f.manager.stop().await;
}

async fn submit(f: &Fixture, input: Value) -> Value {
    let response = f.owner("POST", "/v1/tasks", input).await;
    assert_eq!(response.0, 200, "{response:?}");
    response.1
}
async fn wait_file(path: &Path) {
    let until = Instant::now() + Duration::from_secs(12);
    while !path.exists() {
        assert!(
            Instant::now() < until,
            "guest file missing: {}",
            path.display()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
fn live(pid: i32) -> bool {
    let path = format!("/proc/{pid}/stat");
    fs::read_to_string(path).is_ok_and(|value| !value.rsplit_once(") ").unwrap().1.starts_with('Z'))
}
async fn descendants(f: &Fixture, session: &ComputerSession, name: &str) -> (Value, Vec<i32>) {
    let script = f.root.path().join(format!("{name}.py"));
    let pidfile = f.root.path().join(format!("{name}.pids"));
    private(
        &script,
        br#"import os,signal,time
signal.signal(signal.SIGTERM,signal.SIG_IGN)
parent=os.getpid()
child=os.fork()
if child==0:
    os.setsid()
    while True: time.sleep(.1)
with open(__file__+'.effect','a') as f: f.write('one\n')
with open(__file__.replace('.py','.pids'),'w') as f: f.write(str(parent)+' '+str(child))
print('effect before stopping',flush=True)
while True: time.sleep(.1)
"#,
    );
    let input = request(
        session,
        "/usr/bin/python3",
        &[script.to_str().unwrap()],
        f.root.path(),
    );
    let task = submit(f, input).await;
    approve(f, &task).await;
    wait_file(&pidfile).await;
    let pids: Vec<i32> = fs::read_to_string(pidfile)
        .unwrap()
        .split_whitespace()
        .map(|p| p.parse().unwrap())
        .collect();
    assert!(pids.iter().all(|p| live(*p)));
    (task, pids)
}
async fn cancel(f: &Fixture, original: &Value) -> Value {
    let response = f
        .owner(
            "POST",
            &format!("/v1/tasks/{}/cancel", original["id"].as_str().unwrap()),
            json!({}),
        )
        .await;
    assert_eq!(response.0, 200);
    task(f, original, "cancelled").await
}
async fn exercise_processes(f: &mut Fixture, session: &ComputerSession, daemon: &mut Process) {
    let (running, pids) = descendants(f, session, "cancel").await;
    let began = Instant::now();
    assert_eq!(f.owner("GET", "/v1/workers", json!({})).await.0, 200);
    assert!(began.elapsed() < Duration::from_secs(2));
    tokio::time::sleep(Duration::from_secs(11)).await;
    assert!(pids.iter().all(|p| live(*p)), "command spans a heartbeat");
    let stopped = cancel(f, &running).await;
    assert!(
        pids.iter().all(|p| !live(*p)),
        "cancelled only after every descendant stopped"
    );
    assert_eq!(stopped["process"]["effect"], "observed");
    assert!(
        String::from_utf8(output(&stopped, "stdout"))
            .unwrap()
            .contains("effect before stopping")
    );
    evidence(
        "cancel-descendants",
        &json!({"pids":pids,"dead":true,"task":stopped}),
    );

    let mut ordinary = request(
        session,
        "/usr/bin/python3",
        &[
            "-c",
            "import os,time; p=os.fork(); os.setsid() if p==0 else open('orphan.pid','w').write(str(p)); time.sleep(60) if p==0 else None",
        ],
        f.root.path(),
    );
    ordinary["work"]["spec"]["limits"]["runtime_seconds"] = 10.into();
    let orphan = submit(f, ordinary).await;
    approve(f, &orphan).await;
    let finished = task(f, &orphan, "completed").await;
    let pid: i32 = fs::read_to_string(f.root.path().join("orphan.pid"))
        .unwrap()
        .parse()
        .unwrap();
    assert!(!live(pid), "normal completion also removes descendants");
    evidence("normal-exit-descendant", &finished);

    let (running, pids) = descendants(f, session, "capacity").await;
    let queued = submit(
        f,
        request(session, "/usr/bin/printf", &["second"], f.root.path()),
    )
    .await;
    approve(f, &queued).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let held = task(f, &queued, "awaiting_approval").await;
    assert_eq!(
        held["approval"]["consumed"], false,
        "one process reserves admission"
    );
    let revoked = submit(
        f,
        request(session, "/usr/bin/printf", &["revoked"], f.root.path()),
    )
    .await;
    approve(f, &revoked).await;
    assert_eq!(
        f.owner(
            "DELETE",
            &format!("/v1/tasks/{}/approval", revoked["id"].as_str().unwrap()),
            json!({})
        )
        .await
        .0,
        200
    );
    let expired = submit(
        f,
        request(session, "/usr/bin/printf", &["expired"], f.root.path()),
    )
    .await;
    assert_eq!(
        f.owner(
            "PUT",
            &format!("/v1/tasks/{}/approval", expired["id"].as_str().unwrap()),
            json!({"expires_at":epoch()+1})
        )
        .await
        .0,
        200
    );
    tokio::time::sleep(Duration::from_secs(2)).await;
    cancel(f, &running).await;
    assert!(pids.iter().all(|p| !live(*p)));
    let second = task(f, &queued, "completed").await;
    assert_eq!(output(&second, "stdout"), b"second");
    for held in [&revoked, &expired] {
        let v = task(f, held, "awaiting_approval").await;
        assert_eq!(v["approval"]["consumed"], false);
        cancel(f, held).await;
    }
    evidence("capacity-and-authority", &second);

    let mut timed = request(session, "/usr/bin/sleep", &["60"], f.root.path());
    timed["work"]["spec"]["limits"]["runtime_seconds"] = 2.into();
    let timeout = submit(f, timed).await;
    approve(f, &timeout).await;
    let uncertain = task(f, &timeout, "failed").await;
    assert_eq!(uncertain["outcome"]["reason"], "limit");
    evidence("runtime-limit", &uncertain);

    let large = submit(
        f,
        request(
            session,
            "/usr/bin/python3",
            &[
                "-c",
                "import os; [(os.write(1,b'x'*4096),os.write(2,b'y'*4096)) for _ in range(1024)]",
            ],
            f.root.path(),
        ),
    )
    .await;
    approve(f, &large).await;
    let complete = task(f, &large, "completed").await;
    for stream in ["stdout", "stderr"] {
        assert_eq!(output(&complete, stream).len(), 1024);
        assert_eq!(complete["process"]["result"][stream]["truncated"], true);
    }
    evidence("bounded-output", &complete);
    let nonzero = submit(
        f,
        request(
            session,
            "/usr/bin/python3",
            &["-c", "raise SystemExit(17)"],
            f.root.path(),
        ),
    )
    .await;
    approve(f, &nonzero).await;
    let failed = task(f, &nonzero, "failed").await;
    assert_eq!(
        failed["process"]["result"]["exit"],
        json!({"type":"exited","code":17})
    );
    evidence("nonzero", &failed);

    let (controlled, pids) = descendants(f, session, "control").await;
    let current = f.confirmed(session.id).await;
    let owner = f.takeover(&current, InputOwner::Owner).await;
    assert!(
        pids.iter().all(|p| !live(*p)),
        "ControlApplied waits for scope emptiness"
    );
    let interrupted = task(f, &controlled, "awaiting_attention").await;
    evidence("control-stop", &interrupted);
    let _ = f.takeover(&owner, InputOwner::Bot).await;
    cancel(f, &controlled).await;

    let (running, pids) = descendants(f, session, "reconnect").await;
    f.restart_manager(None).await;
    let interrupted = task(f, &running, "awaiting_attention").await;
    assert_eq!(interrupted["command"], running["command"]);
    assert!(pids.iter().all(|p| !live(*p)));
    assert_eq!(
        fs::read_to_string(f.root.path().join("reconnect.py.effect")).unwrap(),
        "one\n"
    );
    let cancelled = cancel(f, &running).await;
    assert_eq!(cancelled["process"], interrupted["process"]);
    evidence("manager-restart", &cancelled);

    let (running, pids) = descendants(f, session, "shutdown").await;
    daemon.stop().await;
    assert!(pids.iter().all(|p| !live(*p)));
    *daemon = production_agent(f, "after-shutdown", true);
    let restored = task(f, &running, "awaiting_attention").await;
    assert_eq!(restored["command"], running["command"]);
    assert_eq!(
        fs::read_to_string(f.root.path().join("shutdown.py.effect")).unwrap(),
        "one\n"
    );
    evidence("shutdown", &restored);
    cancel(f, &running).await;

    let (running, pids) = descendants(f, session, "daemon-crash").await;
    daemon.0.kill().unwrap();
    daemon.0.wait().unwrap();
    assert!(pids.iter().all(|p| live(*p)), "service outlives the daemon");
    *daemon = production_agent(f, "after-crash", true);
    let restored = task(f, &running, "awaiting_attention").await;
    assert!(pids.iter().all(|p| !live(*p)));
    assert_eq!(
        fs::read_to_string(f.root.path().join("daemon-crash.py.effect")).unwrap(),
        "one\n"
    );
    evidence("daemon-crash", &restored);
    cancel(f, &running).await;
}

struct UnitCleanup(PathBuf);
impl Drop for UnitCleanup {
    fn drop(&mut self) {
        let mut manifests = vec![self.0.join("process-probe/manifest.json")];
        if let Ok(entries) = fs::read_dir(self.0.join("processes")) {
            manifests.extend(
                entries
                    .filter_map(Result::ok)
                    .map(|e| e.path().join("manifest.json")),
            );
        }
        for path in manifests {
            let Ok(bytes) = fs::read(path) else {
                continue;
            };
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            let journal: JournalId = value["journal"].as_str().unwrap().parse().unwrap();
            let execution: ExecutionId = value["execution"].as_str().unwrap().parse().unwrap();
            let unit = format!("botstral-{journal}-{execution}.service");
            let _ = Command::new("/usr/bin/timeout")
                .args(["8", "systemctl", "--user", "stop", &unit])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            let _ = Command::new("/usr/bin/timeout")
                .args(["3", "systemctl", "--user", "reset-failed", &unit])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
}

#[cfg(feature = "nonproduction-test-worker")]
#[tokio::test]
#[ignore = "Uses existing debug-only fault injection with real Linux production-agent processes."]
async fn w01_crash_boundaries() {
    for (point, count) in [
        ("process_launch_intent", 0),
        ("process_start_observed", 0),
        ("process_result_observed", 1),
    ] {
        let mut f = Fixture::new().await;
        let _cleanup = UnitCleanup(f.root.path().join("production-agent"));
        let session = f.durable_session().await;
        let mut init = production_agent(&f, "init", true);
        f.wait_phase("ready", 1).await;
        init.stop().await;
        let task_input = request(
            &session,
            "/usr/bin/python3",
            &["-c", "open('invocations','a').write('one\\n')"],
            f.root.path(),
        );
        let original = submit(&f, task_input).await;
        approve(&f, &original).await;
        let state = f.root.path().join("production-agent");
        let log = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(f.root.path().join("fault.log"))
            .unwrap();
        let mut daemon = Process(
            Command::new(agent_binary())
                .args(["run", "--state-dir", state.to_str().unwrap()])
                .env("BOTSTRAL_FAULT", point)
                .stdout(log.try_clone().unwrap())
                .stderr(log)
                .spawn()
                .unwrap(),
        );
        wait_file(&state.join(format!("fault-{point}.reached"))).await;
        daemon.0.wait().unwrap();
        let mut restarted = production_agent(&f, "recovered", true);
        let recovered = task(&f, &original, "awaiting_attention").await;
        let invocations = fs::read_to_string(f.root.path().join("invocations")).unwrap_or_default();
        assert_eq!(invocations.lines().count(), count);
        assert_eq!(recovered["command"], original["command"]);
        evidence(point, &json!({"invocations":count,"task":recovered}));
        if point == "process_launch_intent" {
            assert_eq!(recovered["outcome"]["reason"], "launch_or_scope_unknown");
            restarted.0.kill().unwrap();
            restarted.0.wait().unwrap();
        } else {
            restarted.stop().await;
        }
        f.manager.stop().await;
    }
}

#[tokio::test]
#[ignore = "Verifies the independent runtime ceiling after an uncatchable daemon crash."]
async fn w01_crash_runtime_bound() {
    let mut f = Fixture::new().await;
    let _cleanup = UnitCleanup(f.root.path().join("production-agent"));
    let session = f.durable_session().await;
    let mut daemon = production_agent(&f, "independent", true);
    f.wait_phase("ready", 1).await;
    let script = f.root.path().join("bounded.py");
    private(
        &script,
        br#"import os,signal,time
signal.signal(signal.SIGTERM,signal.SIG_IGN)
p=os.getpid()
c=os.fork()
if c==0:
    os.setsid()
    while True: time.sleep(.1)
with open('bounded.pids','w') as f: f.write(str(p)+' '+str(c))
with open('bounded.ledger','a') as f: f.write('one\n')
while True: time.sleep(.1)
"#,
    );
    let mut input = request(
        &session,
        "/usr/bin/python3",
        &[script.to_str().unwrap()],
        f.root.path(),
    );
    input["work"]["spec"]["limits"]["runtime_seconds"] = 3.into();
    let original = submit(&f, input).await;
    approve(&f, &original).await;
    wait_file(&f.root.path().join("bounded.pids")).await;
    let pids: Vec<i32> = fs::read_to_string(f.root.path().join("bounded.pids"))
        .unwrap()
        .split_whitespace()
        .map(|p| p.parse().unwrap())
        .collect();
    daemon.0.kill().unwrap();
    daemon.0.wait().unwrap();
    assert!(pids.iter().all(|p| live(*p)));
    let deadline = Instant::now() + Duration::from_secs(8);
    while pids.iter().any(|p| live(*p)) {
        assert!(
            Instant::now() < deadline,
            "independent supervision exceeded runtime and stop grace"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let mut restarted = production_agent(&f, "after-independent", true);
    let uncertain = task(&f, &original, "awaiting_attention").await;
    assert_eq!(
        fs::read_to_string(f.root.path().join("bounded.ledger")).unwrap(),
        "one\n"
    );
    assert_eq!(uncertain["outcome"]["reason"], "scope_stop_unproved");
    evidence(
        "independent-runtime",
        &json!({"pids":pids,"all_dead":true,"task":uncertain}),
    );
    restarted.0.kill().unwrap();
    restarted.0.wait().unwrap();
    f.manager.stop().await;
}

#[cfg(not(target_os = "linux"))]
#[tokio::test]
#[ignore = "Checks production capability refusal on an unsupported host."]
async fn w01_unsupported_host() {
    let mut f = Fixture::new().await;
    let session = f.durable_session().await;
    let mut daemon = production_agent(&f, "unsupported", true);
    f.wait_phase("ready", 1).await;
    let original = submit(&f, request(&session, "/usr/bin/true", &[], f.root.path())).await;
    approve(&f, &original).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let unchanged = task(&f, &original, "queued").await;
    assert_eq!(unchanged["approval"]["consumed"], false);
    let journal =
        rusqlite::Connection::open(f.root.path().join("production-agent/agent.sqlite3")).unwrap();
    assert_eq!(
        journal
            .query_row::<i64, _, _>("SELECT count(*) FROM executions", [], |r| r.get(0))
            .unwrap(),
        0
    );
    evidence("unsupported-host", &unchanged);
    daemon.stop().await;
    f.manager.stop().await;
}

#[tokio::test]
#[ignore = "Verifies that a replacement journal cannot prove an old process scope empty."]
async fn w01_journal_replacement() {
    let mut f = Fixture::new().await;
    let _cleanup = UnitCleanup(f.root.path().join("production-agent"));
    let replacement = f.root.path().join("replacement-agent");
    let _replacement_cleanup = UnitCleanup(replacement.clone());
    let session = f.durable_session().await;
    let mut original_daemon = production_agent(&f, "old-journal", true);
    f.wait_phase("ready", 1).await;
    let (original, pids) = descendants(&f, &session, "old-scope").await;
    original_daemon.0.kill().unwrap();
    original_daemon.0.wait().unwrap();
    fs::create_dir(&replacement).unwrap();
    fs::set_permissions(&replacement, fs::Permissions::from_mode(0o700)).unwrap();
    for file in ["config.json", "process.json"] {
        private(
            &replacement.join(file),
            &fs::read(f.root.path().join("production-agent").join(file)).unwrap(),
        );
    }
    let binary = std::env::var_os("BOTSTRAL_PROCESS_AGENT_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| agent_binary().into());
    let mut daemon = launch(
        &binary,
        &["run", "--state-dir", replacement.to_str().unwrap()],
        f.root.path(),
        "replacement",
    );
    let unknown = task(&f, &original, "awaiting_attention").await;
    assert!(
        unknown["effect_evidence"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["reason"] == "journal_replaced")
    );
    assert!(pids.iter().all(|p| live(*p)));
    let next = submit(
        &f,
        request(
            &session,
            "/usr/bin/python3",
            &["-c", "open('forbidden-replacement','w').write('effect')"],
            f.root.path(),
        ),
    )
    .await;
    approve(&f, &next).await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        fs::read_to_string(f.root.path().join("forbidden-replacement")).unwrap_or_default(),
        "",
        "unknown old scope blocks fresh process claims"
    );
    f.owner(
        "POST",
        &format!("/v1/tasks/{}/cancel", original["id"].as_str().unwrap()),
        json!({}),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(600)).await;
    let pending = task(&f, &original, "cancelling").await;
    assert!(
        pids.iter().all(|p| live(*p)),
        "replacement journal cannot claim live descendants stopped"
    );
    evidence(
        "journal-replacement",
        &json!({"task":pending,"old_pids":pids,"still_live":true,"replacement_effect":false}),
    );
    daemon.stop().await;
    f.manager.stop().await;
}
