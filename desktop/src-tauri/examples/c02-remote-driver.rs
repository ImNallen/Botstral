use botstral_desktop::{
    keychain::NativeSecrets,
    remote::{IntentCommand, RemoteService, save_artifact},
};
use botstral_owner_client::SecretStore;
use serde_json::{Value, json};
use std::{
    io::{BufRead, Write},
    path::PathBuf,
    sync::Arc,
};

struct Cleanup(Arc<NativeSecrets>);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = self.0.delete();
    }
}

#[tokio::main]
async fn main() {
    let root = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .expect("private driver directory"),
    );
    let slot = format!("c02-production-{}", uuid::Uuid::new_v4());
    let store = Arc::new(NativeSecrets::new(&slot).unwrap());
    let _cleanup = Cleanup(store.clone());
    let mut service = RemoteService::new(root.join("owner"), store.clone());
    for line in std::io::stdin().lock().lines() {
        let input: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let result: Result<(), String> = async {
            let command = match input["op"].as_str().unwrap() {
                "begin" => IntentCommand::Begin {
                    label: "C02 production native driver".into(),
                },
                "stage" => IntentCommand::Stage(
                    serde_json::from_slice(
                        &std::fs::read(input["file"].as_str().unwrap()).unwrap(),
                    )
                    .unwrap(),
                ),
                "trust" => IntentCommand::StageTrust(
                    serde_json::from_slice(
                        &std::fs::read(input["file"].as_str().unwrap()).unwrap(),
                    )
                    .unwrap(),
                ),
                "confirm" => IntentCommand::Confirm(input["id"].as_str().unwrap().parse().unwrap()),
                "send" => {
                    IntentCommand::Send(serde_json::from_value(input["input"].clone()).unwrap())
                }
                "review" => IntentCommand::Review(input["id"].as_str().unwrap().parse().unwrap()),
                "approve" => IntentCommand::Approve(input["id"].as_str().unwrap().parse().unwrap()),
                "cancel" => {
                    IntentCommand::CancelTask(input["id"].as_str().unwrap().parse().unwrap())
                }
                "suspend" => IntentCommand::Suspend,
                "resume" => IntentCommand::Resume,
                "refresh" => IntentCommand::Refresh,
                "inspect" => return Ok(()),
                "restore" => {
                    service.shutdown().await;
                    service = RemoteService::new(root.join("owner"), store.clone());
                    service.restore().await?;
                    return Ok(());
                }
                "export" => {
                    let artifact = service
                        .artifact(
                            input["task"].as_str().unwrap().parse().unwrap(),
                            input["artifact"].as_str().unwrap().parse().unwrap(),
                        )
                        .await?;
                    save_artifact(&root.join(input["name"].as_str().unwrap()), artifact)?;
                    return Ok(());
                }
                "quit" => {
                    service.sign_out().await?;
                    return Ok(());
                }
                _ => return Err("unknown driver command".into()),
            };
            service.command(command).await.map(|_| ())
        }
        .await;
        let view = service.inspect();
        println!(
            "{}",
            json!({"ok":result.is_ok(),"error":result.err(),"view":view,"workspace":view.workspace})
        );
        std::io::stdout().flush().unwrap();
        if input["op"] == "quit" {
            break;
        }
    }
    service.shutdown().await;
}
