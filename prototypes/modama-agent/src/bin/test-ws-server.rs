use std::{net::SocketAddr, time::SystemTime};

use anyhow::Result;
use futures_util::{StreamExt, SinkExt};
use log::{error, info};
use modama_agent::{common::init_logging, messages::EventChannelMessage, share::Share};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::accept_async;

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

async fn handle_connection(peer: SocketAddr, stream: TcpStream) -> Result<(), tungstenite::Error> {
    let mut ws_stream = accept_async(stream).await?;

    info!("New connection: {}", peer);

    let foreground_share = Share {
        share_id: "c3b89492-a384-45c3-bbb0-1405cd3c63d1".to_owned(),
        share_name: "share-c3b89492-a384-45c3-bbb0-1405cd3c63d1".to_owned(),
        dest_path: "/adhoc/datamanagement/2023-11-06-ingest-stuff/".to_owned(),
        created_timestamp: SystemTime::now(),
        username: "sharef76b764d16a043ea805918305ce".to_owned(),
        password: "kubeFWCBMvw1AVTADZsM4wEXxECwBxgAAo8_9cAZ_5U".to_owned(),
    };
    let shares = vec![foreground_share.clone().into()];
    let msg = EventChannelMessage::UpdateShareList {
        shares,
        foreground_share: Some(foreground_share.into()),
    };

    let json_msg = serde_json::to_string(&msg).unwrap();

    ws_stream.send(tungstenite::Message::Text(json_msg)).await.unwrap();

    while let Some(msg) = ws_stream.next().await {
        let msg = msg?;
        if msg.is_text() {
            info!("got message: {}", msg.to_text().unwrap());
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
