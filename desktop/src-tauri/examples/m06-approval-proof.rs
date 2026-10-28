use botstral_desktop::remote::{ActionReview, RemoteSnapshot};
use botstral_link::*;
use serde_json::json;
use std::{collections::BTreeMap, fs, path::PathBuf};
use uuid::Uuid;

fn snapshot(work: WorkSpec) -> RemoteSnapshot {
    let id = Uuid::from_u128(0x00000000000040008000000000000001);
    let command: CommandBody = serde_json::from_value(json!({
        "id":id,"owner":id,"task":id,"bot":id,"worker":id,"session":id,
        "operation":id,"execution":id,"work":work
    }))
    .unwrap();
    RemoteSnapshot {
        routine_panel: Default::default(),
        generation: 1,
        revision: 1,
        status: "online".into(),
        issue: None,
        pairing: None,
        candidate: None,
        workspace: None,
        review: Some(ActionReview {
            id,
            task: command.task,
            command,
            permission_revision: 7,
            approval: ApprovalView {
                id: ApprovalId::new(),
                binding: None,
                permission_revision: 7,
                revoked: false,
                consumed: false,
            },
            expires_at: 2_000_000_000,
        }),
        intents: vec![],
    }
}

fn main() {
    let output = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .expect("new output file required"),
    );
    let account = ProcessAccount {
        uid: 1000,
        gid: 1000,
    };
    let limits = ProcessLimits {
        runtime_seconds: 60,
        output_bytes: 4096,
        processes: 16,
        memory_bytes: 134_217_728,
    };
    let before = snapshot(WorkSpec::Process {
        spec: Box::new(ProcessSpec {
            profile: ProcessProfile::LinuxSystemdV1,
            account,
            limits: limits.clone(),
            executable: "/usr/bin/printf".into(),
            args: vec!["Verified file output\\n".into()],
            environment: BTreeMap::new(),
            cwd: "/home/botstral/workspace".into(),
            deadline: 2_000_000_000,
        }),
    });
    let after = snapshot(WorkSpec::Connector {
        spec: Box::new(ConnectorSpec {
            operation: ConnectorOperation::Call {
                proposal: ConnectorProposal {
                    tool: ConnectorToolBinding {
                        connection: ConnectionVersion {
                            connection: "00000000-0000-4000-8000-000000000002".parse().unwrap(),
                            revision: 2,
                            credential_epoch: 2,
                        },
                        catalog_revision: 1,
                        grant_revision: 7,
                        name: "write_receipt".into(),
                        schema_digest: CertificateFingerprint::from_der(
                            b"synthetic screenshot schema",
                        ),
                    },
                    arguments: json!({"name":"report.txt","text":"Verified connector output"}),
                },
            },
            runner: ConnectorRunner {
                account,
                limits,
                runtime: CertificateFingerprint::from_der(b"synthetic screenshot runtime"),
            },
            deadline: 2_000_000_000,
            result: OutputDeclaration {
                slot: "connector".into(),
                name: "connector.json".into(),
                media_type: MediaType::TextPlain,
                max_bytes: 4096,
            },
        }),
    });
    let value = json!({"before":before,"after":after});
    let action = value["after"]["review"]["action"].as_str().unwrap();
    assert!(action.contains("Run connector operation"));
    assert!(action.contains("report.txt") && action.contains("\"grant_revision\": 7"));
    assert!(action.contains("Complete execution details"));
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)
        .unwrap();
    serde_json::to_writer_pretty(file, &value).unwrap();
}
