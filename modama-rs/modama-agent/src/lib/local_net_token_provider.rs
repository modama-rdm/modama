use std::{
    collections::HashMap,
    error::Error,
    io::{self, BufRead},
    net::{Ipv4Addr, SocketAddr},
    ops::DerefMut,
    sync::Arc,
    time::Duration,
};

use log::{debug, error, info, warn};
use modama_common::{
    tokens::{BoxFutureToken, TokenError, TokenProvider, TokenSignResponse},
    version::modama_git_version,
};
use reqwest::{Certificate, Client, StatusCode};
use tokio::{
    net::UdpSocket,
    spawn,
    sync::Mutex,
    task::JoinHandle,
    time::{Instant, sleep},
};
use url::Url;

use crate::{
    local_net_common::{DiscoveryMessage, NOTIFY_MCAST_ADDR, NOTIFY_PORT, create_mcast_socket},
    local_net_token_proxy::{get_private_addresses, task_exists_and_running},
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

    pub async fn success(&self, _agent: &RemoteAgent) {
        // TODO: store the success signal and use it for sorting agents
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

/// Discover token services on the local network and fetch tokens from them.
#[derive(Debug)]
pub struct LocalNetTokenProvider {
    http_client: Client,
    agents: RemoteAgents,
    discovery_loop_handle: JoinHandle<()>,
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
    debug!("got a discovery msg: {msg:?}");
    agents
        .seen(&RemoteAgent {
            url: msg.api_url().to_owned(),
        })
        .await;
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
                    let addr = *addr;
                    if !task_exists_and_running(&discovery_tasks, addr).await {
                        info!("starting discovery task on {addr}");
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
        modama_common::install_rustls_crypto_provider();
        let root_cert = Certificate::from_pem(pem)?;
        let http_client = reqwest::ClientBuilder::new()
            .add_root_certificate(root_cert)
            .user_agent(format!("modama-agent/{}", modama_git_version()))
            .build()?;

        let agents = RemoteAgents::new().await;
        let bg_agents = agents.clone();

        let handle = match local_addr {
            DiscoveryAddr::AnyPrivate => {
                spawn(async move { discovery_loop_starter(bg_agents).await })
            }
            DiscoveryAddr::V4(ipv4_addr) => {
                spawn(async move { discovery_loop(ipv4_addr, bg_agents).await })
            }
        };

        Ok(Self {
            http_client,
            agents,
            discovery_loop_handle: handle,
        })
    }

    pub async fn new(
        ca_cert_reader: &mut impl BufRead,
        local_addr: DiscoveryAddr,
    ) -> Result<Self, TokenError> {
        let mut ca_pem: Vec<u8> = Vec::new();
        ca_cert_reader.read_to_end(&mut ca_pem)?;
        Self::new_with_pem(&ca_pem, local_addr).await
    }

    pub async fn abort(&self) {
        self.discovery_loop_handle.abort();
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
            let resp = match self.http_client.put(agent.url.clone()).send().await {
                Ok(r) => r,
                Err(e) => {
                    warn!(
                        "error requesting token from agent at {}: {e} (source: {:?})",
                        agent.url,
                        e.source()
                    );
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
                self.agents.success(agent).await;
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
    use std::{
        io::BufReader,
        net::Ipv4Addr,
        path::PathBuf,
        sync::{Arc, LazyLock},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    use itertools::Itertools;
    use modama_common::{
        ca::Ca,
        logging::init_logging,
        test_helpers::{MockTokenProvider, TempDir},
        tokens::{Claims, LocalTokenSigner, TokenError, TokenProvider},
    };
    use rustls::pki_types::PrivateKeyDer;
    use tokio::{
        sync::Mutex,
        task::JoinHandle,
        time::{Instant, sleep},
    };
    use url::Url;

    use crate::{
        local_net_common::DiscoveryInnerValidated,
        local_net_token_provider::{
            DiscoveryAddr, DiscoveryMessage, LocalNetTokenProvider, RemoteAgent,
        },
        local_net_token_proxy::{SharedState, advertisement_loop, local_token_proxy},
    };

    static DISCOVERY_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

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
        assert!(
            DiscoveryMessage::try_parse(
                "stuff\n{\"version\": 1, \"api\": \"http://localhost:1234/api/\"}"
            )
            .is_none()
        )
    }

    #[test]
    fn test_disco_parse_invalid_6() {
        assert!(
            DiscoveryMessage::try_parse("stuff\n{\"version\": 1, \"api\": \"not a Url\"}")
                .is_none()
        )
    }

    #[test]
    fn test_disco_parse_valid() {
        assert_eq!(
            DiscoveryMessage::try_parse(
                "MODAMA-AGENT-NOTIFY\n{\"version\": 1, \"api\": \"http://localhost:1234/api/\"}"
            ),
            Some(DiscoveryMessage {
                inner: DiscoveryInnerValidated {
                    version: 1,
                    api: Url::parse("http://localhost:1234/api/").unwrap()
                }
            })
        )
    }

    #[test]
    fn test_disco_parse_forward_compat() {
        assert_eq!(
            DiscoveryMessage::try_parse(
                "MODAMA-AGENT-NOTIFY\n{\"version\": 2, \"api\": \"http://localhost:1234/api/\", \"extra\": \"field\"}"
            ),
            Some(DiscoveryMessage {
                inner: DiscoveryInnerValidated {
                    version: 2,
                    api: Url::parse("http://localhost:1234/api/").unwrap()
                }
            })
        )
    }

    #[tokio::test]
    async fn test_local_net_token_provider_new() {
        // create a temporary dir for certs (use local TempDir helper)
        let tmpdir = TempDir::new("test-token-proxy");

        // create CA and serialize
        let ca = Ca::new(Some(tmpdir.to_owned()), "hunter42").expect("create ca");
        ca.serialize_ca_to_files().expect("serialize ca");

        // create server cert
        let server_names = vec!["127.0.0.1".to_string()];
        let server_kc = ca.server_cert(&server_names).expect("server cert");
        ca.save_server_cert(&server_names, &server_kc)
            .expect("save server cert");

        // paths to server files
        let ca_cert = tmpdir.join("ca").join("pub").join("ca.cert.pem");

        let mut reader = BufReader::new(std::fs::File::open(&ca_cert).unwrap());

        let tp =
            LocalNetTokenProvider::new(&mut reader, DiscoveryAddr::V4(Ipv4Addr::new(127, 0, 0, 1)))
                .await
                .unwrap();

        tp.abort().await;
    }

    #[tokio::test]
    async fn test_simple_udp_discovery() {
        let _handle = DISCOVERY_MUTEX.lock().await;
        init_logging();
        let ca = Ca::new::<String>(None, "hunter42").unwrap();
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
            .insert(loopback, Url::parse("http://localhost:1234").unwrap());
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
            url: Url::parse("http://localhost:1234").unwrap(),
        };
        assert_eq!(agents, vec![&agent] as Vec<&RemoteAgent>,);

        sender_task.abort();
        receiver.abort().await;
    }

    struct TestTokenProxy {
        ca: Ca,
        _ca_cert: PathBuf,
        _tmpdir: TempDir,
        token_proxy: JoinHandle<()>,
    }

    /// Run a test token proxy on loopback, which we use to test local network
    /// service discovery. Tokens are signed with its own private key.
    async fn run_test_token_proxy(state: Arc<Mutex<SharedState>>) -> TestTokenProxy {
        // create a temporary dir for certs (use local TempDir helper)
        let tmpdir = TempDir::new("test-token-proxy");

        // create CA and serialize
        let ca = Ca::new(Some(tmpdir.to_owned()), "hunter42").expect("create ca");
        ca.serialize_ca_to_files().expect("serialize ca");

        // create server cert
        let server_names = vec!["127.0.0.1".to_string()];
        let server_kc = ca.server_cert(&server_names).expect("server cert");
        ca.save_server_cert(&server_names, &server_kc)
            .expect("save server cert");

        // paths to server files
        let ca_cert = tmpdir.join("ca").join("ca.cert.pem");

        let addr = Ipv4Addr::new(127, 0, 0, 1);

        let server_pk_der = PrivateKeyDer::Pkcs8(server_kc.key_pair.serialize_der().into());

        let token_signer = LocalTokenSigner::new(server_pk_der.secret_der());
        let token_provider: Arc<dyn TokenProvider + 'static + Sync + Send> =
            Arc::new(MockTokenProvider {
                signer: token_signer,
                claims: Claims {
                    sub: "x-modama-agent://test_resource#test-agent".to_owned(),
                    exp: (SystemTime::now() + Duration::from_secs(1000))
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                },
            });

        let token_proxy = local_token_proxy(addr, token_provider, state)
            .await
            .unwrap();

        TestTokenProxy {
            ca,
            _ca_cert: ca_cert,
            _tmpdir: tmpdir,
            token_proxy,
        }
    }

    #[tokio::test]
    /// integration test with a mock token proxy, testing the whole advertisement procedure
    async fn test_remote_token() {
        let _handle = DISCOVERY_MUTEX.lock().await;
        init_logging();

        let loopback_addr = Ipv4Addr::new(127, 0, 0, 1);
        let shared_state = Arc::new(Mutex::new(SharedState {
            api_urls: Default::default(),
        }));

        let proxy = run_test_token_proxy(Arc::clone(&shared_state)).await;

        let ca_cert = proxy.ca.get_ca_cert_pem();

        // the proxy should have inserted itself into the list of URLs by now:
        assert!(
            shared_state
                .lock()
                .await
                .api_urls
                .contains_key(&loopback_addr)
        );

        let receiver = LocalNetTokenProvider::new_with_pem(
            ca_cert.as_bytes(),
            DiscoveryAddr::V4(loopback_addr),
        )
        .await
        .unwrap();
        // this sends out a token on multicast ASAP (loopback in this case), and then again after 5 seconds
        let sender_task = advertisement_loop(loopback_addr, Arc::clone(&shared_state)).await;

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
        let expected_agent = RemoteAgent {
            url: shared_state
                .lock()
                .await
                .api_urls
                .get(&loopback_addr)
                .unwrap()
                .clone(),
        };
        assert_eq!(agents, vec![&expected_agent] as Vec<&RemoteAgent>,);

        let token = receiver.token_from_local_net().await.unwrap();

        assert_eq!(token.resource, "test_resource");
        assert_eq!(token.agent_name, "test-agent");
        assert_eq!(token.agent_uri, "x-modama-agent://test_resource#test-agent");

        proxy.token_proxy.abort();
        sender_task.abort();
        receiver.abort().await;
    }

    #[tokio::test]
    async fn test_no_source_trivial_case() {
        let _handle = DISCOVERY_MUTEX.lock().await;
        init_logging();

        let loopback_addr = Ipv4Addr::new(127, 0, 0, 1);
        let shared_state = Arc::new(Mutex::new(SharedState {
            api_urls: Default::default(),
        }));

        let proxy = run_test_token_proxy(Arc::clone(&shared_state)).await;

        let ca_cert = proxy.ca.get_ca_cert_pem();

        // the proxy should have inserted itself into the list of URLs by now:
        assert!(
            shared_state
                .lock()
                .await
                .api_urls
                .contains_key(&loopback_addr)
        );

        let receiver = LocalNetTokenProvider::new_with_pem(
            ca_cert.as_bytes(),
            DiscoveryAddr::V4(loopback_addr),
        )
        .await
        .unwrap();

        assert!(matches!(
            receiver.token_from_local_net().await,
            Err(TokenError::NoSource)
        ));
    }

    // TODO: more tests
    // - [ ] invalid data tests (making sure the loops don't crash or at least recover)
    // - [ ] check coverage for additional missing parts
}
