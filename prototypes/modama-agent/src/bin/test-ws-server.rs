use std::{fs::File, io::Read, net::SocketAddr};

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use log::{error, info};
use modama_agent::{
    common::init_logging,
    messages::{EventChannelMessage, EventChannelRequests, ShareMsg},
};
use serde::Deserialize;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{accept_async, WebSocketStream};

async fn accept_agent_connection(peer: SocketAddr, stream: TcpStream) {
    if let Err(e) = handle_connection(peer, stream).await {
        match e {
            tungstenite::Error::ConnectionClosed
            | tungstenite::Error::Protocol(_)
            | tungstenite::Error::Utf8 => (),
            err => error!("Error handling connection: {err}"),
        }
    }
}

async fn handle_message(msg: &str, ws_stream: &mut WebSocketStream<TcpStream>) -> Result<()> {
    let msg: EventChannelRequests = serde_json::from_str(msg)?;

    match msg {
        EventChannelRequests::ShareListUpdate => {
            let msg = get_share_list().await;
            let json_msg = serde_json::to_string(&msg)?;

            ws_stream.send(tungstenite::Message::Text(json_msg)).await?;
        }
    }

    Ok(())
}

#[derive(Deserialize)]
struct TestData {
    share: ShareMsg,
}

async fn get_share_list() -> EventChannelMessage {
    let mut test_data = Vec::new();
    File::open("test-data.toml")
        .expect("Need test data")
        .read_to_end(&mut test_data)
        .unwrap();
    let test_data = String::from_utf8(test_data).unwrap();
    let test_data: TestData = toml::from_str(&test_data).unwrap();
    let foreground_share = test_data.share;
    let shares = vec![foreground_share.clone()];

    EventChannelMessage::UpdateShareList {
        shares,
        foreground_share: Some(foreground_share),
    }
}

async fn handle_connection(peer: SocketAddr, stream: TcpStream) -> Result<(), tungstenite::Error> {
    let mut ws_stream = accept_async(stream).await?;

    info!("New connection: {}", peer);

    let msg = get_share_list().await;
    let json_msg = serde_json::to_string(&msg).unwrap();

    ws_stream
        .send(tungstenite::Message::Text(json_msg))
        .await
        .unwrap();

    while let Some(msg) = ws_stream.next().await {
        let msg = msg?;
        if let Ok(msg) = msg.into_text() {
            info!("Got message: {}", msg);
            handle_message(&msg, &mut ws_stream).await.unwrap(); // FIXME: unwrap -> thiserror
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    init_logging();

    let addr = "localhost:1234";
    let listener = TcpListener::bind(&addr).await.expect("Can't listen");
    while let Ok((stream, _)) = listener.accept().await {
        let peer = stream.peer_addr().expect("should have a peer address");
        tokio::spawn(accept_agent_connection(peer, stream));
    }
}
