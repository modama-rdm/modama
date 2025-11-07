use modama_common::agent::AgentId;
use tokio::net::TcpStream;
use tokio_rustls::server::TlsStream;

pub fn agent_id_from_stream(stream: &TlsStream<TcpStream>) -> Option<AgentId> {
    let (_, server_connection) = stream.get_ref();
    let peer_certs = server_connection.peer_certificates()?;
    let peer_cert = peer_certs.first()?;
    AgentId::from_cert(peer_cert).ok()
}
