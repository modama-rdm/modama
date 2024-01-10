use std::{
    fs::File,
    io::{BufReader, Read},
    net::SocketAddr,
    sync::Arc,
};

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use log::{error, info};
use modama_agent::cert_helpers::AgentId;
use modama_common::{
    logging::init_logging,
    messages::{EventChannelMessage, EventChannelRequests, Share},
};
use rustls_pemfile::{certs, private_key};
use serde::Deserialize;
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{
    rustls::{
        pki_types::{CertificateDer, PrivateKeyDer},
        server::WebPkiClientVerifier,
        RootCertStore, ServerConfig,
    },
    TlsAcceptor, TlsStream,
};
use tokio_tungstenite::{accept_async, WebSocketStream};

async fn accept_agent_connection(peer: SocketAddr, stream: TcpStream, acceptor: TlsAcceptor) {
    let stream = acceptor.accept(stream).await.unwrap();
    let (_, server_connection) = stream.get_ref();

    let peer_certs = server_connection.peer_certificates().unwrap();
    let peer_cert = peer_certs.get(0).unwrap();

    let agent_id = AgentId::from_cert(peer_cert).unwrap();

    info!("agent id: {agent_id:?}");

    if let Err(e) = handle_connection(peer, stream.into()).await {
        match e {
            tungstenite::Error::ConnectionClosed
            | tungstenite::Error::Protocol(_)
            | tungstenite::Error::Utf8 => (),
            err => error!("Error handling connection: {err}"),
        }
    }
}

async fn handle_message(
    msg: &str,
    ws_stream: &mut WebSocketStream<TlsStream<TcpStream>>,
) -> Result<()> {
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
    share: Share,
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

async fn handle_connection(
    peer: SocketAddr,
    stream: TlsStream<TcpStream>,
) -> Result<(), tungstenite::Error> {
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

fn get_root_cert_store() -> Result<RootCertStore> {
    let mut root_cert_store = RootCertStore::empty();

    let ca_certs = File::open("./certs/ca/ca.cert.pem")?;
    for cert in certs(&mut BufReader::new(ca_certs)) {
        root_cert_store.add(cert?)?;
    }

    Ok(root_cert_store)
}

fn get_server_chain() -> Result<Vec<CertificateDer<'static>>> {
    let cert_file = File::open("./certs/servers/localhost.chain.pem")?;

    // FIXME: fail instead of flattening
    Ok(certs(&mut BufReader::new(cert_file)).flatten().collect())
}

fn get_server_key() -> Result<PrivateKeyDer<'static>> {
    let key_file = File::open("./certs/servers/localhost.priv.pem")?;
    let pk = private_key(&mut BufReader::new(key_file))?.expect("private key file contains a key");
    Ok(pk)
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();

    let roots = get_root_cert_store()?;
    let client_cert_verifier = WebPkiClientVerifier::builder(roots.into()).build()?;

    let config = ServerConfig::builder()
        .with_client_cert_verifier(client_cert_verifier)
        .with_single_cert(get_server_chain()?, get_server_key()?)?;

    let acceptor = TlsAcceptor::from(Arc::new(config));

    let addr = "localhost:1234";
    let listener = TcpListener::bind(&addr).await.expect("Can't listen");
    while let Ok((stream, _)) = listener.accept().await {
        let peer = stream.peer_addr().expect("should have a peer address");
        let acc = acceptor.clone();
        tokio::spawn(accept_agent_connection(peer, stream, acc));
    }

    Ok(())
}
