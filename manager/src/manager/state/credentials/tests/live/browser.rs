use super::*;

#[test]
#[ignore = "Requires local Chromium and Node. Run scripts/verify-p06.py."]
fn p06_controlled_browser_acceptance() {
    use std::{
        io::{Read, Write},
        os::unix::net::UnixListener,
        process::Command,
    };
    #[derive(serde::Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
    enum Input {
        Start {
            plan: Box<LoginSitePlan>,
            purpose: CredentialFields,
            policy: PasswordPolicy,
        },
        Operation {
            operation: Box<ProtectedBrowserOperation>,
        },
        ReviewDecision {
            review: Box<CredentialUseReview>,
            decision: UseDecision,
        },
        Decision {
            id: CredentialUseId,
            decision: UseDecision,
        },
        Submission {
            body: Zeroizing<String>,
        },
        Stop,
    }
    let root = tempfile::Builder::new()
        .prefix("p06-")
        .tempdir_in("/tmp")
        .unwrap();
    let listener = UnixListener::bind(root.path().join("manager.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let evidence = std::env::var_os("BOTSTRAL_P06_EVIDENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.path().join("evidence"));
    let mut child = Command::new("node")
        .arg(workspace.join("manager/tests/p06_browser.mjs"))
        .arg(root.path())
        .arg(evidence)
        .current_dir(workspace)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut current: Option<Live> = None;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "controlled browser driver failed");
            break;
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            panic!("controlled browser deadline");
        }
        let (mut stream, _) = match listener.accept() {
            Ok(stream) => stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(_) => panic!("fixture socket failed"),
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Zeroizing::new(Vec::new());
        (&mut stream).take(65537).read_to_end(&mut bytes).unwrap();
        assert!(bytes.len() <= 65536);
        let input: Input = serde_json::from_slice(&bytes).expect("fixture protocol");
        let response = match input {
            Input::Start {
                plan,
                purpose,
                policy,
            } => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64;
                let f = live(*plan, purpose, policy, now);
                let task = f.state.load_task(f.authority.task).unwrap();
                let WorkSpec::Computer { spec } = task.command.work else {
                    unreachable!()
                };
                let operation = serde_json::to_value(&spec.operations[0]).unwrap();
                current = Some(f);
                serde_json::json!({"operation":operation})
            }
            Input::Operation { operation } => {
                let f = current.as_mut().unwrap();
                f.call(*operation)
                    .ok()
                    .and_then(|reply| serde_json::to_value(reply).ok())
                    .unwrap_or_else(|| serde_json::json!({"type":"refused"}))
            }
            Input::ReviewDecision { review, decision } => {
                match current.as_mut().unwrap().state.decide_credential_use(
                    owner(),
                    *review,
                    decision,
                ) {
                    Ok(review) => serde_json::json!({"type":"review","review":review}),
                    Err(_) => serde_json::json!({"type":"refused"}),
                }
            }
            Input::Decision { id, decision } => {
                let f = current.as_mut().unwrap();
                let review: CredentialUseReview = parse(
                    f.state
                        .connection
                        .query_row(
                            "SELECT view FROM credential_uses WHERE id=?1",
                            [id.to_string()],
                            |r| r.get(0),
                        )
                        .unwrap(),
                )
                .unwrap();
                match f.state.decide_credential_use(owner(), review, decision) {
                    Ok(review) => serde_json::json!({"type":"review","review":review}),
                    Err(_) => serde_json::json!({"type":"refused"}),
                }
            }
            Input::Submission { body } => {
                let f = current.as_ref().unwrap();
                let fields: std::collections::HashMap<_, _> =
                    reqwest::Url::parse(&format!("https://example.test/?{}", body.as_str()))
                        .unwrap()
                        .query_pairs()
                        .map(|(k, v)| (k.into_owned(), Zeroizing::new(v.into_owned())))
                        .collect();
                let valid = if let Some(code) = fields.get("totp") {
                    let (expected, _) =
                        totp::current(&live_values().totp_seed, f.state.now()).unwrap();
                    **code == *expected
                } else {
                    fields
                        .get("username")
                        .is_some_and(|s| **s == live_values().username)
                        && fields
                            .get("password")
                            .is_some_and(|s| **s == live_values().password)
                };
                serde_json::json!({"valid":valid})
            }
            Input::Stop => serde_json::json!({"stopped":true}),
        };
        let bytes = Zeroizing::new(serde_json::to_vec(&response).unwrap());
        stream.write_all(&bytes).unwrap();
    }
}
