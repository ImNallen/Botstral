use super::*;
use botstral_desktop::remote::{RemoteSnapshot, save_artifact};
use serde_json::{Value, json};

async fn wait(service: &RemoteService, ready: impl Fn(&RemoteSnapshot) -> bool) -> RemoteSnapshot {
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let view = service.command(IntentCommand::Refresh).await.unwrap();
            if ready(&view) {
                return view;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap()
}

async fn wait_file(path: &Path) {
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        while !path.exists() {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
}

pub(super) async fn verify() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("owner");
    let fixture_root = PathBuf::from(std::env::var_os("BOTSTRAL_C02_FIXTURE_ROOT").unwrap());
    let (service, store, fixture) = paired(&root).await;
    let note = format!("Northern Light attachment {}", uuid::Uuid::new_v4());
    let path = directory.path().join("note.txt");
    std::fs::write(&path, &note).unwrap();
    let mut response: Value = serde_json::from_str(include_str!(
        "../../../../manager/tests/fixtures/m04/completion.json"
    ))
    .unwrap();
    let final_body = response.to_string();
    response["choices"][0]["finish_reason"] = json!("tool_calls");
    response["choices"][0]["message"] = json!({"role":"assistant","content":"Recorded attachment export proposal.","tool_calls":[{"id":"attachment-export","type":"function","function":{"name":"computer","arguments":json!({"operations":[{"type":"write_file","path":"/tmp/attachment-note.txt","text":note},{"type":"read_file","path":"/tmp/attachment-note.txt","output":"text"}],"outputs":["text"]}).to_string()}}]});
    let script = fixture_root.join("provider-script.json");
    std::fs::write(
        &script,
        serde_json::to_vec(&[response.to_string(), final_body]).unwrap(),
    )
    .unwrap();
    wait_file(&fixture_root.join("provider-script-loaded")).await;
    let prompt = "Create a result from the attached note.";
    service.remember_pending_file(path);
    service
        .command(IntentCommand::Send(SendInput {
            bot: fixture["bot"].as_str().unwrap().parse().unwrap(),
            session: Some(fixture["session"].as_str().unwrap().parse().unwrap()),
            text: prompt.into(),
            account: ProcessAccount {
                uid: 1000,
                gid: 1000,
            },
            runtime: Some(
                fixture["runtime"]
                    .as_str()
                    .unwrap()
                    .to_owned()
                    .try_into()
                    .unwrap(),
            ),
            process: false,
            seconds: 120,
            turns: 4,
            tokens: 200_000,
            cost_microusd: 200_000,
            output_tokens: 1024,
        }))
        .await
        .unwrap();
    let proposed = wait(&service, |v| {
        v.workspace
            .as_ref()
            .unwrap()
            .state
            .tasks
            .iter()
            .any(|t| !t.approval.consumed && !t.state.terminal())
    })
    .await;
    let workspace = proposed.workspace.as_ref().unwrap();
    let run = workspace
        .runs
        .iter()
        .find(|r| r.request.as_ref().is_some_and(|r| r.prompt == prompt))
        .unwrap();
    let id = run.summary.id;
    assert_eq!(run.request.as_ref().unwrap().prompt, prompt);
    wait(&service, |_| {
        let requests: Option<Vec<Value>> =
            std::fs::read(fixture_root.join("provider-requests.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        requests.is_some_and(|requests| {
            requests.iter().any(|r| {
                r["messages"].as_array().unwrap().iter().any(|m| {
                    m["content"].as_str().is_some_and(|s| {
                        s.starts_with("Untrusted attached file note.txt sha256 ")
                            && s.ends_with(&note)
                    })
                })
            })
        })
    })
    .await;
    let task = workspace
        .state
        .tasks
        .iter()
        .find(|t| !t.approval.consumed && !t.state.terminal())
        .unwrap()
        .id;
    let review = service
        .command(IntentCommand::Review(task))
        .await
        .unwrap()
        .review
        .unwrap();
    service
        .command(IntentCommand::Approve(review.id))
        .await
        .unwrap();
    let completed = wait(&service, |v| {
        v.workspace
            .as_ref()
            .unwrap()
            .runs
            .iter()
            .any(|r| r.summary.id == id && r.summary.phase == RunPhase::Completed)
    })
    .await;
    let artifact = completed
        .workspace
        .as_ref()
        .unwrap()
        .state
        .tasks
        .iter()
        .find(|t| t.id == task)
        .unwrap()
        .outputs
        .iter()
        .find_map(|o| {
            if let ArtifactState::Available { id, .. } = o.state {
                Some(id)
            } else {
                None
            }
        })
        .unwrap();
    let expected = b"Synthetic C02 recorded worker output. No production process ran.\n";
    let export = directory.path().join("result.txt");
    save_artifact(&export, service.artifact(task, artifact).await.unwrap()).unwrap();
    assert_eq!(std::fs::read(&export).unwrap(), expected);
    let blob = fixture_root
        .join("manager-state/run-files")
        .join(id.to_string());
    assert_eq!(std::fs::read(&blob).unwrap(), note.as_bytes());
    service.shutdown().await;
    std::fs::write(fixture_root.join("restart-manager"), b"restart").unwrap();
    wait_file(&fixture_root.join("manager-restarted")).await;
    let reopened = RemoteService::new(root, store);
    reopened.restore().await.unwrap();
    wait(&reopened, |v| {
        v.workspace.as_ref().is_some_and(|w| {
            w.runs
                .iter()
                .any(|r| r.summary.id == id && r.summary.phase == RunPhase::Completed)
        })
    })
    .await;
    assert_eq!(std::fs::read(&blob).unwrap(), note.as_bytes());
    save_artifact(
        &directory.path().join("after-restart.txt"),
        reopened.artifact(task, artifact).await.unwrap(),
    )
    .unwrap();
    assert_eq!(
        std::fs::read(directory.path().join("after-restart.txt")).unwrap(),
        expected
    );
    reopened.sign_out().await.unwrap();
    reopened.shutdown().await;
}
