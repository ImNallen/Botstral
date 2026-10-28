use botstral_manager::{owner_client::OwnerClient, protect_process};
use std::{fs, path::PathBuf, time::Duration};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    protect_process()?;
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let command = args
        .next()
        .ok_or("expected init, import, trust, call, or watch")?;
    let root = PathBuf::from(args.next().ok_or("missing private client directory")?);
    let value = args
        .next()
        .ok_or("missing label, protected input path, or duration")?;
    if args.next().is_some() {
        return Err("too many arguments".into());
    }
    if command == "init" {
        let request = OwnerClient::initialize(&root, value)?;
        println!("{}", serde_json::to_string(&request)?);
        return Ok(());
    }
    let mut client = OwnerClient::open(&root)?;
    match command.as_str() {
        "import" => client.import_invitation(serde_json::from_slice(
            &botstral_link::tls::private_bytes(&PathBuf::from(value))?,
        )?)?,
        "trust" => client.import_trust(serde_json::from_slice(
            &botstral_link::tls::private_bytes(&PathBuf::from(value))?,
        )?)?,
        "call" => {
            let operation = serde_json::from_slice(&fs::read(value)?)?;
            let mut session = client.connect().await?;
            println!(
                "{}",
                serde_json::to_string(&session.call(operation).await?)?
            );
        }
        "watch" => {
            client
                .watch(Duration::from_secs(value.parse()?), |head| {
                    println!("notification head={head}")
                })
                .await?;
            println!("applied cursor={}", client.cursor);
        }
        _ => return Err("unknown command".into()),
    }
    Ok(())
}
