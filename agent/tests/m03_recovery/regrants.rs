use super::*;

#[tokio::test]
async fn identical_regrant_is_delivered_after_denial_before_delayed_report() {
    let mut f = Fixture::new().await;
    let session = f.durable_session().await;
    let task = f
        .submit(
            &session,
            WorkSpec::TestEffect {
                value: 1,
                observable: true,
            },
        )
        .await;
    let approved = f.approve(&task).await;
    let binding: ExecutionBinding =
        serde_json::from_value(approved["approval"]["binding"].clone()).unwrap();
    let mut socket = transport::connect_socket(&f.worker).await.unwrap();
    wire(&mut socket, WorkerMessage::Hello(Hello::current(true))).await;
    let ManagerMessage::Welcome { generation, .. } = read(&mut socket).await else {
        panic!("welcome")
    };
    wire(&mut socket, WorkerMessage::SnapshotApplied { generation }).await;
    let incarnation = JournalId::new();
    wire(
        &mut socket,
        WorkerMessage::RecoveryHello {
            generation,
            incarnation,
            high_water: 0,
        },
    )
    .await;
    assert!(matches!(
        read(&mut socket).await,
        ManagerMessage::InventoryRequest { offset: 0 }
    ));
    wire(
        &mut socket,
        WorkerMessage::Inventory {
            generation,
            incarnation,
            offset: 0,
            total: 0,
            entries: vec![],
        },
    )
    .await;
    assert!(matches!(
        read(&mut socket).await,
        ManagerMessage::RecoveryReady { .. }
    ));
    let ManagerMessage::Delivery(delivery) = read(&mut socket).await else {
        panic!("initial delivery")
    };
    assert_eq!(delivery.authority, Some(binding.clone()));
    assert_eq!(
        f.owner(
            "DELETE",
            &format!("/v1/tasks/{}/approval", task["id"].as_str().unwrap()),
            json!({})
        )
        .await
        .0,
        200
    );
    wire(
        &mut socket,
        WorkerMessage::Claim {
            incarnation,
            fence: delivery.fence,
            binding: binding.clone(),
        },
    )
    .await;
    assert!(matches!(
        read(&mut socket).await,
        ManagerMessage::ClaimDenied { .. }
    ));
    let regrant = f
        .owner(
            "PUT",
            &format!("/v1/tasks/{}/approval", task["id"].as_str().unwrap()),
            json!({"expires_at":binding.expires_at}),
        )
        .await;
    assert_eq!(regrant.0, 200);
    assert_eq!(
        regrant.1["approval"]["binding"],
        approved["approval"]["binding"]
    );
    let reports = [ExecutionEvent::Accepted, ExecutionEvent::ApprovalRequired]
        .into_iter()
        .enumerate()
        .map(|(index, event)| DurableReport {
            sequence: index as u64 + 1,
            command: delivery.body.id,
            execution: binding.execution,
            event,
        })
        .collect();
    wire(
        &mut socket,
        WorkerMessage::Reports {
            generation,
            incarnation,
            high_water: 2,
            reports,
        },
    )
    .await;
    let mut redelivered = false;
    let mut acknowledged = false;
    while !(redelivered && acknowledged) {
        let message = tokio::time::timeout(Duration::from_secs(2), read(&mut socket))
            .await
            .expect("identical regrant must be redelivered without reconnecting");
        match message {
            ManagerMessage::Delivery(renewed) => {
                assert_eq!(renewed, delivery);
                redelivered = true;
            }
            ManagerMessage::ReportAck { through: 2 } => acknowledged = true,
            other => panic!("redelivery or receipt expected, got {other:?}"),
        }
    }
    wire(
        &mut socket,
        WorkerMessage::Claim {
            incarnation,
            fence: delivery.fence,
            binding: binding.clone(),
        },
    )
    .await;
    assert!(
        matches!(read(&mut socket).await, ManagerMessage::Permit { binding: permitted, .. } if permitted == binding)
    );
    evidence(
        "identical-regrant",
        json!({"identical_binding_redelivered":true,"delayed_report_acknowledged":true,"claim_permitted_without_reconnect":true}),
    );
    socket.close(None).await.unwrap();
    f.manager.stop().await;
}
