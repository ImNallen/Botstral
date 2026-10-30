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
