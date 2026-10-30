use botstral_owner_client::SecretStore;
use zeroize::Zeroizing;

pub fn run() -> Option<std::process::ExitCode> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["--release-acceptance-hold"] {
        println!("release acceptance process ready");
        std::thread::sleep(std::time::Duration::from_secs(30));
        return Some(std::process::ExitCode::SUCCESS);
    }
    if args.first().map(String::as_str) != Some("--release-keychain-check") {
        return None;
    }
    let result = (|| {
        if args.len() != 3 {
            return Err(());
        }
        let id = uuid::Uuid::parse_str(&args[1]).map_err(|_| ())?;
        let slot = format!("release-acceptance-{id}");
        let store = crate::keychain::NativeSecrets::new(&slot).map_err(|_| ())?;
        let expected = b"Botstral synthetic release continuity check";
        match args[2].as_str() {
            "write" => store
                .replace(Zeroizing::new(expected.to_vec()))
                .map_err(|_| ())?,
            "verify" => {
                let actual = store.read().map_err(|_| ())?.ok_or(())?;
                if actual.as_slice() != expected {
                    return Err(());
                }
            }
            "delete" => store.delete().map_err(|_| ())?,
            _ => return Err(()),
        }
        Ok(())
    })();
    Some(if result.is_ok() {
        println!("{{\"keychain_continuity\":true}}");
        std::process::ExitCode::SUCCESS
    } else {
        eprintln!("release keychain check failed");
        std::process::ExitCode::FAILURE
    })
}
