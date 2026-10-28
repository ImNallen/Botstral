use botstral_link::{
    ConversationMessage, EventReplay, MessageId, OwnerError, OwnerOperation, OwnerReply,
};
use botstral_owner_client::{ClientError, OwnerClient};
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args().collect();
    let root = PathBuf::from(&args[1]);
    let other_root = PathBuf::from(&args[3]);
    let mut client = OwnerClient::open(&root).unwrap();
    let mut session = client.connect().await.unwrap();
    assert!(matches!(
        client.connect().await,
        Err(ClientError::Refused(OwnerError::Busy))
    ));
    drop(client);
    assert!(matches!(
        OwnerClient::open(&root),
        Err(ClientError::Storage)
    ));
    session
        .call(OwnerOperation::Resume {
            cursor: 0,
            limit: 64,
        })
        .await
        .unwrap();
    drop(session);
    let mut client = OwnerClient::open(&root).unwrap();
    let mut session = client.connect().await.unwrap();
    std::fs::create_dir(root.join(".transfer.part")).unwrap();
    let id = MessageId::new();
    let outcome = session
        .call(OwnerOperation::AppendMessage {
            input: ConversationMessage {
                id,
                conversation: args[2].parse().unwrap(),
                text: "Committed before response storage failed".into(),
            },
        })
        .await;
    assert!(matches!(outcome, Err(ClientError::UnknownOutcome)));
    std::fs::remove_dir(root.join(".transfer.part")).unwrap();
    drop(session);
    let mut session = client.connect().await.unwrap();
    let reply = session
        .call(OwnerOperation::Resume {
            cursor: 0,
            limit: 64,
        })
        .await
        .unwrap();
    let OwnerReply::Replay(replay) = reply else {
        panic!("Expected authoritative replay");
    };
    let EventReplay::Snapshot { state, .. } = *replay else {
        panic!("Expected snapshot");
    };
    assert!(state.messages.iter().any(|message| message.id == id));
    drop(session);
    let mut other = OwnerClient::open(&other_root).unwrap();
    let mut other_session = other.connect().await.unwrap();
    assert!(matches!(
        client.recover(&mut other_session).await,
        Err(ClientError::Protocol)
    ));
    println!(
        "PASS exclusive session, retained storage lock, unknown committed mutation, and client ownership"
    );
}
