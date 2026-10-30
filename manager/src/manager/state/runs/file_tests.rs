use super::*;
use std::fs;

fn hex_of(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0xf) as usize] as char);
    }
    encoded
}

fn staged(id: RunId, name: &str, bytes: &[u8]) -> StageRunFile {
    StageRunFile {
        id,
        name: name.into(),
        sha256: CertificateFingerprint::from_der(bytes),
        hex: hex_of(bytes),
    }
}

fn blob(fixture: &Fixture, id: RunId) -> std::path::PathBuf {
    fixture
        .state
        .directory
        .join("run-files")
        .join(id.to_string())
}

#[test]
fn utf8_file_is_inlined_once_and_survives_restart() {
    let mut f = fixture();
    let body = "attached-note-alpha";
    let bytes = body.as_bytes();
    let input = staged(f.input.id, "notes.txt", bytes);
    let sentence = format!(
        "Untrusted attached file notes.txt sha256 {}:\n{body}",
        input.sha256.as_str()
    );
    f.state.stage_run_file(token(), input.clone()).unwrap();
    f.state.stage_run_file(token(), input).unwrap();
    let other = staged(f.input.id, "notes.txt", b"different-bytes");
    assert_eq!(
        f.state.stage_run_file(token(), other).unwrap_err(),
        OperationError::Conflict
    );
    assert_eq!(fs::read(blob(&f, f.input.id)).unwrap(), bytes);
    let view = f.state.submit_run(token(), f.input.clone()).unwrap();
    assert_eq!(view.request.prompt, f.input.prompt);
    assert!(!view.request.prompt.contains(body));
    assert_eq!(f.state.submit_run(token(), f.input.clone()).unwrap(), view);
    assert_eq!(fs::read(blob(&f, f.input.id)).unwrap(), bytes);
    assert_eq!(
        fs::read_dir(f.state.directory.join("run-files"))
            .unwrap()
            .count(),
        1
    );
    let contents = f.state.run_user_contents(f.input.id).unwrap();
    assert!(contents.iter().any(|content| content == &sentence));
    assert!(
        contents
            .iter()
            .all(|content| content != body || content == &f.input.prompt)
    );
    super::restart_tests::reopen(&mut f);
    assert_eq!(fs::read(blob(&f, f.input.id)).unwrap(), bytes);
    let restored = f.state.run_user_contents(f.input.id).unwrap();
    assert!(restored.iter().any(|content| content == &sentence));
    assert!(
        !f.state
            .load_run(f.input.id)
            .unwrap()
            .request
            .prompt
            .contains(body)
    );
}

#[test]
fn non_utf8_file_is_retained_without_its_bytes() {
    let mut f = fixture();
    let bytes = [0xff, 0xfe, 0xfd];
    let input = staged(f.input.id, "blob.bin", &bytes);
    let sentence = format!(
        "Untrusted attached file blob.bin is retained. sha256 {}. length 3. The bytes are not inlined.",
        input.sha256.as_str()
    );
    f.state.stage_run_file(token(), input).unwrap();
    f.state.submit_run(token(), f.input.clone()).unwrap();
    assert_eq!(fs::read(blob(&f, f.input.id)).unwrap(), bytes);
    let contents = f.state.run_user_contents(f.input.id).unwrap();
    assert!(contents.iter().any(|content| content == &sentence));
    assert!(contents.iter().all(|content| {
        !content
            .as_bytes()
            .windows(bytes.len())
            .any(|window| window == bytes)
    }));
}

#[test]
fn secret_file_is_retained_and_omitted_from_the_transcript() {
    let mut f = fixture();
    let secret = "synthetic-state-inference-secret";
    let bytes = format!("notes {secret} end").into_bytes();
    let input = staged(f.input.id, "secret.txt", &bytes);
    let sentence = format!(
        "Untrusted attached file secret.txt is retained. sha256 {}. length {}. The bytes are not inlined.",
        input.sha256.as_str(),
        bytes.len()
    );
    f.state.stage_run_file(token(), input).unwrap();
    let view = f.state.submit_run(token(), f.input.clone()).unwrap();
    assert!(!view.request.prompt.contains(secret));
    let contents = f.state.run_user_contents(f.input.id).unwrap();
    assert!(contents.iter().any(|content| content == &sentence));
    assert!(contents.iter().all(|content| !content.contains(secret)));
    assert_eq!(fs::read(blob(&f, f.input.id)).unwrap(), bytes);
}

#[test]
fn offline_restore_preserves_bound_context_and_pending_attachment_admission() {
    use std::{os::unix::fs::PermissionsExt, process::Command};
    let mut f = fixture();
    let bound = b"Bound attachment survives restoration.";
    let pending = b"Pending attachment survives restoration.";
    f.state
        .stage_run_file(token(), staged(f.input.id, "bound.txt", bound))
        .unwrap();
    f.state.submit_run(token(), f.input.clone()).unwrap();
    let mut next = f.input.clone();
    next.id = RunId::new();
    f.state
        .stage_run_file(token(), staged(next.id, "pending.txt", pending))
        .unwrap();
    let source = f.state.directory.canonicalize().unwrap();
    fs::set_permissions(&source, fs::Permissions::from_mode(0o700)).unwrap();
    f.state
        .connection
        .execute(
            "VACUUM INTO ?1",
            [source.join("manager.sqlite3").to_str().unwrap()],
        )
        .unwrap();
    fs::write(source.join("manager.lock"), b"").unwrap();
    for entry in fs::read_dir(&source).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        }
    }
    let installation = std::mem::replace(
        &mut f.state.installation,
        Installation::Configured {
            vault: VaultState::Locked,
        },
    );
    drop(f.state);
    let recovery = tempfile::tempdir().unwrap();
    let recovery_root = recovery.path().canonicalize().unwrap();
    fs::set_permissions(&recovery_root, fs::Permissions::from_mode(0o700)).unwrap();
    let backup = recovery_root.join("backup");
    let restored = recovery_root.join("restored");
    let maintenance = |args: &[&std::ffi::OsStr]| {
        let output = Command::new("python3")
            .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/packaging/backup.py"))
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    maintenance(&[
        "create".as_ref(),
        "--state-dir".as_ref(),
        source.as_os_str(),
        "--destination".as_ref(),
        backup.as_os_str(),
    ]);
    fs::remove_dir_all(source.join("run-files")).unwrap();
    maintenance(&[
        "restore".as_ref(),
        "--backup".as_ref(),
        backup.as_os_str(),
        "--destination".as_ref(),
        restored.as_os_str(),
    ]);
    let (configured, _) = watch::channel(false);
    let mut state = State::open(restored.clone(), configured).unwrap();
    state.test_time = Some(1000);
    state.installation = installation;
    state
        .connection
        .execute(
            "INSERT INTO sessions VALUES(?1,?2,1,1,10000)",
            params![
                Uuid::new_v4().to_string(),
                crypto::token_hash(&"x".repeat(43)).as_slice()
            ],
        )
        .unwrap();
    for (id, expected) in [
        (f.input.id, bound.as_slice()),
        (next.id, pending.as_slice()),
    ] {
        let path = restored.join("run-files").join(id.to_string());
        assert_eq!(fs::read(&path).unwrap(), expected);
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert!(
        state
            .run_user_contents(f.input.id)
            .unwrap()
            .iter()
            .any(|text| text.contains(std::str::from_utf8(bound).unwrap()))
    );
    state.submit_run(token(), next.clone()).unwrap();
    assert!(
        state
            .run_user_contents(next.id)
            .unwrap()
            .iter()
            .any(|text| text.contains(std::str::from_utf8(pending).unwrap()))
    );
}
