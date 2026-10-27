use crate::{
    manager::devices::owner_error,
    manager::{Manager, devices::DeviceLease},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use botstral_link::*;
use futures_util::{SinkExt, StreamExt};
use hyper_util::rt::TokioIo;
use sha2::{Digest, Sha256};
use std::{io, time::Duration};
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinSet,
};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

type Socket = WebSocketStream<TokioIo<hyper::upgrade::Upgraded>>;
const DEADLINE: Duration = Duration::from_secs(5);

async fn send(socket: &mut Socket, message: OwnerServerMessage) -> io::Result<()> {
    tokio::time::timeout(
        DEADLINE,
        socket.send(Message::Text(serde_json::to_string(&message)?.into())),
    )
    .await
    .map_err(io::Error::other)?
    .map_err(io::Error::other)
}

pub(crate) async fn connected(mut socket: Socket, manager: Manager) -> io::Result<()> {
    let first = tokio::time::timeout(DEADLINE, socket.next()).await;
    let hello = match first {
        Ok(Some(Ok(Message::Text(text)))) => serde_json::from_str::<OwnerHello>(&text).ok(),
        _ => None,
    };
    let Some(hello) = hello else {
        return send(
            &mut socket,
            OwnerServerMessage::Error {
                id: None,
                code: OwnerError::InvalidInput,
            },
        )
        .await;
    };
    let events = manager.subscribe_owner_events();
    let (stop, mut stopped) = watch::channel(false);
    let (identity, lease) = match manager.connect_owner(hello, stop).await {
        Ok(pair) => pair,
        Err(error) => {
            return send(
                &mut socket,
                OwnerServerMessage::Error {
                    id: None,
                    code: owner_error(error),
                },
            )
            .await;
        }
    };
    let operation = async {
        send(
            &mut socket,
            OwnerServerMessage::Welcome {
                version: OWNER_VERSION,
                manager: identity,
                device: lease.device(),
            },
        )
        .await?;
        socket_loop(socket, manager.clone(), lease, events).await
    };
    let result = tokio::select! { biased; _=stopped.changed()=>Ok(()), result=operation=>result };
    let _ = manager.disconnect_owner(lease).await;
    result
}

struct Outbound {
    message: OwnerServerMessage,
    completed: Option<oneshot::Sender<()>>,
}
impl From<OwnerServerMessage> for Outbound {
    fn from(message: OwnerServerMessage) -> Self {
        Self {
            message,
            completed: None,
        }
    }
}

async fn socket_loop(
    socket: Socket,
    manager: Manager,
    lease: DeviceLease,
    mut events: watch::Receiver<u64>,
) -> io::Result<()> {
    let (mut sink, mut stream) = socket.split();
    let (control, mut controls) = mpsc::channel::<Outbound>(8);
    let (bulk, mut chunks) = mpsc::channel::<Outbound>(1);
    let mut writer = JoinSet::new();
    writer.spawn(async move {
        loop {
            let item =
                tokio::select! { biased; item=controls.recv()=>item, item=chunks.recv()=>item };
            let Some(item) = item else {
                return Ok::<_, io::Error>(());
            };
            let frame = serde_json::to_string(&item.message)?;
            if frame.len() >= MAX_MESSAGE {
                return Err(io::Error::other("owner frame limit"));
            }
            tokio::time::timeout(DEADLINE, sink.send(Message::Text(frame.into())))
                .await
                .map_err(io::Error::other)?
                .map_err(io::Error::other)?;
            if let Some(completed) = item.completed {
                let _ = completed.send(());
            }
        }
    });
    let mut jobs = JoinSet::new();
    let idle = tokio::time::sleep(Duration::from_secs(30));
    tokio::pin!(idle);
    let mut watching = false;
    let mut applied = 0;
    let mut announced = 0;
    loop {
        tokio::select! {
            biased;
            _=&mut idle=>return Err(io::Error::other("owner dead peer")),
            _=writer.join_next()=>return Err(io::Error::other("owner writer closed")),
            result=jobs.join_next(), if !jobs.is_empty()=>{
                if !matches!(result,Some(Ok(Ok(())))) {return Err(io::Error::other("owner transfer failed"));}
            },
            incoming=stream.next()=>{
                let message=match incoming {
                    Some(Ok(Message::Text(text)))=>serde_json::from_str::<OwnerClientMessage>(&text).map_err(io::Error::other)?,
                    _=>return Ok(()),
                };
                idle.as_mut().reset(tokio::time::Instant::now()+Duration::from_secs(30));
                match message {
                    OwnerClientMessage::Ping=>control.try_send(OwnerServerMessage::Pong.into()).map_err(io::Error::other)?,
                    OwnerClientMessage::Watch {cursor}=>{watching=true;applied=cursor;announced=cursor;},
                    OwnerClientMessage::Applied {cursor}=>{
                        let head=*events.borrow();
                        if cursor<applied || cursor>head { return Err(io::Error::other("invalid owner cursor")); }
                        applied=cursor;
                    },
                    OwnerClientMessage::Call {id,operation}=>{
                        if !jobs.is_empty() {
                            control.try_send(OwnerServerMessage::Error {id:Some(id),code:OwnerError::Busy}.into()).map_err(io::Error::other)?;
                        } else {
                            jobs.spawn(transfer(manager.clone(),lease,id,*operation,bulk.clone(),control.clone()));
                        }
                    }
                }
            },
            changed=events.changed(), if watching=>{
                changed.map_err(io::Error::other)?;
                let head=*events.borrow_and_update();
                if head>announced && head>applied {
                    control.try_send(OwnerServerMessage::Notification {head}.into()).map_err(io::Error::other)?;
                    announced=head;
                }
            },
        }
    }
}

struct CountBytes(usize);
impl io::Write for CountBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > OWNER_TRANSFER_BYTES - self.0 {
            return Err(io::Error::other("owner transfer limit"));
        }
        self.0 += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

async fn transfer(
    manager: Manager,
    lease: DeviceLease,
    request: RequestId,
    operation: OwnerOperation,
    bulk: mpsc::Sender<Outbound>,
    control: mpsc::Sender<Outbound>,
) -> io::Result<()> {
    tokio::time::timeout(Duration::from_secs(OWNER_TRANSFER_SECONDS), async move {
        let permit = match manager.snapshots.clone().try_acquire_owned() {
            Ok(permit) => std::sync::Arc::new(permit),
            Err(_) => {
                control
                    .send(
                        OwnerServerMessage::Error {
                            id: Some(request),
                            code: OwnerError::Busy,
                        }
                        .into(),
                    )
                    .await
                    .map_err(io::Error::other)?;
                return Ok(());
            }
        };
        let result = match manager
            .owner_operation(lease, operation, permit.clone())
            .await
        {
            Ok(reply) => reply,
            Err(error) => {
                drop(permit);
                control
                    .send(
                        OwnerServerMessage::Error {
                            id: Some(request),
                            code: owner_error(error),
                        }
                        .into(),
                    )
                    .await
                    .map_err(io::Error::other)?;
                return Ok(());
            }
        };
        let bytes = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let mut count = CountBytes(0);
            serde_json::to_writer(&mut count, &result).map_err(io::Error::other)?;
            let mut bytes = Vec::with_capacity(count.0);
            serde_json::to_writer(&mut bytes, &result).map_err(io::Error::other)?;
            Ok::<_, io::Error>(bytes)
        })
        .await
        .map_err(io::Error::other)??;
        let id = TransferId::new();
        let chunks = bytes.len().div_ceil(OWNER_CHUNK_BYTES) as u32;
        bulk.send(
            OwnerServerMessage::TransferStart {
                id,
                request: Some(request),
                bytes: bytes.len() as u64,
                chunks,
                digest: format!("{:x}", Sha256::digest(&bytes)),
            }
            .into(),
        )
        .await
        .map_err(io::Error::other)?;
        for (index, chunk) in bytes.chunks(OWNER_CHUNK_BYTES).enumerate() {
            bulk.send(
                OwnerServerMessage::TransferChunk {
                    id,
                    index: index as u32,
                    data: STANDARD.encode(chunk),
                }
                .into(),
            )
            .await
            .map_err(io::Error::other)?;
        }
        let (completed, received) = oneshot::channel();
        bulk.send(Outbound {
            message: OwnerServerMessage::TransferComplete { id },
            completed: Some(completed),
        })
        .await
        .map_err(io::Error::other)?;
        received.await.map_err(io::Error::other)?;
        Ok(())
    })
    .await
    .map_err(io::Error::other)?
}
