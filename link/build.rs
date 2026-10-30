use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("..");
    let config_path = root.join("release.json");
    let schema_path = root.join("link/schema/protocol.json");
    println!("cargo:rerun-if-changed={}", config_path.display());
    println!("cargo:rerun-if-changed={}", schema_path.display());
    println!("cargo:rerun-if-env-changed=BOTSTRAL_RELEASE_IDENTITY");
    let git = |args: &[&str]| {
        let result = Command::new("git")
            .args(args)
            .current_dir(&root)
            .output()
            .expect("git required for release identity");
        assert!(result.status.success(), "git release identity failed");
        String::from_utf8(result.stdout).unwrap().trim().to_owned()
    };
    println!(
        "cargo:rerun-if-changed={}",
        root.join(git(&["rev-parse", "--git-path", "HEAD"]))
            .display()
    );
    let reference = Command::new("git")
        .args(["symbolic-ref", "-q", "HEAD"])
        .current_dir(&root)
        .output()
        .unwrap();
    if reference.status.success() {
        let name = String::from_utf8(reference.stdout).unwrap();
        println!(
            "cargo:rerun-if-changed={}",
            root.join(git(&["rev-parse", "--git-path", name.trim()]))
                .display()
        );
    }
    let config: Value = serde_json::from_slice(&fs::read(config_path).unwrap()).unwrap();
    let revision = git(&["rev-parse", "HEAD"]);
    let identity = if let Ok(path) = env::var("BOTSTRAL_RELEASE_IDENTITY") {
        println!("cargo:rerun-if-changed={path}");
        let value: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(
            value["revision"], revision,
            "release source revision mismatch"
        );
        value
    } else {
        json!({"version": config["version"], "channel": "stable", "revision": revision, "build_id": "development"})
    };
    assert!(
        identity["version"]
            .as_str()
            .unwrap()
            .starts_with(config["version"].as_str().unwrap()),
        "release version mismatch"
    );
    let envelope = json!({"identity": identity, "protocols": config["protocols"], "link_schema_sha256": format!("{:x}", Sha256::digest(fs::read(schema_path).unwrap()))});
    let output = PathBuf::from(env::var("OUT_DIR").unwrap());
    let json = serde_json::to_vec(&envelope).unwrap();
    fs::write(output.join("release.json"), &json).unwrap();
    let mut marker = b"BOTSTRAL_RELEASE_V1\0".to_vec();
    marker.extend(json);
    marker.push(0);
    fs::write(output.join("release.bin"), marker).unwrap();
}
