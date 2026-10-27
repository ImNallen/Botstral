use super::*;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};

#[test]
fn collection_validates_files_and_exposes_only_bounded_untrusted_text() {
    for case in [
        "text",
        "binary",
        "large",
        "invalid_utf8",
        "tampered",
        "wrong_length",
        "oversized",
        "symlink",
        "secret",
        "unavailable",
    ] {
        let mut f = fixture();
        let mut task = f.propose();
        let id = ArtifactId::new();
        let bytes = match case {
            "large" => vec![b'x'; 8192],
            "invalid_utf8" => vec![0xff],
            "secret" => b"synthetic-state-inference-secret".to_vec(),
            _ => b"untrusted file content".to_vec(),
        };
        let path = f.state.directory.join("artifacts");
        fs::create_dir_all(&path).unwrap();
        let file = path.join(format!("{id}.blob"));
        fs::write(&file, &bytes).unwrap();
        let content = ArtifactContent {
            length: if case == "wrong_length" {
                99
            } else {
                bytes.len() as u64
            },
            sha256: CertificateFingerprint::from_der(if case == "tampered" {
                b"different"
            } else {
                &bytes
            }),
        };
        task.outputs = vec![OutputView {
            declaration: OutputDeclaration {
                slot: "file".into(),
                name: "result.txt".into(),
                media_type: if case == "binary" {
                    MediaType::Png
                } else {
                    MediaType::TextPlain
                },
                max_bytes: if case == "oversized" { 1 } else { 8192 },
            },
            state: if case == "unavailable" {
                ArtifactState::Unavailable {
                    reason: ArtifactUnavailable::Missing,
                }
            } else {
                ArtifactState::Available { id, content }
            },
        }];
        if case == "symlink" {
            let actual = path.join("actual");
            fs::rename(&file, &actual).unwrap();
            symlink(&actual, &file).unwrap();
        }
        let permit = CollectPermit {
            run: f.input.id,
            turn: RunTurnId::new(),
            task,
            directory: f.state.directory.clone(),
            secret: f.state.provider_secret().unwrap(),
        };
        let result = crate::manager::runs::results::collect(permit);
        match case {
            "text" => {
                let result = result.unwrap();
                assert!(result.contains("untrusted_text"));
                assert!(result.contains("untrusted file content"));
            }
            "binary" | "large" | "unavailable" => {
                let result = result.unwrap();
                assert!(!result.contains("untrusted_text"));
                assert!(result.contains("artifacts"));
            }
            _ => assert!(result.is_err(), "{case}"),
        }
    }
}

#[test]
fn transcript_retention_and_replay_are_bounded() {
    let mut f = fixture();
    for _ in 0..MAX_RUNS {
        f.input.id = RunId::new();
        f.state.submit_run(token(), f.input.clone()).unwrap();
    }
    assert!(f.state.submit_run(token(), f.input.clone()).is_ok());
    f.input.id = RunId::new();
    assert_eq!(
        f.state.submit_run(token(), f.input.clone()),
        Err(OperationError::Busy)
    );
    fs::set_permissions(
        f.state.directory.join("memory-index.sqlite3"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let EventReplay::Snapshot { state, .. } = f.state.resume(token(), 0, 64).unwrap() else {
        panic!()
    };
    assert_eq!(state.runs.len(), MAX_RUNS);
    assert!(json(&state.runs).unwrap().len() < MAX_RUNS * 1024);
    let mut f = fixture();
    let task = f.propose();
    f.approve_and_claim(task.id);
    f.finish(task.id);
    let RunJob::Collect(permit) = f
        .state
        .claim_run_work(&HashSet::new(), true)
        .unwrap()
        .unwrap()
    else {
        panic!()
    };
    f.state
        .finish_run_result(
            f.input.id,
            permit.turn,
            permit.task,
            Ok("x".repeat(MAX_RUN_RESULT_BYTES + 1)),
        )
        .unwrap();
    let view = f.state.load_run(f.input.id).unwrap();
    assert_eq!(
        view.summary.phase,
        RunPhase::AwaitingAttention {
            reason: RunAttentionReason::ResultInvalid
        }
    );
    assert!(json(&view).unwrap().len() < MAX_RUN_VIEW_BYTES);
}
