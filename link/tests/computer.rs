use botstral_link::*;

#[test]
fn owner_computer_command_is_first_class_and_strict() {
    let value = serde_json::json!({"type":"computer", "spec": {
        "profile":"linux_playwright_v1",
        "account":{"uid":1000,"gid":1000},
        "runtime": {"sha256":"a".repeat(64)},
        "deadline":4000000000i64,
        "limits":{"runtime_seconds":60,"output_bytes":1024,"processes":128,"memory_bytes":1073741824u64},
        "viewport":{"width":800,"height":600},
        "operations":[{"type":"navigate","url":"https://example.org/"},
                      {"type":"capture_text","selector":"body","output":"text"}],
        "outputs":[{"slot":"text","name":"body.txt","media_type":"text_plain","max_bytes":4096}]
    }});
    let work: WorkSpec =
        serde_json::from_value(value.clone()).expect("owner API must accept typed Computer work");
    assert_eq!(serde_json::to_value(work).unwrap(), value);
    let mut invalid = value;
    invalid["spec"]["operations"][0] =
        serde_json::json!({"type":"evaluate","script":"arbitrary()"});
    assert!(serde_json::from_value::<WorkSpec>(invalid).is_err());
}

fn spec() -> ComputerSpec {
    ComputerSpec {
        profile: ComputerProfile::LinuxPlaywrightV1,
        account: ProcessAccount {
            uid: 1000,
            gid: 1000,
        },
        runtime: ComputerRuntime {
            sha256: CertificateFingerprint::from_der(b"runtime"),
        },
        deadline: 4000000000,
        limits: ProcessLimits {
            runtime_seconds: 60,
            output_bytes: 1024,
            processes: 128,
            memory_bytes: 1073741824,
        },
        viewport: Viewport {
            width: 800,
            height: 600,
        },
        operations: vec![ComputerOperation::ExportFile {
            path: "/home/shared/approved.txt".into(),
            output: "file".into(),
        }],
        outputs: vec![OutputDeclaration {
            slot: "file".into(),
            name: "file.txt".into(),
            media_type: MediaType::TextPlain,
            max_bytes: MAX_ARTIFACT_BYTES,
        }],
    }
}
#[test]
fn computer_bounds_and_immutable_inputs_preserve_process_ceilings() {
    let original = spec();
    original.validate().unwrap();
    assert!(
        original.limits.validate().is_err(),
        "Computer ceilings cannot pass Process validation"
    );
    let value = serde_json::to_value(&original).unwrap();
    for field in [
        "account",
        "runtime",
        "deadline",
        "limits",
        "viewport",
        "operations",
        "outputs",
    ] {
        let mut changed = value.clone();
        match field {
            "account" => changed[field]["uid"] = 1001.into(),
            "runtime" => changed[field]["sha256"] = "b".repeat(64).into(),
            "deadline" => changed[field] = 4000000001i64.into(),
            "limits" => changed[field]["memory_bytes"] = 536870912.into(),
            "viewport" => changed[field]["width"] = 900.into(),
            "operations" => changed[field][0]["path"] = "/approved/outside/workspace".into(),
            "outputs" => changed[field][0]["max_bytes"] = 100.into(),
            _ => unreachable!(),
        }
        assert_ne!(
            CertificateFingerprint::from_der(&serde_json::to_vec(&value).unwrap()),
            CertificateFingerprint::from_der(&serde_json::to_vec(&changed).unwrap()),
            "{field}"
        );
    }
    let mut bad = original.clone();
    bad.operations.push(ComputerOperation::Screenshot {
        output: "invented".into(),
    });
    assert!(bad.validate().is_err());
    let mut bad = original.clone();
    bad.outputs[0].name = "unsafe\r\nheader".into();
    assert!(bad.validate().is_err());
    let mut bad = original.clone();
    bad.operations = vec![ComputerOperation::Click {
        selector: "s".repeat(257),
    }];
    assert!(bad.validate().is_err());
    let mut bad = original.clone();
    bad.outputs[0].max_bytes = MAX_ARTIFACT_BYTES + 1;
    assert!(bad.validate().is_err());
    let mut bad = original.clone();
    bad.outputs = (0..3)
        .map(|i| OutputDeclaration {
            slot: format!("slot{i}"),
            ..original.outputs[0].clone()
        })
        .collect();
    assert!(bad.validate().is_err());
    let mut maximal = original;
    maximal
        .operations
        .extend((0..3).map(|_| ComputerOperation::Fill {
            selector: "input".into(),
            text: "x".repeat(512),
        }));
    assert!(serde_json::to_vec(&maximal).unwrap().len() > MAX_COMPUTER_SPEC);
    assert!(maximal.validate().is_err());
}
#[test]
fn bounded_artifact_chunks_fit_the_existing_transport_envelope() {
    let message = Envelope::new(WorkerMessage::ArtifactChunk {
        chunk: ArtifactChunk {
            journal: JournalId::new(),
            execution: ExecutionId::new(),
            slot: "x".repeat(24),
            id: ArtifactId::new(),
            offset: MAX_ARTIFACT_BYTES,
            hex: "ff".repeat(ARTIFACT_CHUNK_BYTES),
        },
    });
    let encoded = serde_json::to_vec(&message).unwrap();
    assert!(encoded.len() < MAX_MESSAGE);
    assert_eq!(decode::<WorkerMessage>(&encoded).unwrap(), message);
    let mut maximal = spec();
    if let ComputerOperation::ExportFile { path, .. } = &mut maximal.operations[0] {
        *path = format!("/{}", "x".repeat(511));
    }
    maximal
        .operations
        .extend((0..2).map(|_| ComputerOperation::Fill {
            selector: "input".into(),
            text: "x".repeat(512),
        }));
    let excess = serde_json::to_vec(&maximal).unwrap().len() - MAX_COMPUTER_SPEC;
    if let Some(ComputerOperation::Fill { text, .. }) = maximal.operations.last_mut() {
        text.truncate(text.len() - excess);
    }
    assert_eq!(
        serde_json::to_vec(&maximal).unwrap().len(),
        MAX_COMPUTER_SPEC
    );
    maximal.validate().unwrap();
    let body = CommandBody {
        id: CommandId::new(),
        owner: OwnerId::new(),
        task: TaskId::new(),
        bot: BotId::new(),
        worker: WorkerId::new(),
        session: ComputerSessionId::new(),
        operation: OperationId::new(),
        execution: ExecutionId::new(),
        work: WorkSpec::Computer {
            spec: Box::new(maximal),
        },
    };
    let entries = vec![
        JournalEntry {
            body,
            phase: JournalPhase::Admitted
        };
        REPORT_PAGE
    ];
    assert!(serde_json::to_vec(&entries).unwrap().len() < PAGE_BYTES);
}
