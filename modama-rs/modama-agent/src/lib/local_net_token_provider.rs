use std::{
    collections::HashMap,
    error::Error,
    io::{self, BufRead},
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use futures_util::{StreamExt, stream::FuturesUnordered};
use log::{debug, warn};
use modama_common::{
    tokens::{BoxFutureToken, TokenError, TokenProvider, TokenSignResponse},
    version::modama_git_version,
};
use reqwest::{Certificate, Client, StatusCode};
use tokio::{
    net::UdpSocket,
    sync::Mutex,
    task::JoinHandle,
    time::{Instant, sleep},
};
use url::Url;

use crate::local_net_common::{
    ANNOUNCEMENT_INTERVAL, DiscoveryMessage, LocalNetwork, local_networks,
};

/// How long connecting to a token proxy may take; they are on a network of this computer, so
/// one that takes longer is most likely unreachable
const TOKEN_PROXY_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// How long a whole token request may take; the token proxy asks the server for the token
const TOKEN_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// How long after its last announcement a token proxy is forgotten: a few announcements may get
/// lost, but one that stopped announcing is gone, or has restarted on another port. One that
/// answered the last request is kept regardless (see `TokenProxyStatus::expired`).
const TOKEN_PROXY_EXPIRY: Duration = ANNOUNCEMENT_INTERVAL.saturating_mul(4);
/// How long a token proxy gets to answer before the next one is asked as well
const TOKEN_PROXY_HEAD_START: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct RemoteAgent {
    url: Url,
}

/// How the last token request to a token proxy went; in the order they are asked
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum LastRequest {
    Succeeded,
    /// Not asked yet, or not answered before another token proxy did
    Unknown,
    Failed,
}

#[derive(Debug, Clone, Copy)]
struct TokenProxyStatus {
    /// When it was last announced
    seen: Instant,
    last_request: LastRequest,
}

impl TokenProxyStatus {
    /// Whether to forget the token proxy. One that answered the last request is kept even
    /// without announcements: on a network that stops delivering them (a switch with IGMP
    /// snooping and no querier, Wi-Fi), it would otherwise be lost while it still works. It
    /// expires once a request to it fails, or another token proxy answers before it.
    fn expired(&self, now: Instant) -> bool {
        self.last_request != LastRequest::Succeeded
            && now.saturating_duration_since(self.seen) > TOKEN_PROXY_EXPIRY
    }
}

fn forget_expired(agents: &mut HashMap<RemoteAgent, TokenProxyStatus>, now: Instant) {
    agents.retain(|agent, status| {
        let expired = status.expired(now);
        if expired {
            debug!(
                "forgetting the token proxy at {}: it is no longer announced",
                agent.url
            );
        }
        !expired
    });
}

#[derive(Clone, Debug)]
pub struct RemoteAgents {
    agents: Arc<Mutex<HashMap<RemoteAgent, TokenProxyStatus>>>,
}

impl RemoteAgents {
    pub async fn new() -> Self {
        Self {
            agents: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Mark an agent as seen, and forget those not heard from for a while
    pub async fn seen(&self, agent: &RemoteAgent) {
        let now = Instant::now();
        let mut agents = self.agents.lock().await;
        forget_expired(&mut agents, now);
        agents
            .entry(agent.clone())
            .and_modify(|status| status.seen = now)
            .or_insert(TokenProxyStatus {
                seen: now,
                last_request: LastRequest::Unknown,
            });
    }

    /// Remember that the agent answered a token request, so it is asked first
    pub async fn success(&self, agent: &RemoteAgent) {
        self.set_last_request(agent, LastRequest::Succeeded).await;
    }

    /// Remember that a token request to the agent failed, so it is asked last. It may announce
    /// itself and still be out of reach, for example behind a host firewall.
    pub async fn report_failure(&self, agent: &RemoteAgent) {
        self.set_last_request(agent, LastRequest::Failed).await;
    }

    /// Remember that another token proxy answered while this one, asked earlier, had not: if
    /// it answered the request before, it no longer counts as answering
    async fn outpaced(&self, agent: &RemoteAgent) {
        if let Some(status) = self.agents.lock().await.get_mut(agent)
            && status.last_request == LastRequest::Succeeded
        {
            status.last_request = LastRequest::Unknown;
        }
    }

    async fn set_last_request(&self, agent: &RemoteAgent, last_request: LastRequest) {
        if let Some(status) = self.agents.lock().await.get_mut(agent) {
            status.last_request = last_request;
        }
    }

    /// The agents heard from recently, with the time of their last announcement
    pub async fn agents(&self) -> HashMap<RemoteAgent, Instant> {
        self.statuses()
            .await
            .into_iter()
            .map(|(agent, status)| (agent, status.seen))
            .collect()
    }

    async fn statuses(&self) -> HashMap<RemoteAgent, TokenProxyStatus> {
        let now = Instant::now();
        let mut agents = self.agents.lock().await;
        forget_expired(&mut agents, now);
        agents.clone()
    }
}

/// Discover token services on the local network and fetch tokens from them.
#[derive(Debug)]
pub struct LocalNetTokenProvider {
    http_client: Client,
    agents: RemoteAgents,
    /// Where discovery listens; token requests only go to proxies on those networks
    local: DiscoveryAddr,
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

/// The networks of the interfaces this agent listens on for announcements, as they are now; it
/// only uses token proxies on those
fn listened_networks(local: DiscoveryAddr) -> Result<Vec<LocalNetwork>, network_interface::Error> {
    Ok(local_networks()?
        .into_iter()
        .filter(|network| match local {
            DiscoveryAddr::AnyPrivate => network.addr.is_private(),
            DiscoveryAddr::V4(addr) => network.addr == addr,
        })
        .collect())
}

/// Whether the token proxy at `url` is on one of `networks`. An announcement reaches every
/// network of the link it is sent on: an enrolled agent with addresses of two networks on one
/// interface announces both there, and one of them is out of our reach. Agents announce IPv4
/// addresses; a host name can't be checked and is out of reach as well.
///
/// Known limit: any network of this computer counts, not only the one the announcement arrived
/// on. If two of its networks use the same address range (a lab network and a VPN, say), a
/// token proxy announced on one may be asked over the other. If that request is refused, the
/// token proxy is asked last from then on. If it gets no answer at all, it delays a token
/// request by `TOKEN_PROXY_HEAD_START` whenever it is asked before a working one, which
/// happens until a token proxy has answered: that one is asked first afterwards.
fn on_networks(url: &Url, networks: &[LocalNetwork]) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(ip)) => networks.iter().any(|network| network.contains(ip)),
        _ => false,
    }
}

/// The token proxies to ask for a token, in this order: those that answered the last request,
/// then those not asked yet, then those that failed; the most recently announced first within
/// each. With `networks`, only those on one of them: the networks are checked at each request,
/// so a proxy counts once this computer joins its network, and no longer once it leaves.
fn token_proxies_to_ask(
    agents: HashMap<RemoteAgent, TokenProxyStatus>,
    networks: Option<&[LocalNetwork]>,
) -> Vec<RemoteAgent> {
    let mut agents: Vec<(RemoteAgent, TokenProxyStatus)> = agents.into_iter().collect();
    agents.sort_by_key(|(_, status)| (status.last_request, std::cmp::Reverse(status.seen)));
    agents
        .into_iter()
        .map(|(agent, _)| agent)
        .filter(|agent| match networks {
            Some(networks) if !on_networks(&agent.url, networks) => {
                debug!(
                    "skipping the token proxy at {}: it isn't on a network this agent listens on \
                     for other agents",
                    agent.url
                );
                false
            }
            _ => true,
        })
        .collect()
}

async fn handle_discovery_msg(msg: &DiscoveryMessage, agents: &mut RemoteAgents) {
    debug!("got a discovery msg: {msg:?}");
    agents
        .seen(&RemoteAgent {
            url: msg.api_url().to_owned(),
        })
        .await;
}

/// Listen for discovery announcements, feeding `agents`, until aborted
fn spawn_discovery(local: DiscoveryAddr, agents: RemoteAgents) -> JoinHandle<()> {
    platform::spawn_discovery(local, agents)
}

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(windows)]
use windows as platform;

/// One socket, bound to the discovery group, with a membership on every private interface: it
/// hears the group there and nowhere else.
///
/// - Bound to the group address, the socket only receives datagrams sent to that group; unicast
///   to the port never reaches it.
/// - Memberships are per interface (identified by its address), so group traffic arriving on
///   other interfaces isn't ours.
/// - `IP_MULTICAST_ALL` off: only our own memberships count, not those of other sockets or
///   processes on this host.
#[cfg(target_os = "linux")]
mod linux {
    use std::{
        collections::HashSet,
        io,
        net::{Ipv4Addr, SocketAddrV4},
        time::Duration,
    };

    use log::{debug, error, info, warn};
    use socket2::{Domain, Protocol, Socket, Type};
    use tokio::{net::UdpSocket, spawn, task::JoinHandle, time::sleep};

    use super::{DiscoveryAddr, RemoteAgents, handle_discovery_msg, try_recv_discovery_message};
    use crate::{
        local_net_common::{NOTIFY_MCAST_ADDR, NOTIFY_PORT},
        local_net_token_proxy::get_private_addresses,
    };

    /// How often new or removed interfaces are picked up
    const REFRESH_INTERVAL: Duration = Duration::from_secs(1);

    pub(super) fn spawn_discovery(local: DiscoveryAddr, agents: RemoteAgents) -> JoinHandle<()> {
        // listen right away, so that announcements sent from now on are heard
        let listener = Listener::open(local)
            .inspect_err(|err| error!("error creating multicast socket (`{err}`), retrying..."))
            .ok();
        spawn(discovery_loop(local, agents, listener))
    }

    struct Listener {
        socket: UdpSocket,
        memberships: Memberships,
    }

    impl Listener {
        fn open(local: DiscoveryAddr) -> io::Result<Self> {
            let socket = create_listener()?;
            let mut memberships = Memberships::default();
            if let Some(wanted) = wanted_addresses(local) {
                memberships.update(&socket, &wanted);
            }
            Ok(Self {
                socket,
                memberships,
            })
        }
    }

    fn create_listener() -> io::Result<UdpSocket> {
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        socket.set_nonblocking(true)?;
        socket.set_multicast_all_v4(false)?;
        socket.bind(&SocketAddrV4::new(NOTIFY_MCAST_ADDR, NOTIFY_PORT).into())?;
        UdpSocket::from_std(socket.into())
    }

    /// The interfaces (by address) to listen on
    fn wanted_addresses(local: DiscoveryAddr) -> Option<HashSet<Ipv4Addr>> {
        match local {
            DiscoveryAddr::V4(addr) => Some(HashSet::from([addr])),
            DiscoveryAddr::AnyPrivate => match get_private_addresses() {
                Ok(addrs) => Some(addrs.into_iter().collect()),
                Err(e) => {
                    // keep the memberships we have
                    warn!("could not list the local interfaces: {e}");
                    None
                }
            },
        }
    }

    /// Memberships to add and to drop, to get from `current` to `wanted` (sorted, for the logs)
    pub(super) fn membership_changes(
        current: &HashSet<Ipv4Addr>,
        wanted: &HashSet<Ipv4Addr>,
    ) -> (Vec<Ipv4Addr>, Vec<Ipv4Addr>) {
        let mut join: Vec<_> = wanted.difference(current).copied().collect();
        let mut leave: Vec<_> = current.difference(wanted).copied().collect();
        join.sort_unstable();
        leave.sort_unstable();
        (join, leave)
    }

    #[derive(Default)]
    struct Memberships {
        joined: HashSet<Ipv4Addr>,
        /// Joins that failed, so each is only logged once; retried on every refresh
        failed: HashSet<Ipv4Addr>,
    }

    impl Memberships {
        fn update(&mut self, socket: &UdpSocket, wanted: &HashSet<Ipv4Addr>) {
            let (join, leave) = membership_changes(&self.joined, wanted);
            for addr in leave {
                // fails if the interface is already gone, which ends the membership anyway
                if let Err(e) = socket.leave_multicast_v4(NOTIFY_MCAST_ADDR, addr) {
                    debug!("could not leave the discovery group on {addr}: {e}");
                }
                self.joined.remove(&addr);
                info!("stopped listening for agents on {addr}");
            }
            for addr in join {
                match socket.join_multicast_v4(NOTIFY_MCAST_ADDR, addr) {
                    Ok(()) => {
                        info!("listening for agents on {addr}");
                        self.joined.insert(addr);
                        self.failed.remove(&addr);
                    }
                    Err(e) => {
                        if self.failed.insert(addr) {
                            warn!("could not listen for agents on {addr}: {e}");
                        }
                    }
                }
            }
            self.failed.retain(|addr| wanted.contains(addr));
        }
    }

    async fn discovery_loop(
        local: DiscoveryAddr,
        mut agents: RemoteAgents,
        mut listener: Option<Listener>,
    ) {
        loop {
            let mut current = match listener.take() {
                Some(listener) => listener,
                None => match Listener::open(local) {
                    Ok(listener) => listener,
                    Err(err) => {
                        error!("error creating multicast socket (`{err}`), retrying...");
                        sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                },
            };
            // the memberships were just set up; refresh them from now on
            let mut refresh = tokio::time::interval_at(
                tokio::time::Instant::now() + REFRESH_INTERVAL,
                REFRESH_INTERVAL,
            );
            loop {
                tokio::select! {
                    _ = refresh.tick() => {
                        if let Some(wanted) = wanted_addresses(local) {
                            current.memberships.update(&current.socket, &wanted);
                        }
                    }
                    // `recv_from` is cancel-safe
                    received = try_recv_discovery_message(&current.socket) => match received {
                        Ok(Some((msg, _addr))) => handle_discovery_msg(&msg, &mut agents).await,
                        Ok(None) => {}
                        Err(err) => {
                            error!("error receiving multicast message (`{err}`), re-creating socket...");
                            sleep(Duration::from_secs(1)).await;
                            break;
                        }
                    }
                }
            }
        }
    }
}

/// One socket per private interface, each bound to that interface's address and the notify
/// port; on Windows, different local addresses don't collide on the same port.
#[cfg(windows)]
mod windows {
    use std::{collections::HashMap, net::Ipv4Addr, time::Duration};

    use log::{error, info};
    use tokio::{net::UdpSocket, spawn, task::JoinHandle, time::sleep};

    use super::{DiscoveryAddr, RemoteAgents, handle_discovery_msg, try_recv_discovery_message};
    use crate::{
        local_net_common::{NOTIFY_MCAST_ADDR, NOTIFY_PORT},
        local_net_token_proxy::get_private_addresses,
    };

    pub(super) fn spawn_discovery(local: DiscoveryAddr, agents: RemoteAgents) -> JoinHandle<()> {
        match local {
            DiscoveryAddr::AnyPrivate => spawn(discovery_loop_starter(agents)),
            DiscoveryAddr::V4(addr) => spawn(discovery_loop(addr, agents)),
        }
    }

    async fn create_mcast_socket(
        port: u16,
        group: Ipv4Addr,
        local: Ipv4Addr,
    ) -> Result<UdpSocket, std::io::Error> {
        let socket = UdpSocket::bind((local, port)).await?;
        socket.join_multicast_v4(group, local)?;
        Ok(socket)
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
                        error!(
                            "error receiving multicast message (`{err}`), re-creating socket..."
                        );
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
                    // stop the tasks of addresses that are gone: they would retry forever
                    discovery_tasks.retain(|addr, task| {
                        let keep = addrs.contains(addr);
                        if !keep {
                            info!("stopped listening for agents on {addr}");
                            task.abort();
                        }
                        keep
                    });
                    for addr in addrs.iter() {
                        let addr = *addr;
                        if discovery_tasks
                            .get(&addr)
                            .is_none_or(JoinHandle::is_finished)
                        {
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
}

#[derive(Copy, Clone, Debug)]
pub enum DiscoveryAddr {
    AnyPrivate,
    V4(Ipv4Addr),
}

impl LocalNetTokenProvider {
    pub async fn new_with_pem(pem: &[u8], local_addr: DiscoveryAddr) -> Result<Self, TokenError> {
        let root_cert = Certificate::from_pem(pem)?;
        let http_client = modama_common::pinned_ca_client_builder(root_cert)
            .user_agent(format!("modama-agent/{}", modama_git_version()))
            .connect_timeout(TOKEN_PROXY_CONNECT_TIMEOUT)
            .timeout(TOKEN_REQUEST_TIMEOUT)
            .build()?;

        let agents = RemoteAgents::new().await;
        let bg_agents = agents.clone();

        let handle = spawn_discovery(local_addr, bg_agents);

        Ok(Self {
            http_client,
            agents,
            local: local_addr,
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

    /// Stop discovery; returns once the task has stopped, so its socket is released
    pub async fn abort(&self) {
        self.discovery_loop_handle.abort();
        while !self.discovery_loop_handle.is_finished() {
            tokio::task::yield_now().await;
        }
    }

    pub async fn agents(&self) -> HashMap<RemoteAgent, Instant> {
        self.agents.agents().await.clone()
    }

    /// Ask one token proxy for a token; the error says what went wrong, for the log
    async fn ask<'a>(
        &self,
        agent: &'a RemoteAgent,
    ) -> (&'a RemoteAgent, Result<TokenSignResponse, String>) {
        let result = async {
            let resp = self
                .http_client
                .put(agent.url.clone())
                .send()
                .await
                .map_err(|e| format!("{e} (source: {:?})", e.source()))?;
            if resp.status() != StatusCode::OK {
                return Err(format!("response code {}", resp.status()));
            }
            let text = resp
                .text()
                .await
                .map_err(|e| format!("error reading the response: {e}"))?;
            serde_json::from_str(&text).map_err(|e| format!("unexpected response: {e}"))
        }
        .await;
        (agent, result)
    }

    /// Get a token from one of the token proxies. They are asked in the order of
    /// `token_proxies_to_ask`; the next one is asked as soon as one fails, or when none has
    /// answered within `TOKEN_PROXY_HEAD_START`, and the first token wins. An unreachable
    /// token proxy so delays the request by the head start at most, not by its timeout.
    async fn token_from_local_net(&self) -> Result<TokenSignResponse, TokenError> {
        let networks = listened_networks(self.local)
            .inspect_err(|e| {
                warn!("could not list this computer's networks, so asking every token proxy: {e}")
            })
            .ok();
        let agents = token_proxies_to_ask(self.agents.statuses().await, networks.as_deref());

        let mut waiting = agents.iter();
        // the token proxies asked so far, in order; `asked` holds their running requests
        let mut asked_agents: Vec<&RemoteAgent> = Vec::new();
        let mut asked = FuturesUnordered::new();
        // Ends: each round returns, takes a token proxy from `waiting`, or finishes a request
        // in `asked`. `waiting` is finite and only shrinks, and every request ends within
        // `TOKEN_REQUEST_TIMEOUT`; once both are empty, the loop returns `NoSource`.
        loop {
            if asked.is_empty() {
                match waiting.next() {
                    Some(agent) => {
                        asked_agents.push(agent);
                        asked.push(self.ask(agent));
                    }
                    None => return Err(TokenError::NoSource),
                }
            }
            tokio::select! {
                Some((agent, result)) = asked.next() => match result {
                    Ok(token) => {
                        self.agents.success(agent).await;
                        // those asked before it that are still silent (failed ones are
                        // marked already, and `outpaced` leaves them as they are)
                        for other in asked_agents.iter().filter(|other| **other != agent) {
                            self.agents.outpaced(other).await;
                        }
                        return Ok(token);
                    }
                    Err(e) => {
                        warn!("could not get a token from the token proxy at {}: {e}", agent.url);
                        self.agents.report_failure(agent).await;
                        if let Some(agent) = waiting.next() {
                            asked_agents.push(agent);
                            asked.push(self.ask(agent));
                        }
                    }
                },
                _ = sleep(TOKEN_PROXY_HEAD_START), if waiting.len() > 0 => {
                    if let Some(agent) = waiting.next() {
                        debug!("no token yet, also asking the token proxy at {}", agent.url);
                        asked_agents.push(agent);
                        asked.push(self.ask(agent));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::{
        collections::HashMap,
        io::BufReader,
        net::Ipv4Addr,
        path::PathBuf,
        sync::Arc,
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
        task::JoinHandle,
        time::{Instant, sleep},
    };
    use url::Url;

    use crate::{
        local_net_common::{DISCOVERY_MUTEX, DiscoveryInnerValidated, LocalNetwork},
        local_net_token_provider::{
            DiscoveryAddr, DiscoveryMessage, LastRequest, LocalNetTokenProvider, RemoteAgent,
            RemoteAgents, TOKEN_PROXY_EXPIRY, TOKEN_REQUEST_TIMEOUT, TokenProxyStatus, on_networks,
            token_proxies_to_ask,
        },
        local_net_token_proxy::{advertisement_loop, local_token_proxy},
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

    #[cfg(target_os = "linux")]
    #[test]
    fn membership_changes_join_new_and_leave_gone_interfaces() {
        use std::collections::HashSet;

        let addr = |last| Ipv4Addr::new(192, 168, 1, last);
        let current = HashSet::from([addr(1), addr(2)]);
        let wanted = HashSet::from([addr(2), addr(4), addr(3)]);
        assert_eq!(
            super::linux::membership_changes(&current, &wanted),
            (vec![addr(3), addr(4)], vec![addr(1)])
        );
        assert_eq!(
            super::linux::membership_changes(&wanted, &wanted),
            (vec![], vec![])
        );
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
        let sender_task =
            advertisement_loop(loopback, Url::parse("http://localhost:1234").unwrap());

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

    /// An enrolled agent with addresses of two networks on one interface announces both on that
    /// link; the token proxy on the network we aren't on is out of reach and isn't asked
    #[test]
    fn only_token_proxies_on_our_networks_are_asked_latest_first() {
        let networks = [LocalNetwork {
            addr: Ipv4Addr::new(192, 168, 0, 7),
            netmask: Some(Ipv4Addr::new(255, 255, 255, 0)),
        }];
        let agent = |url: &str| RemoteAgent {
            url: Url::parse(url).unwrap(),
        };
        let now = Instant::now();
        let seen = |after: u64| TokenProxyStatus {
            seen: now + Duration::from_secs(after),
            last_request: LastRequest::Unknown,
        };
        let agents = HashMap::from([
            (agent("http://192.168.0.20:40729/sign/"), seen(0)),
            (agent("http://10.0.0.1:41767/sign/"), seen(1)),
            (agent("http://192.168.0.30:40000/sign/"), seen(2)),
        ]);
        assert_eq!(
            token_proxies_to_ask(agents.clone(), Some(&networks)),
            vec![
                agent("http://192.168.0.30:40000/sign/"),
                agent("http://192.168.0.20:40729/sign/"),
            ]
        );
        // without the networks (listing them failed), every one is asked
        assert_eq!(
            token_proxies_to_ask(agents, None),
            vec![
                agent("http://192.168.0.30:40000/sign/"),
                agent("http://10.0.0.1:41767/sign/"),
                agent("http://192.168.0.20:40729/sign/"),
            ]
        );
    }

    #[test]
    fn token_proxies_that_answered_are_asked_first_and_failed_ones_last() {
        let agent = |url: &str| RemoteAgent {
            url: Url::parse(url).unwrap(),
        };
        let now = Instant::now();
        let status = |after: u64, last_request| TokenProxyStatus {
            seen: now + Duration::from_secs(after),
            last_request,
        };
        let agents = HashMap::from([
            (
                agent("http://10.0.0.1:1/sign/"),
                status(0, LastRequest::Succeeded),
            ),
            (
                agent("http://10.0.0.2:1/sign/"),
                status(1, LastRequest::Unknown),
            ),
            (
                agent("http://10.0.0.3:1/sign/"),
                status(2, LastRequest::Unknown),
            ),
            (
                agent("http://10.0.0.4:1/sign/"),
                status(3, LastRequest::Failed),
            ),
        ]);
        assert_eq!(
            token_proxies_to_ask(agents, None),
            vec![
                agent("http://10.0.0.1:1/sign/"),
                agent("http://10.0.0.3:1/sign/"),
                agent("http://10.0.0.2:1/sign/"),
                agent("http://10.0.0.4:1/sign/"),
            ]
        );
    }

    #[test]
    fn a_token_proxy_expires_a_few_announcements_after_its_last() {
        let seen = Instant::now();
        let later = seen + TOKEN_PROXY_EXPIRY + Duration::from_millis(1);
        for last_request in [LastRequest::Unknown, LastRequest::Failed] {
            let status = TokenProxyStatus { seen, last_request };
            assert!(!status.expired(seen));
            assert!(!status.expired(seen + TOKEN_PROXY_EXPIRY));
            assert!(status.expired(later));
        }
        // one that answered the last request is kept without announcements
        let answered = TokenProxyStatus {
            seen,
            last_request: LastRequest::Succeeded,
        };
        assert!(!answered.expired(later + Duration::from_secs(3600)));
    }

    #[tokio::test]
    async fn an_outpaced_token_proxy_no_longer_counts_as_answering() {
        let agent = |url: &str| RemoteAgent {
            url: Url::parse(url).unwrap(),
        };
        let answered = agent("http://10.0.0.1:1/sign/");
        let failed = agent("http://10.0.0.2:1/sign/");
        let agents = RemoteAgents::new().await;
        for agent in [&answered, &failed] {
            agents.seen(agent).await;
        }
        agents.success(&answered).await;
        agents.report_failure(&failed).await;

        agents.outpaced(&answered).await;
        agents.outpaced(&failed).await;
        let statuses = agents.statuses().await;
        assert_eq!(statuses[&answered].last_request, LastRequest::Unknown);
        assert_eq!(statuses[&failed].last_request, LastRequest::Failed);
    }

    #[tokio::test]
    async fn expired_token_proxies_are_forgotten_and_announced_ones_keep_their_status() {
        let agent = |url: &str| RemoteAgent {
            url: Url::parse(url).unwrap(),
        };
        let stopped = agent("http://10.0.0.1:1/sign/");
        let announcing = agent("http://10.0.0.2:1/sign/");
        let agents = RemoteAgents::new().await;
        agents.seen(&stopped).await;
        agents.seen(&announcing).await;
        agents.report_failure(&announcing).await;
        assert_eq!(agents.agents().await.len(), 2);

        // as if `stopped` was last announced too long ago
        let long_ago = Instant::now()
            .checked_sub(TOKEN_PROXY_EXPIRY + Duration::from_secs(1))
            .unwrap();
        agents.agents.lock().await.get_mut(&stopped).unwrap().seen = long_ago;

        agents.seen(&announcing).await;
        let statuses = agents.statuses().await;
        assert_eq!(statuses.keys().collect_vec(), vec![&announcing]);
        assert_eq!(statuses[&announcing].last_request, LastRequest::Failed);
    }

    #[test]
    fn only_ipv4_token_proxies_on_our_networks_count() {
        let networks = [LocalNetwork {
            addr: Ipv4Addr::new(192, 168, 0, 7),
            netmask: Some(Ipv4Addr::new(255, 255, 255, 0)),
        }];
        let url = |s: &str| Url::parse(s).unwrap();
        assert!(on_networks(
            &url("http://192.168.0.20:40729/sign/"),
            &networks
        ));
        assert!(!on_networks(&url("http://10.0.0.1:41767/sign/"), &networks));
        assert!(!on_networks(
            &url("http://agent.example:40729/sign/"),
            &networks
        ));
        assert!(!on_networks(&url("http://192.168.0.20:40729/sign/"), &[]));
    }

    struct TestTokenProxy {
        ca: Ca,
        _ca_cert: PathBuf,
        _tmpdir: TempDir,
        token_proxy: JoinHandle<()>,
        url: Url,
    }

    /// Run a test token proxy on loopback, which we use to test local network
    /// service discovery. Tokens are signed with its own private key.
    async fn run_test_token_proxy() -> TestTokenProxy {
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

        let (token_proxy, url) = local_token_proxy(addr, token_provider).unwrap();

        TestTokenProxy {
            ca,
            _ca_cert: ca_cert,
            _tmpdir: tmpdir,
            token_proxy,
            url,
        }
    }

    #[tokio::test]
    /// integration test with a mock token proxy, testing the whole advertisement procedure
    async fn test_remote_token() {
        let _handle = DISCOVERY_MUTEX.lock().await;
        init_logging();

        let loopback_addr = Ipv4Addr::new(127, 0, 0, 1);
        let proxy = run_test_token_proxy().await;
        let ca_cert = proxy.ca.get_ca_cert_pem();

        let receiver = LocalNetTokenProvider::new_with_pem(
            ca_cert.as_bytes(),
            DiscoveryAddr::V4(loopback_addr),
        )
        .await
        .unwrap();
        // this sends out a token on multicast ASAP (loopback in this case), and then again after 5 seconds
        let sender_task = advertisement_loop(loopback_addr, proxy.url.clone());

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
            url: proxy.url.clone(),
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
        let proxy = run_test_token_proxy().await;
        let ca_cert = proxy.ca.get_ca_cert_pem();

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
        // releases the discovery socket before the next test takes the mutex
        receiver.abort().await;
    }

    /// A token proxy that accepts connections and never answers
    fn silent_token_proxy() -> (std::net::TcpListener, RemoteAgent) {
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let agent = RemoteAgent {
            url: Url::parse(&format!("http://127.0.0.1:{port}/sign/")).unwrap(),
        };
        (listener, agent)
    }

    #[tokio::test]
    async fn a_silent_or_failing_token_proxy_does_not_hold_up_the_token() {
        let _handle = DISCOVERY_MUTEX.lock().await;
        init_logging();

        let proxy = run_test_token_proxy().await;
        let ca_cert = proxy.ca.get_ca_cert_pem();
        let receiver = LocalNetTokenProvider::new_with_pem(
            ca_cert.as_bytes(),
            DiscoveryAddr::V4(Ipv4Addr::LOCALHOST),
        )
        .await
        .unwrap();

        // asked in the reverse order of their announcements: the closed port (refused), the
        // silent one, then the working one
        let working = RemoteAgent {
            url: proxy.url.clone(),
        };
        let (_listener, silent) = silent_token_proxy();
        let refusing = {
            let (listener, agent) = silent_token_proxy();
            drop(listener);
            agent
        };
        for agent in [&working, &silent, &refusing] {
            receiver.agents.seen(agent).await;
            sleep(Duration::from_millis(2)).await;
        }

        let started = Instant::now();
        let token = receiver.token_from_local_net().await.unwrap();
        assert_eq!(token.resource, "test_resource");
        assert!(started.elapsed() < TOKEN_REQUEST_TIMEOUT / 4);

        let statuses = receiver.agents.statuses().await;
        assert_eq!(statuses[&working].last_request, LastRequest::Succeeded);
        assert_eq!(statuses[&silent].last_request, LastRequest::Unknown);
        assert_eq!(statuses[&refusing].last_request, LastRequest::Failed);

        // from now on, the working one is asked first
        let started = Instant::now();
        receiver.token_from_local_net().await.unwrap();
        assert!(started.elapsed() < super::TOKEN_PROXY_HEAD_START);

        proxy.token_proxy.abort();
        receiver.abort().await;
    }

    // TODO: more tests
    // - [ ] invalid data tests (making sure the loops don't crash or at least recover)
    // - [ ] check coverage for additional missing parts
}
