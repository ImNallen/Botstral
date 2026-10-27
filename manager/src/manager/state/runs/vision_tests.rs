use super::*;
use crate::manager::provider::{ModelRequest, Provider};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

fn image(f: &mut Fixture) -> (ImageReference, Vec<u8>) {
    let mut task = f.propose();
    f.approve_and_claim(task.id);
    f.finish(task.id);
    task = f.state.load_task(task.id).unwrap();
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 2, 2);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255])
            .unwrap();
    }
    let id = ArtifactId::new();
    let content = ArtifactContent {
        length: bytes.len() as u64,
        sha256: CertificateFingerprint::from_der(&bytes),
    };
    task.outputs = vec![OutputView {
        declaration: OutputDeclaration {
            slot: "screen".into(),
            name: "screen.png".into(),
            media_type: MediaType::Png,
            max_bytes: MAX_IMAGE_BYTES,
        },
        state: ArtifactState::Available {
            id,
            content: content.clone(),
        },
    }];
    tasks::save(&f.state.connection, &task).unwrap();
    std::fs::create_dir_all(f.state.directory.join("artifacts")).unwrap();
    std::fs::write(
        f.state
            .directory
            .join("artifacts")
            .join(format!("{id}.blob")),
        &bytes,
    )
    .unwrap();
    f.state.connection.execute("INSERT INTO artifacts(id,task,worker,journal,execution,slot,reserved,state) VALUES(?1,?2,?3,?4,?5,'screen',0,'available')",
        params![id.to_string(),task.id.to_string(),task.command.worker.to_string(),f.journal.to_string(),task.command.execution.to_string()]).unwrap();
    assert_eq!(
        f.state
            .cancel_run(token(), f.input.id)
            .unwrap()
            .summary
            .phase,
        RunPhase::Cancelled
    );
    let mut config = f
        .state
        .load_inference_configuration(f.input.bot, 1)
        .unwrap();
    config.policy.prices.billing =
        InferenceBilling::ContextTokensIncludingImagesAtAcknowledgedCeilings;
    f.state
        .configure_inference(
            token(),
            ConfigureInference {
                bot: f.input.bot,
                expected_revision: 1,
                policy: config.policy,
            },
        )
        .unwrap();
    let verification = f
        .state
        .prepare_inference_verification(
            token(),
            VerifyInference {
                bot: f.input.bot,
                revision: 2,
            },
        )
        .unwrap();
    f.state
        .finish_inference_verification(
            verification.configuration,
            verification.epoch,
            Some((true, true)),
        )
        .unwrap();
    f.input.configuration_revision = 2;
    f.input.id = RunId::new();
    f.input.execution_policy.vision = Some(VisionPolicy {
        allowed_slots: vec!["screen".into()],
        max_images: 1,
        max_image_bytes: MAX_IMAGE_BYTES,
        max_total_pixels: MAX_IMAGE_PIXELS,
    });
    (
        ImageReference {
            task: task.id,
            artifact: id,
            slot: "screen".into(),
            sha256: content.sha256,
        },
        bytes,
    )
}
#[tokio::test]
async fn vision_sends_exact_authorized_png_bytes_and_retains_artifact_identity() {
    let mut f = fixture();
    let (reference, bytes) = image(&mut f);
    f.input.images = vec![reference.clone()];
    f.input.execution_policy.computer = None;
    f.input.execution_policy.process = None;
    f.state.submit_run(token(), f.input.clone()).unwrap();
    let permit = claim(&mut f.state);
    let encoded: String = f
        .state
        .connection
        .query_row(
            "SELECT provider_request FROM run_turns WHERE id=?1",
            [permit.turn.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert!(encoded.contains(&reference.artifact.to_string()));
    assert!(encoded.contains(reference.sha256.as_str()));
    assert!(!encoded.contains("base64"));
    let model = permit.configuration.policy.model_id.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let provider = Provider::fixture(format!("http://{}", listener.local_addr().unwrap()));
    let app=axum::Router::new().route("/v1/chat/completions",axum::routing::post(move |axum::Json(body):axum::Json<Value>| {
        let bytes=bytes.clone(); let model=model.clone(); async move {
            let url=body["messages"][0]["content"][1]["image_url"]["url"].as_str().unwrap();
            assert_eq!(STANDARD.decode(url.strip_prefix("data:image/png;base64,").unwrap()).unwrap(),bytes);
            assert!(body["messages"][0].get("authorized_images").is_none());
            assert_eq!(body["max_tokens"],64);
            assert!(body.get("tools").is_none());
            assert_eq!(body["tool_choice"], "none");
            axum::Json(json!({"id":"vision-fixture","model":model,"choices":[{"index":0,"message":{"role":"assistant","content":"red green blue white"},"finish_reason":"stop"}],"usage":{"prompt_tokens":40,"completion_tokens":4,"total_tokens":44}}))
        }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let outcome = provider
        .chat(
            ModelRequest {
                policy: &permit.configuration.policy,
                deadline: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64
                    + 10,
                max_output_tokens: 64,
                max_total_tokens: permit.turn.reservation.total_tokens,
                prepared: &permit.prepared,
            },
            &permit.secret,
            None,
        )
        .await;
    f.state
        .finish_run_call(f.input.id, permit.turn.id, permit.turn.attempt, outcome)
        .unwrap();
    let completed = f.state.load_run(f.input.id).unwrap();
    assert_eq!(completed.summary.phase, RunPhase::Completed);
    assert_eq!(completed.summary.accounting.settled_tokens, 44);
    server.abort();
}
#[test]
fn vision_rejects_wrong_ownership_digest_capability_billing_and_corrupt_png_before_admission() {
    for case in [
        "bot",
        "digest",
        "capability",
        "billing",
        "corrupt",
        "bytes",
        "pixels",
        "count",
    ] {
        let mut f = fixture();
        let (mut reference, _) = image(&mut f);
        match case {
            "bot" => {
                let mut t = f.state.load_task(reference.task).unwrap();
                t.command.bot = BotId::new();
                tasks::save(&f.state.connection, &t).unwrap();
            }
            "digest" => reference.sha256 = CertificateFingerprint::from_der(b"wrong"),
            "capability" => {
                f.state
                    .connection
                    .execute("UPDATE inference_configurations SET verified_vision=0", [])
                    .unwrap();
            }
            "billing" => {
                f.input.configuration_revision = 1;
                let verification = f
                    .state
                    .prepare_inference_verification(
                        token(),
                        VerifyInference {
                            bot: f.input.bot,
                            revision: 2,
                        },
                    )
                    .unwrap();
                f.state
                    .finish_inference_verification(
                        verification.configuration,
                        verification.epoch,
                        Some((true, true)),
                    )
                    .unwrap();
            }
            "corrupt" => std::fs::write(
                f.state
                    .directory
                    .join("artifacts")
                    .join(format!("{}.blob", reference.artifact)),
                b"not png",
            )
            .unwrap(),
            "bytes" => {
                f.input
                    .execution_policy
                    .vision
                    .as_mut()
                    .unwrap()
                    .max_image_bytes = 1
            }
            "pixels" => {
                f.input
                    .execution_policy
                    .vision
                    .as_mut()
                    .unwrap()
                    .max_total_pixels = 1
            }
            "count" => f.input.images.push(reference.clone()),
            _ => unreachable!(),
        }
        f.input.images.push(reference);
        assert!(
            f.state.submit_run(token(), f.input.clone()).is_err(),
            "{case}"
        );
        assert!(matches!(
            f.state.load_run(f.input.id),
            Err(OperationError::NotFound)
        ));
    }
}
#[test]
fn old_text_request_still_decodes() {
    let f = fixture();
    let mut value = serde_json::to_value(&f.input).unwrap();
    value.as_object_mut().unwrap().remove("images");
    value["execution_policy"]
        .as_object_mut()
        .unwrap()
        .remove("vision");
    value["execution_policy"]
        .as_object_mut()
        .unwrap()
        .remove("vibe");
    assert_eq!(serde_json::from_value::<SubmitRun>(value).unwrap(), f.input);
}
