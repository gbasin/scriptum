use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use scriptum_daemon::rpc::yjs_ws::{serve, YjsWsState};
use tokio::net::TcpListener;
use tokio::time::{timeout, Instant};
use tokio_tungstenite::{
    connect_async, tungstenite::Message as WsMessage, MaybeTlsStream, WebSocketStream,
};
use yrs::sync::{Awareness, DefaultProtocol, Message, Protocol, SyncMessage};
use yrs::updates::encoder::Encode;
use yrs::{Doc, GetString, ReadTxn, Text, Transact};

type ClientSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

#[tokio::test]
async fn two_ws_clients_sync_over_yjs_endpoint() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("test listener should bind");
    let addr = listener.local_addr().expect("listener should expose local address");

    let server_state = YjsWsState::default();
    let state_for_server = server_state.clone();
    let server_task = tokio::spawn(async move {
        serve(listener, state_for_server).await.expect("yjs ws server should run");
    });

    let protocol = DefaultProtocol;

    let (mut client_a_socket, _) =
        connect_async(format!("ws://{addr}/yjs")).await.expect("client A should connect");
    let client_a = Awareness::new(Doc::with_client_id(1));
    {
        let text = client_a.doc().get_or_insert_text("content");
        let mut txn = client_a.doc().transact_mut();
        text.push(&mut txn, "from-a");
    }
    handshake(&mut client_a_socket, &client_a, &protocol).await;

    let (mut client_b_socket, _) =
        connect_async(format!("ws://{addr}/yjs")).await.expect("client B should connect");
    let client_b = Awareness::new(Doc::with_client_id(2));
    handshake(&mut client_b_socket, &client_b, &protocol).await;

    assert_eq!(text_content(&client_b), "from-a");

    let incremental_update = {
        let text = client_b.doc().get_or_insert_text("content");
        let mut txn = client_b.doc().transact_mut();
        text.push(&mut txn, " + b");
        txn.encode_update_v1()
    };

    let update_payload = Message::Sync(SyncMessage::Update(incremental_update)).encode_v1();
    client_b_socket
        .send(WsMessage::Binary(update_payload.into()))
        .await
        .expect("client B should send incremental update");

    let expected = "from-a + b";
    let deadline = Instant::now() + Duration::from_secs(2);
    while text_content(&client_a) != expected {
        assert!(
            Instant::now() < deadline,
            "client A did not receive broadcasted incremental update"
        );

        let incoming = recv_binary(&mut client_a_socket).await;
        let responses =
            protocol.handle(&client_a, &incoming).expect("client A should decode y-sync message");
        for response in responses {
            client_a_socket
                .send(WsMessage::Binary(response.encode_v1().into()))
                .await
                .expect("client A should send protocol response");
        }
    }

    assert_eq!(server_state.get_text_string("content").await, expected);

    let _ = client_a_socket.close(None).await;
    let _ = client_b_socket.close(None).await;
    server_task.abort();
}

async fn handshake(socket: &mut ClientSocket, awareness: &Awareness, protocol: &DefaultProtocol) {
    let step1 = Message::Sync(SyncMessage::SyncStep1(awareness.doc().transact().state_vector()))
        .encode_v1();
    socket.send(WsMessage::Binary(step1.into())).await.expect("client should send sync step 1");

    // Server responds with step-2 (its state) followed by step-1 (requesting client's state).
    for _ in 0..2 {
        let incoming = recv_binary(socket).await;
        let responses = protocol
            .handle(awareness, &incoming)
            .expect("client should decode y-sync handshake message");

        for response in responses {
            socket
                .send(WsMessage::Binary(response.encode_v1().into()))
                .await
                .expect("client should send handshake response");
        }
    }
}

async fn recv_binary(socket: &mut ClientSocket) -> Vec<u8> {
    loop {
        let next = timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("timed out waiting for websocket frame");
        let message =
            next.expect("websocket should remain open").expect("websocket read should succeed");

        match message {
            WsMessage::Binary(payload) => return payload.to_vec(),
            WsMessage::Ping(payload) => {
                socket
                    .send(WsMessage::Pong(payload))
                    .await
                    .expect("websocket should reply to ping");
            }
            WsMessage::Close(_) => panic!("websocket closed unexpectedly"),
            WsMessage::Text(_) | WsMessage::Pong(_) | WsMessage::Frame(_) => {}
        }
    }
}

fn text_content(awareness: &Awareness) -> String {
    let txn = awareness.doc().transact();
    txn.get_text("content").map(|text| text.get_string(&txn)).unwrap_or_default()
}
