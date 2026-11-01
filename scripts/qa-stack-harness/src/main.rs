use botstral_desktop::remote::{IntentCommand, RemoteService, SendInput, stage_join_code};
use botstral_link::ProcessAccount;
use botstral_owner_client::{ClientError, SecretStore};
use serde_json::{Value, json};
use std::{
    io::{BufRead, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
};
use zeroize::Zeroizing;

struct MemorySecrets(Mutex<Option<Zeroizing<Vec<u8>>>>);
impl SecretStore for MemorySecrets {
    fn read(&self) -> Result<Option<Zeroizing<Vec<u8>>>, ClientError> {
        Ok(self.0.lock().expect("store").clone())
    }
    fn replace(&self, value: Zeroizing<Vec<u8>>) -> Result<(), ClientError> {
        *self.0.lock().expect("store") = Some(value);
        Ok(())
    }
    fn delete(&self) -> Result<(), ClientError> {
        self.0.lock().expect("store").take();
        Ok(())
    }
}

fn emit(value: &Value) {
    println!("{value}");
    let _ = std::io::stdout().flush();
}

fn snapshot_json(service: &RemoteService) -> Value {
    serde_json::to_value(service.inspect()).unwrap_or_else(|_| json!({"serialize": "failed"}))
}

fn sent_limits(service: &RemoteService) -> Value {
    let Some(workspace) = service.inspect().workspace else {
        return json!(null);
    };
    json!(
        workspace
            .runs
            .iter()
            .map(|run| {
                json!({
                    "id": run.summary.id,
                    "phase": format!("{:?}", run.summary.phase),
                    "max_total_tokens": run.request.as_ref().map(|r| r.limits.max_total_tokens),
                    "max_output_tokens": run.request.as_ref().map(|r| r.limits.max_output_tokens),
                    "max_cost_microusd": run.request.as_ref().map(|r| r.limits.max_cost_microusd),
                    "prompt": run.request.as_ref().map(|r| r.prompt.clone()),
                })
            })
            .collect::<Vec<_>>()
    )
}

#[tokio::main]
async fn main() {
    let root = PathBuf::from(std::env::args().nth(1).expect("owner directory"));
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&root)
            .expect("owner dir");
        let _ = std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700));
    }
    let store = Arc::new(MemorySecrets(Mutex::new(None)));
    let service = RemoteService::new(root.clone(), store);
    for line in std::io::stdin().lock().lines() {
        let input: Value = serde_json::from_str(&line.expect("stdin")).expect("json");
        let op = input["op"].as_str().expect("op");
        let result: Result<Value, String> = async {
            match op {
                "begin" => {
                    let label = input["label"].as_str().unwrap_or("QA desktop").to_owned();
                    let view = service.command(IntentCommand::Begin { label }).await?;
                    Ok(json!({
                        "ok": true,
                        "status": view.status,
                        "fingerprint": view.pairing.as_ref().map(|p| p.fingerprint.clone()),
                        "label": view.pairing.as_ref().map(|p| p.label.clone()),
                    }))
                }
                "join" => {
                    let code = input["code"].as_str().ok_or("missing code")?;
                    let view = stage_join_code(&service, code).await?;
                    Ok(json!({
                        "ok": true,
                        "status": view.status,
                        "candidate": view.candidate,
                    }))
                }
                "confirm" => {
                    let id = service.inspect().candidate.ok_or("no candidate")?.id;
                    let view = service.command(IntentCommand::Confirm(id)).await?;
                    Ok(json!({
                        "ok": true,
                        "status": view.status,
                        "issue": view.issue,
                        "workspace": snapshot_json(&service)["workspace"],
                    }))
                }
                "refresh" => {
                    let view = service.command(IntentCommand::Refresh).await?;
                    Ok(json!({
                        "ok": true,
                        "status": view.status,
                        "issue": view.issue,
                        "workspace": snapshot_json(&service)["workspace"],
                        "sent_limits": sent_limits(&service),
                    }))
                }
                "send" => {
                    let bot = input["bot"]
                        .as_str()
                        .ok_or("missing bot")?
                        .parse()
                        .map_err(|_| "bot")?;
                    let tokens = input["tokens"].as_u64().ok_or("missing tokens")?;
                    let output = input["output"].as_u64().unwrap_or(1024) as u32;
                    let cost = input["cost"].as_u64().unwrap_or(200_000);
                    let turns = input["turns"].as_u64().unwrap_or(1) as u32;
                    let text = input["text"]
                        .as_str()
                        .unwrap_or("Reply with the single word pong.")
                        .to_owned();
                    match service
                        .command(IntentCommand::Send(SendInput {
                            bot,
                            session: None,
                            text,
                            account: ProcessAccount {
                                uid: 1000,
                                gid: 1000,
                            },
                            runtime: None,
                            process: false,
                            seconds: 120,
                            turns,
                            tokens,
                            cost_microusd: cost,
                            output_tokens: output,
                        }))
                        .await
                    {
                        Ok(view) => Ok(json!({
                            "ok": true,
                            "status": view.status,
                            "issue": view.issue,
                            "tokens_sent": tokens,
                            "output_sent": output,
                            "cost_sent": cost,
                            "workspace": snapshot_json(&service)["workspace"],
                            "sent_limits": sent_limits(&service),
                        })),
                        Err(error) => Ok(json!({
                            "ok": false,
                            "error": error,
                            "tokens_sent": tokens,
                            "output_sent": output,
                            "cost_sent": cost,
                            "issue": service.inspect().issue,
                        })),
                    }
                }
                "wait" => {
                    let until = tokio::time::Instant::now() + std::time::Duration::from_secs(180);
                    loop {
                        let view = service.command(IntentCommand::Refresh).await?;
                        let workspace = snapshot_json(&service)["workspace"].clone();
                        let phase = workspace["runs"]
                            .as_array()
                            .and_then(|runs| runs.last())
                            .and_then(|run| run["phase"].as_str())
                            .unwrap_or("")
                            .to_owned();
                        if matches!(
                            phase.as_str(),
                            "completed" | "stopped" | "cancelled" | "awaiting_attention"
                        ) || tokio::time::Instant::now() >= until
                        {
                            return Ok(json!({
                                "ok": true,
                                "status": view.status,
                                "issue": view.issue,
                                "phase": phase,
                                "workspace": workspace,
                                "sent_limits": sent_limits(&service),
                            }));
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                    }
                }
                "inspect" => Ok(json!({
                    "ok": true,
                    "view": snapshot_json(&service),
                    "sent_limits": sent_limits(&service),
                })),
                "quit" => {
                    service.shutdown().await;
                    return Ok(json!({"ok": true, "quit": true}));
                }
                other => Err(format!("unknown op {other}")),
            }
        }
        .await;
        match result {
            Ok(value) => emit(&value),
            Err(error) => emit(&json!({
                "ok": false,
                "error": error,
                "view": snapshot_json(&service),
            })),
        }
        if op == "quit" {
            break;
        }
    }
}
