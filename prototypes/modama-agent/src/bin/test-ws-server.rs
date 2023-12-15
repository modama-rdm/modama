use std::{net::SocketAddr, time::SystemTime};

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use log::{error, info};
use modama_agent::{
    common::init_logging,
    messages::{EventChannelMessage, EventChannelRequests},
    share::Share,
};
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

            ws_stream
                .send(tungstenite::Message::Text(json_msg))
                .await?;
        }
    }

    Ok(())
}

async fn get_share_list() -> EventChannelMessage {
    let foreground_share = Share {
        share_id: "c3b89492-a384-45c3-bbb0-1405cd3c63d1".to_owned(),
        share_name: "share-c3b89492-a384-45c3-bbb0-1405cd3c63d1".to_owned(),
        dest_path: "/adhoc/datamanagement/2023-11-06-ingest-stuff/".to_owned(),
        created_timestamp: SystemTime::now(),
        username: "sharef76b764d16a043ea805918305ce".to_owned(),
        password: "kubeFWCBMvw1AVTADZsM4wEXxECwBxgAAo8_9cAZ_5U".to_owned(),
    };
    let shares = vec![foreground_share.clone().into()];

    EventChannelMessage::UpdateShareList {
        shares,
        foreground_share: Some(foreground_share.into()),
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
