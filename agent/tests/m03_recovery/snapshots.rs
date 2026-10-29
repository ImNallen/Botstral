use super::*;

#[tokio::test]
async fn aggregate_admission_limits_keep_complete_snapshots_bounded() {
    let mut f = Fixture::new().await;
    let session = f.durable_session().await;
    let content = "\n".repeat(4096);
    let reference = format!("https://example.test/{}", "a".repeat(491));
    let mut first_conversation = None;
    for entry in 0..256 {
        let bot = if entry == 0 {
            session.bot_id
        } else {
            BotId::new()
        };
        if entry != 0 {
            assert_eq!(
                f.owner(
                    "PUT",
                    &format!("/v1/bots/{bot}"),
                    json!({"name":"Snapshot bound"})
                )
                .await
                .0,
                200
            );
        }
        let conversation = ConversationId::new();
        first_conversation.get_or_insert(conversation);
        assert_eq!(
            f.owner(
                "POST",
                "/v1/conversations",
                json!({"id":conversation,"bot":bot})
            )
            .await
            .0,
            200
        );
        assert_eq!(
            f.owner(
                "POST",
                "/v1/messages",
                json!({"id":MessageId::new(),"conversation":conversation,"text":content})
            )
            .await
            .0,
            200
        );
        assert_eq!(
            f.owner(
                "PUT",
                &format!("/v1/bots/{bot}/permission"),
                json!({"expected_revision":1,"allow":true})
            )
            .await
            .0,
            200
        );
        let sources = (0..8)
            .map(|_| json!({"type":"external","reference":reference}))
            .collect::<Vec<_>>();
        assert_eq!(f.owner("PUT", "/v1/documents", json!({"bot":bot,"kind":"instructions","expected_revision":0,"sources":sources,"markdown":content})).await.0, 200);
    }
    for _ in 0..128 {
        assert_eq!(f.owner("POST", "/v1/tasks", json!({"id":TaskId::new(),"session_id":session.id,"conversation_id":null,
            "settings":{"model":"m".repeat(128),"max_turns":1000,"max_tokens":1000000,"deadline":4000000000i64,"max_cost_micros":9007199254740991u64},
            "work":{"type":"unavailable"}})).await.0, 200);
    }
    let extra_bot = BotId::new();
    assert_eq!(
        f.owner(
            "PUT",
            &format!("/v1/bots/{extra_bot}"),
            json!({"name":"Overflow"})
        )
        .await
        .0,
        200
    );
    assert_eq!(
        f.owner(
            "POST",
            "/v1/conversations",
            json!({"id":ConversationId::new(),"bot":extra_bot})
        )
        .await
        .0,
        503
    );
    assert_eq!(f.owner("POST", "/v1/messages", json!({"id":MessageId::new(),"conversation":first_conversation.unwrap(),"text":"overflow"})).await.0, 503);
    assert_eq!(
        f.owner(
            "PUT",
            &format!("/v1/bots/{extra_bot}/permission"),
            json!({"expected_revision":1,"allow":true})
        )
        .await
        .0,
        503
    );
    assert_eq!(f.owner("PUT", "/v1/documents", json!({"bot":extra_bot,"kind":"instructions","expected_revision":0,"sources":[{"type":"external","reference":"https://example.test/source"}],"markdown":"overflow"})).await.0, 409);
    let overflow = json!({"id":TaskId::new(),"session_id":session.id,"conversation_id":null,
        "settings":{"model":"unavailable","max_turns":1,"max_tokens":1,"deadline":4000000000i64,"max_cost_micros":1},"work":{"type":"unavailable"}});
    assert_eq!(f.owner("POST", "/v1/tasks", overflow).await.0, 503);
    let (status, snapshot) = f.owner("GET", "/v1/events/0/64", json!({})).await;
    assert_eq!(status, 200);
    let bytes = serde_json::to_vec(&snapshot).unwrap().len();
    assert!(
        bytes < 32 * 1024 * 1024,
        "snapshot exceeded documented bound: {bytes}"
    );
    for collection in ["conversations", "messages", "documents", "permissions"] {
        assert_eq!(snapshot["state"][collection].as_array().unwrap().len(), 256);
    }
    assert_eq!(snapshot["state"]["tasks"].as_array().unwrap().len(), 128);
    evidence(
        "snapshot-bounds",
        json!({"serialized_bytes":bytes,"retained_tasks":128,"conversations":256,"messages":256,"documents":256,"permissions":256,"overflow_rejected":true}),
    );
    f.manager.stop().await;
}
