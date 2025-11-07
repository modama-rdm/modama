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

use log::{error, info, warn};
use modama_common::tokens::{BoxFutureToken, TokenError, TokenProvider, TokenSignResponse};
use reqwest::{Certificate, Client, StatusCode};
use tokio::{
    net::UdpSocket,
    spawn,
    sync::Mutex,
    task::JoinHandle,
    time::{sleep, Instant},
};
use url::Url;

use crate::{
    local_net_common::{create_mcast_socket, DiscoveryMessage, NOTIFY_MCAST_ADDR, NOTIFY_PORT},
    local_net_token_publisher::{get_private_addresses, task_exists_and_running},
};

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
        Box::pin(async move { myself.token_from_local_net().await })
    }
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

async fn discovery_loop_starter(bg_agents: RemoteAgents) {
    let mut discovery_tasks: HashMap<Ipv4Addr, JoinHandle<()>> = HashMap::new();

    loop {
        match get_private_addresses() {
            Ok(addrs) => {
                // when new interfaces appear, start discovery loop on them;
                // when interfaces disappear, the tasks should just kill itself
                for addr in addrs.iter() {
                    info!("starting discovery task on {addr}");
                    let addr = *addr;
                    if !task_exists_and_running(&discovery_tasks, addr).await {
                        let agents = bg_agents.clone();
                        let handle = spawn(async move { discovery_loop(addr, agents).await });
                        discovery_tasks.insert(addr, handle);
                    }
                }
            }
            Err(e) => {
                error!("error while discovering local interfaces: {e}");
                sleep(Duration::from_secs(10)).await;
                continue;
            }
        };
        sleep(Duration::from_secs(1)).await;
    }
}

#[derive(Copy, Clone, Debug)]
pub enum DiscoveryAddr {
    AnyPrivate,
    V4(Ipv4Addr),
}

impl LocalNetTokenProvider {
    pub async fn new_with_pem(pem: &[u8], local_addr: DiscoveryAddr) -> Result<Self, TokenError> {
        let root_cert = Certificate::from_pem(pem)?;
        let client = reqwest::ClientBuilder::new()
            .add_root_certificate(root_cert)
            .build()?;

        let agents = RemoteAgents::new().await;
        let bg_agents = agents.clone();

        match local_addr {
            DiscoveryAddr::AnyPrivate => {
                let _handle = spawn(async move { discovery_loop_starter(bg_agents).await });
            }
            DiscoveryAddr::V4(ipv4_addr) => {
                let _handle = spawn(async move { discovery_loop(ipv4_addr, bg_agents).await });
            }
        }

        Ok(Self { client, agents })
    }

    pub async fn new(ca_cert_file: &Path, local_addr: DiscoveryAddr) -> Result<Self, TokenError> {
        let mut ca_certs = File::open(ca_cert_file)?;
        let mut ca_pem: Vec<u8> = Vec::new();
        ca_certs.read_to_end(&mut ca_pem)?;
        Self::new_with_pem(&ca_pem, local_addr).await
    }

    pub async fn agents(&self) -> HashMap<RemoteAgent, Instant> {
        self.agents.agents().await.clone()
    }

    async fn token_from_local_net(&self) -> Result<TokenSignResponse, TokenError> {
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
                        warn!(
                            "error reading response text from {} (`{e}`), trying next agent",
                            agent.url
                        );
                        sleep(Duration::from_millis(200)).await;
                        continue;
                    }
                };
                return Ok(serde_json::from_str(&text)?);
            } else {
                warn!(
                    "bad response code {} from from {}, trying next agent",
                    resp.status(),
                    agent.url
                );
                sleep(Duration::from_millis(200)).await;
                continue;
            }
        }

        Err(TokenError::NoSource)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::{net::Ipv4Addr, sync::Arc, time::Duration};

    use itertools::Itertools;
    use modama_common::{ca::Ca, logging::init_logging};
    use tokio::{
        sync::Mutex,
        time::{sleep, Instant},
    };
    use url::Url;

    use crate::{
        local_net_common::DiscoveryInner,
        local_net_token_provider::{DiscoveryAddr, DiscoveryMessage, LocalNetTokenProvider, RemoteAgent},
        local_net_token_publisher::{SharedState, advertisement_loop},
    };

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

    #[tokio::test]
    async fn test_simple_udp_discovery() {
        init_logging();
        let ca = Ca::new::<String>(None).unwrap();
        let ca_cert = ca.get_ca_cert_pem();

        let receiver = LocalNetTokenProvider::new_with_pem(
            ca_cert.as_bytes(),
            DiscoveryAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
        )
        .await
        .unwrap();
        let loopback = Ipv4Addr::new(127, 0, 0, 1);
        let shared_state = Arc::new(Mutex::new(SharedState {
            api_urls: Default::default(),
        }));
        shared_state
            .lock()
            .await
            .api_urls
            .insert(loopback, "https://localhost:1234".to_owned());
        let sender_task = advertisement_loop(loopback, Arc::clone(&shared_state)).await;

        let deadline = Instant::now() + Duration::from_secs(1);

        loop {
            if !receiver.agents().await.is_empty() {
                break;
            } else {
                sleep(Duration::from_millis(1)).await;
            }
            if Instant::now() > deadline {
                panic!("timeout, no advertisement received")
            }
        }

        let agents = receiver.agents().await;
        let agents = agents.keys().collect_vec();
        let agent = RemoteAgent {
            url: Url::parse("https://localhost:1234").unwrap(),
        };
        assert_eq!(agents, vec![&agent] as Vec<&RemoteAgent>,);

        sender_task.abort();
    }
}
