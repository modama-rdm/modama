use std::{
    collections::{HashMap, HashSet},
    io,
    net::Ipv4Addr,
    sync::Arc,
    time::Duration,
};

use axum::{Json, Router, extract::State, routing::put};
use hyper::StatusCode;
use log::{debug, error, info, warn};
use modama_common::tokens::{TokenError, TokenProvider, TokenSignResponse};
use tokio::{net::UdpSocket, spawn, task::JoinHandle, time::sleep};
use url::Url;

use crate::local_net_common::{
    ANNOUNCEMENT_INTERVAL, DiscoveryInnerValidated, DiscoveryMessage, NOTIFY_CURRENT_VERSION,
    NOTIFY_MCAST_ADDR, NOTIFY_PORT, create_mcast_socket_for_sending, local_networks,
};

pub fn get_private_addresses() -> Result<Vec<Ipv4Addr>, network_interface::Error> {
    Ok(local_networks()?
        .into_iter()
        .map(|network| network.addr)
        .filter(Ipv4Addr::is_private)
        .collect())
}

#[derive(Clone)]
struct ProxyState {
    token_provider: Arc<dyn TokenProvider + Sync + Send>,
}

async fn proxy_sign_request(
    State(state): State<ProxyState>,
) -> Result<Json<TokenSignResponse>, StatusCode> {
    debug!("proxy_sign_request");
    match state.token_provider.fetch_token().await {
        Ok(resp) => Ok(Json(resp)),
        Err(TokenError::ResponseCode(s)) => match StatusCode::from_u16(s.as_u16()) {
            Ok(s) => Err(s),
            Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
        },
        Err(err) => {
            error!("error fetching token: {err}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Start the token server for agents without a client certificate on `addr`, on a random
/// port. Returns its task and the URL to announce; token requests are forwarded to
/// `token_provider`.
pub fn local_token_proxy(
    addr: Ipv4Addr,
    token_provider: Arc<dyn TokenProvider + Sync + Send>,
) -> Result<(JoinHandle<()>, Url), io::Error> {
    let api_state = ProxyState { token_provider };
    let app = Router::<ProxyState>::new()
        .route("/sign/", put(proxy_sign_request))
        .with_state(api_state);

    let listener = std::net::TcpListener::bind((addr, 0))?;
    let listen_port = listener.local_addr()?.port();
    listener.set_nonblocking(true)?;
    let server = axum_server::from_tcp(listener)?;

    let url_str = format!("http://{addr}:{listen_port}/sign/");
    let url = Url::parse(url_str.as_str())
        .map_err(|parse_err| io::Error::other(format!("url parse failed: {parse_err}")))?;

    let server_handle = spawn(async move {
        info!("token proxy listening at http://{addr}:{listen_port}");
        if let Err(e) = server.serve(app.into_make_service()).await {
            // the task ends here; `TokenProxies::update` restarts the token proxy
            error!("the token proxy at http://{addr}:{listen_port} failed: {e}");
        }
    });

    Ok((server_handle, url))
}

pub async fn try_send_discovery_message(
    socket: &UdpSocket,
    msg: &DiscoveryMessage,
) -> Result<(), io::Error> {
    let msg = msg.serialize();
    let buf = msg.as_bytes();
    let size = socket
        .send_to(buf, (NOTIFY_MCAST_ADDR, NOTIFY_PORT))
        .await?;
    debug!("sent discovery message `{msg}` ({size})");
    Ok(())
}

/// Announce the token proxy at `api_url` on the interface with address `local`, via multicast
/// UDP packets, until aborted
pub fn advertisement_loop(local: Ipv4Addr, api_url: Url) -> JoinHandle<()> {
    spawn(async move {
        let msg = DiscoveryMessage {
            inner: DiscoveryInnerValidated {
                version: NOTIFY_CURRENT_VERSION,
                api: api_url,
            },
        };
        loop {
            let socket = match create_mcast_socket_for_sending(local) {
                Ok(socket) => socket,
                Err(err) => {
                    error!("error creating multicast socket on {local} (`{err}`), retrying...");
                    sleep(Duration::from_secs(5)).await;
                    continue;
                }
            };
            loop {
                match try_send_discovery_message(&socket, &msg).await {
                    Ok(()) => sleep(ANNOUNCEMENT_INTERVAL).await,
                    Err(err) => {
                        error!(
                            "error sending multicast message on {local} (`{err}`), re-creating \
                             socket..."
                        );
                        sleep(Duration::from_secs(1)).await;
                        break;
                    }
                }
            }
        }
    })
}

/// The token proxy on one address: its server, and the task that announces it. Dropping it
/// stops both.
struct TokenProxy {
    server: JoinHandle<()>,
    advertisement: JoinHandle<()>,
}

impl TokenProxy {
    fn start(
        addr: Ipv4Addr,
        token_provider: Arc<dyn TokenProvider + Sync + Send>,
    ) -> Result<Self, io::Error> {
        let (server, url) = local_token_proxy(addr, token_provider)?;
        Ok(Self {
            server,
            advertisement: advertisement_loop(addr, url),
        })
    }

    fn is_running(&self) -> bool {
        !self.server.is_finished() && !self.advertisement.is_finished()
    }
}

impl Drop for TokenProxy {
    fn drop(&mut self) {
        self.server.abort();
        self.advertisement.abort();
    }
}

/// The token proxies of this agent, one per address
pub struct TokenProxies {
    token_provider: Arc<dyn TokenProvider + Sync + Send>,
    running: HashMap<Ipv4Addr, TokenProxy>,
    /// Addresses a token proxy couldn't be started on, so each is only logged once; retried on
    /// every update
    failed: HashSet<Ipv4Addr>,
}

impl TokenProxies {
    pub fn new(token_provider: Arc<dyn TokenProvider + Sync + Send>) -> Self {
        Self {
            token_provider,
            running: HashMap::new(),
            failed: HashSet::new(),
        }
    }

    /// The addresses with a running token proxy
    pub fn addresses(&self) -> HashSet<Ipv4Addr> {
        self.running.keys().copied().collect()
    }

    /// Run a token proxy on each of `addrs` and on no other address: start the missing ones,
    /// stop those of addresses that are gone, and restart those that stopped by themselves.
    /// Must be called from within a Tokio runtime.
    pub fn update(&mut self, addrs: &HashSet<Ipv4Addr>) {
        self.running.retain(|addr, proxy| {
            if !addrs.contains(addr) {
                info!(
                    "stopping the token proxy on {addr}: this computer no longer has the address"
                );
                false
            } else if !proxy.is_running() {
                warn!("the token proxy on {addr} stopped, restarting it");
                false
            } else {
                true
            }
        });
        self.failed.retain(|addr| addrs.contains(addr));
        for addr in addrs {
            if self.running.contains_key(addr) {
                continue;
            }
            match TokenProxy::start(*addr, Arc::clone(&self.token_provider)) {
                Ok(proxy) => {
                    self.running.insert(*addr, proxy);
                    self.failed.remove(addr);
                }
                Err(e) => {
                    if self.failed.insert(*addr) {
                        warn!("could not start the token proxy on {addr}: {e}");
                    }
                }
            }
        }
    }
}

/// Keep a token proxy running on every private address of this computer, following the
/// addresses as they come and go
pub async fn token_publisher_loop(token_fetcher: Arc<dyn TokenProvider + Sync + Send>) {
    let mut proxies = TokenProxies::new(token_fetcher);
    loop {
        match get_private_addresses() {
            Ok(addrs) => proxies.update(&addrs.into_iter().collect()),
            Err(e) => {
                // keep the token proxies we have
                error!("error while discovering local interfaces: {e}");
                sleep(Duration::from_secs(10)).await;
                continue;
            }
        };
        sleep(Duration::from_secs(1)).await;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::{collections::HashSet, net::Ipv4Addr, sync::Arc, time::Duration};

    use modama_common::{
        test_helpers::MockTokenProvider,
        tokens::{Claims, LocalTokenSigner, TokenProvider},
    };
    use tokio::time::sleep;

    use super::TokenProxies;
    use crate::local_net_common::DISCOVERY_MUTEX;

    #[tokio::test]
    async fn token_proxies_follow_the_addresses() {
        // announces on loopback, which the discovery tests listen on
        let _guard = DISCOVERY_MUTEX.lock().await;
        let token_provider: Arc<dyn TokenProvider + Sync + Send> = Arc::new(MockTokenProvider {
            signer: LocalTokenSigner::new(b"secret"),
            claims: Claims {
                sub: "x-modama-agent://test_resource#test-agent".to_owned(),
                exp: 0,
            },
        });
        let loopback = Ipv4Addr::LOCALHOST;
        let mut proxies = TokenProxies::new(token_provider);

        proxies.update(&HashSet::from([loopback]));
        assert_eq!(proxies.addresses(), HashSet::from([loopback]));
        let server = proxies.running[&loopback].server.abort_handle();
        let advertisement = proxies.running[&loopback].advertisement.abort_handle();

        // unchanged addresses keep their token proxy
        proxies.update(&HashSet::from([loopback]));
        assert!(!server.is_finished() && !advertisement.is_finished());

        // the address is gone: both tasks end
        proxies.update(&HashSet::new());
        assert!(proxies.addresses().is_empty());
        for _ in 0..1000 {
            if server.is_finished() && advertisement.is_finished() {
                return;
            }
            sleep(Duration::from_millis(1)).await;
        }
        panic!("the tasks of the removed address are still running");
    }
}
