use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use anyhow::{Context, Result};
use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use tokio::{
    net::TcpListener,
    sync::{broadcast, Mutex},
};
use yrs::encoding::read::Cursor;
use yrs::sync::{Awareness, DefaultProtocol, Message, MessageReader, Protocol, SyncMessage};
use yrs::updates::decoder::{Decode, DecoderV1};
use yrs::updates::encoder::Encode;
use yrs::{Doc, GetString, ReadTxn, Transact, Update};

const UPDATE_BUFFER_SIZE: usize = 256;

#[derive(Clone)]
pub struct YjsWsState {
    inner: Arc<YjsWsStateInner>,
}

struct YjsWsStateInner {
    awareness: Mutex<Awareness>,
    updates_tx: broadcast::Sender<(u64, Vec<u8>)>,
    next_client_id: AtomicU64,
}

impl YjsWsState {
    pub fn new(doc: Doc) -> Self {
        let (updates_tx, _) = broadcast::channel(UPDATE_BUFFER_SIZE);
        Self {
            inner: Arc::new(YjsWsStateInner {
                awareness: Mutex::new(Awareness::new(doc)),
                updates_tx,
                next_client_id: AtomicU64::new(1),
            }),
        }
    }

    pub fn router(self) -> Router {
        Router::new().route("/yjs", get(yjs_ws_route)).with_state(self)
    }

    pub async fn get_text_string(&self, name: &str) -> String {
        let awareness = self.inner.awareness.lock().await;
        let txn = awareness.doc().transact();
        txn.get_text(name).map(|text| text.get_string(&txn)).unwrap_or_default()
    }
}

impl Default for YjsWsState {
    fn default() -> Self {
        Self::new(Doc::new())
    }
}

pub async fn serve(listener: TcpListener, state: YjsWsState) -> Result<()> {
    axum::serve(listener, state.router()).await.context("daemon yjs websocket server failed")
}

async fn yjs_ws_route(ws: WebSocketUpgrade, State(state): State<YjsWsState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: YjsWsState) {
    let client_id = state.inner.next_client_id.fetch_add(1, Ordering::Relaxed);
    let mut updates_rx = state.inner.updates_tx.subscribe();

    loop {
        tokio::select! {
            incoming = socket.recv() => {
                let Some(Ok(message)) = incoming else {
                    break;
                };

                match message {
                    WsMessage::Binary(payload) => {
                        if let Err(error) = process_incoming_binary(client_id, payload.as_ref(), &state, &mut socket).await {
                            tracing::warn!(?error, "failed to process yjs websocket frame");
                            break;
                        }
                    }
                    WsMessage::Close(_) => break,
                    WsMessage::Ping(payload) => {
                        if socket.send(WsMessage::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    WsMessage::Pong(_) | WsMessage::Text(_) => {}
                }
            }
            outbound = updates_rx.recv() => {
                match outbound {
                    Ok((sender_id, payload)) if sender_id != client_id => {
                        if socket.send(WsMessage::Binary(payload.into())).await.is_err() {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

async fn process_incoming_binary(
    client_id: u64,
    payload: &[u8],
    state: &YjsWsState,
    socket: &mut WebSocket,
) -> Result<()> {
    let protocol = DefaultProtocol;
    let mut responses = Vec::new();
    let mut broadcast_updates = Vec::new();

    {
        let awareness = state.inner.awareness.lock().await;
        let mut decoder = DecoderV1::new(Cursor::new(payload));
        let mut reader = MessageReader::new(&mut decoder);

        while let Some(next_message) = reader.next() {
            let message = next_message.context("failed to decode y-sync message")?;
            match message {
                Message::Sync(SyncMessage::SyncStep1(state_vector)) => {
                    if let Some(response) = protocol
                        .handle_sync_step1(&awareness, state_vector)
                        .context("failed to process sync step 1")?
                    {
                        responses.push(response.encode_v1());
                    }

                    let server_sv = awareness.doc().transact().state_vector();
                    responses.push(Message::Sync(SyncMessage::SyncStep1(server_sv)).encode_v1());
                }
                Message::Sync(SyncMessage::SyncStep2(update)) => {
                    let decoded = Update::decode_v1(&update)
                        .context("failed to decode sync step 2 update")?;
                    protocol
                        .handle_sync_step2(&awareness, decoded)
                        .context("failed to process sync step 2")?;

                    // Step-2 carries client updates during handshake; fan it out as regular updates.
                    broadcast_updates.push(Message::Sync(SyncMessage::Update(update)).encode_v1());
                }
                Message::Sync(SyncMessage::Update(update)) => {
                    let decoded = Update::decode_v1(&update)
                        .context("failed to decode incremental update")?;
                    protocol
                        .handle_update(&awareness, decoded)
                        .context("failed to process incremental update")?;
                    broadcast_updates.push(Message::Sync(SyncMessage::Update(update)).encode_v1());
                }
                other => {
                    if let Some(response) = protocol
                        .handle_message(&awareness, other)
                        .context("failed to process y-sync message")?
                    {
                        responses.push(response.encode_v1());
                    }
                }
            }
        }
    }

    for response in responses {
        socket
            .send(WsMessage::Binary(response.into()))
            .await
            .context("failed to send y-sync response")?;
    }

    for update in broadcast_updates {
        let _ = state.inner.updates_tx.send((client_id, update));
    }

    Ok(())
}
