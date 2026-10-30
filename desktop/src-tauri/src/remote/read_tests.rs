use super::*;

fn envelope(command: IntentCommand) -> Envelope {
    Envelope {
        generation: 1,
        command,
        reply: oneshot::channel().0,
    }
}
fn poll() -> Envelope {
    envelope(IntentCommand::CredentialUses {
        result: oneshot::channel().0,
    })
}

#[tokio::test]
async fn credential_polling_waits_for_the_active_read() {
    let (tx, mut rx) = mpsc::channel(16);
    let (release, ready) = oneshot::channel();
    let mut read = tokio::spawn(async move {
        let mut pending = VecDeque::new();
        let result = finish_read(
            async {
                ready.await.unwrap();
                Ok(())
            },
            &mut rx,
            &mut pending,
        )
        .await;
        (result, pending)
    });
    tx.send(poll()).await.unwrap();
    tx.send(envelope(IntentCommand::LoginItems {
        result: oneshot::channel().0,
    }))
    .await
    .unwrap();
    tx.send(poll()).await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut read)
            .await
            .is_err(),
        "Background polling must leave the active owner read running"
    );
    release.send(()).unwrap();
    let (result, mut pending) = read.await.unwrap();
    assert!(matches!(result, ReadResult::Done(Ok(()))));
    assert_eq!(pending.len(), 3);
    assert!(matches!(
        pending.pop_front().unwrap().command,
        IntentCommand::CredentialUses { .. }
    ));
    assert!(matches!(
        pending.pop_front().unwrap().command,
        IntentCommand::LoginItems { .. }
    ));
    assert!(matches!(
        pending.pop_front().unwrap().command,
        IntentCommand::CredentialUses { .. }
    ));
}

#[tokio::test]
async fn authority_change_interrupts_a_read_ahead_of_background_polls() {
    let (tx, mut rx) = mpsc::channel(16);
    tx.send(poll()).await.unwrap();
    tx.send(envelope(IntentCommand::Suspend)).await.unwrap();
    let mut pending = VecDeque::new();
    let result = finish_read(std::future::pending(), &mut rx, &mut pending).await;
    assert!(matches!(
        result,
        ReadResult::Interrupted(Some(Envelope {
            command: IntentCommand::Suspend,
            ..
        }))
    ));
    assert_eq!(pending.len(), 1);
}

#[tokio::test]
async fn polling_backlog_is_bounded_without_delaying_authority_changes() {
    let (tx, mut rx) = mpsc::channel(16);
    let mut pending: VecDeque<_> = (0..16).map(|_| poll()).collect();
    let (reply, refused) = oneshot::channel();
    tx.send(Envelope { reply, ..poll() }).await.unwrap();
    tx.send(envelope(IntentCommand::Suspend)).await.unwrap();
    let result = finish_read(std::future::pending(), &mut rx, &mut pending).await;
    assert!(matches!(
        result,
        ReadResult::Interrupted(Some(Envelope {
            command: IntentCommand::Suspend,
            ..
        }))
    ));
    assert_eq!(pending.len(), 16);
    assert!(matches!(refused.await.unwrap(), Err(error) if error == "busy"));
}
