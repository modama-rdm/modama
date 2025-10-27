use std::{
    collections::HashMap,
    fs::File,
    io::{self, Read},
    net::{Ipv4Addr, SocketAddr},
    ops::DerefMut,
    path::Path,
    sync::Arc,
    time::Duration,
};

use log::{error, warn};
use modama_common::tokens::{BoxFutureToken, TokenError, TokenProvider};
use reqwest::{Certificate, Client, StatusCode};
use serde::{Deserialize, Serialize};
use tokio::{
    net::UdpSocket,
    spawn,
    sync::Mutex,
    time::{sleep, Instant},
};
use url::Url;

pub const NOTIFY_HEADER: &str = "MODAMA-AGENT-NOTIFY";
pub const NOTIFY_PORT: u32 = 5055;
pub const NOTIFY_MCAST_ADDR: Ipv4Addr = const { Ipv4Addr::new(239, 192, 0, 1) };

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveryInner {
    pub version: u32,
    pub api: String,
}

// TODO: move to modama-common
#[derive(Debug, PartialEq, Eq)]
pub struct DiscoveryMessage {
    pub inner: DiscoveryInner,
}

impl DiscoveryMessage {
    /// Serialize the discovery message to a String.
    pub fn serialize(&self) -> String {
        #[allow(clippy::unwrap_used)] // infalliable
        let payload = serde_json::to_string(&self.inner).unwrap();
        format!("{NOTIFY_HEADER}\n{payload}",)
    }

    pub fn api_url(&self) -> &str {
        &self.inner.api
    }

    /// Try to parse from the string representation to a `DiscoveryMessage`,
    /// returning None if this is not possible.
    pub fn try_parse(value: &str) -> Option<Self> {
        if !value.contains("\n") {
            return None;
        }
        let mut parts = value.split("\n");
        let header = parts.next()?;
        if header != NOTIFY_HEADER {
            return None;
        }
        let payload = parts.next()?;
        Some(Self {
            inner: serde_json::from_str(payload).ok()?,
        })
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct RemoteAgent {
    url: Url,
}

#[derive(Clone, Debug)]
pub struct RemoteAgents {
    agents: Arc<Mutex<HashMap<RemoteAgent, Instant>>>,
}

impl RemoteAgents {
    pub async fn new() -> Self {
        Self {
            agents: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Mark an agent as seen
    pub async fn seen(&self, agent: &RemoteAgent) {
        let mut agents = self.agents.lock().await;
        let map: &mut HashMap<RemoteAgent, Instant> = agents.deref_mut();
        map.insert(agent.clone(), Instant::now());
    }

    /// Access agents for iteration
    pub async fn agents(&self) -> HashMap<RemoteAgent, Instant> {
        self.agents.lock().await.clone()
    }

    /// Forget a remote agent if we couldn't reach it. Could be improved such
    /// that we remember that it failed instead, and not try again if it
    /// recently failed (or put it at the back of the list). For example if an
    /// agent can multicast, but can't be reached because of a host-based
    /// firewall.
    pub async fn report_failure(&self, agent: &RemoteAgent) {
        let mut agents = self.agents.lock().await;
        let map: &mut HashMap<RemoteAgent, Instant> = agents.deref_mut();
        map.insert(agent.clone(), Instant::now());
    }
}

#[derive(Debug)]
pub struct LocalNetTokenProvider {
    client: Client,
    agents: RemoteAgents,
}

impl TokenProvider for LocalNetTokenProvider {
    fn fetch_token(self: Arc<Self>) -> BoxFutureToken {
        let myself = Arc::clone(&self);
        Box::pin(async move { myself.get_token_from_local_net().await })
    }
}

async fn create_mcast_socket(
    port: u32,
    group: Ipv4Addr,
    local: Ipv4Addr,
) -> Result<UdpSocket, std::io::Error> {
    let group_w_port = format!("{group}:{port}", group = group, port = port);
    let socket = UdpSocket::bind(&group_w_port).await?;
    // let group_addr: Ipv4Addr = group.parse()?;
    // let local_addr: Ipv4Addr = local.parse()?;
    socket.join_multicast_v4(group, local)?;
    Ok(socket)
}

async fn try_recv_discovery_message(
    socket: &UdpSocket,
) -> Result<Option<(DiscoveryMessage, SocketAddr)>, io::Error> {
    let mut buf = [0u8; 1024];
    let (size, addr) = socket.recv_from(&mut buf).await?;
    let filled_part = &buf[..size];
    let recv_str = if let Ok(s) = str::from_utf8(filled_part) {
        s
    } else {
        return Ok(None);
    };
    Ok(DiscoveryMessage::try_parse(recv_str).map(|msg| (msg, addr)))
}

async fn handle_discovery_msg(msg: &DiscoveryMessage, agents: &mut RemoteAgents) {
    if let Ok(url) = Url::try_from(msg.api_url()) {
        agents.seen(&RemoteAgent { url }).await;
    } else {
        warn!(
            "handle_discovery_msg: invalid URL `{}` encountered",
            msg.api_url()
        )
    }
}

/// Listen for multicast messages on "local interfaces".
/// TODO: what is local? how do we configure/discover this properly?
/// For now, we support one local interface only.
/// sysinfo crate? doesn't look complete to me (ipv6 comes to mind...)
///
/// We can put the current state into a shared object (Arc<tokio::sync::Mutex<...>>)
async fn discovery_loop(local: Ipv4Addr, mut agents: RemoteAgents) {
    'outer: loop {
        let socket = match create_mcast_socket(NOTIFY_PORT, NOTIFY_MCAST_ADDR, local).await {
            Ok(socket) => socket,
            Err(err) => {
                error!("error creating multicast socket (`{err}`), retrying...");
                sleep(Duration::from_millis(5000)).await;
                continue 'outer;
            }
        };
        'inner: loop {
            match try_recv_discovery_message(&socket).await {
                Ok(Some((msg, _addr))) => handle_discovery_msg(&msg, &mut agents).await,
                Ok(None) => continue 'inner,
                Err(err) => {
                    error!("error receiving multicast message (`{err}`), re-creating socket...");
                    sleep(Duration::from_millis(1000)).await;
                    continue 'outer;
                }
            }
        }
    }
}

impl LocalNetTokenProvider {
    pub async fn new(ca_cert_file: &Path, local_addr: Ipv4Addr) -> Result<Self, TokenError> {
        let mut ca_certs = File::open(ca_cert_file)?;
        let mut ca_pem: Vec<u8> = Vec::new();
        ca_certs.read_to_end(&mut ca_pem)?;
        let root_cert = Certificate::from_pem(&ca_pem)?;
        let client = reqwest::ClientBuilder::new()
            .add_root_certificate(root_cert)
            .build()?;

        let agents = RemoteAgents::new().await;

        let bg_agents = agents.clone();
        let _handle = spawn(async move { discovery_loop(local_addr, bg_agents).await });

        Ok(Self { client, agents })
    }

    async fn get_token_from_local_net(&self) -> Result<String, TokenError> {
        // idea: just iterate over known agents sorted by last seen
        let agents = self.agents.agents().await;
        let mut agents: Vec<(RemoteAgent, Instant)> = agents.into_iter().collect();
        agents.sort_by_key(|(_, ts)| *ts);

        // latest first:
        for agent in agents.iter().rev().map(|t| &t.0) {
            let resp = match self.client.put(agent.url.clone()).send().await {
                Ok(r) => r,
                Err(e) => {
                    warn!("error from discovered agent {}: {e}", agent.url);
                    sleep(Duration::from_millis(200)).await;
                    continue;
                }
            };
            if resp.status() == StatusCode::OK {
                let text = match resp.text().await {
                    Ok(t) => t,
                    Err(e) => {
                        warn!("error reading response text from {} (`{e}`), trying next agent", agent.url);
                        sleep(Duration::from_millis(200)).await;
                        continue;
                    }
                };
                return Ok(text)
            } else {
                warn!("bad response code {} from from {}, trying next agent", resp.status(), agent.url);
                sleep(Duration::from_millis(200)).await;
                continue;
            }
        }

        Err(TokenError::NoSource)
    }
}

#[cfg(test)]
mod tests {
    use crate::local_net_token_provider::{DiscoveryInner, DiscoveryMessage};

    #[test]
    fn test_disco_parse_invalid_1() {
        assert!(DiscoveryMessage::try_parse("").is_none())
    }

    #[test]
    fn test_disco_parse_invalid_2() {
        assert!(DiscoveryMessage::try_parse("stuff").is_none())
    }

    #[test]
    fn test_disco_parse_invalid_3() {
        assert!(DiscoveryMessage::try_parse("MODAMA-AGENT-NOTIFY").is_none())
    }

    #[test]
    fn test_disco_parse_invalid_json() {
        assert!(DiscoveryMessage::try_parse("MODAMA-AGENT-NOTIFY\n{}").is_none())
    }

    #[test]
    fn test_disco_parse_invalid_4() {
        assert!(DiscoveryMessage::try_parse("stuff\n{}").is_none())
    }

    #[test]
    fn test_disco_parse_invalid_5() {
        assert!(DiscoveryMessage::try_parse(
            "stuff\n{\"version\": 1, \"api\": \"http://localhost:1234/api/\"}"
        )
        .is_none())
    }

    #[test]
    fn test_disco_parse_valid() {
        assert_eq!(
            DiscoveryMessage::try_parse(
                "MODAMA-AGENT-NOTIFY\n{\"version\": 1, \"api\": \"http://localhost:1234/api/\"}"
            ),
            Some(DiscoveryMessage {
                inner: DiscoveryInner {
                    version: 1,
                    api: "http://localhost:1234/api/".to_owned()
                }
            })
        )
    }
}
