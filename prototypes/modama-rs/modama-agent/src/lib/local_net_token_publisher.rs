use std::{
    collections::HashMap,
    io,
    net::{IpAddr, Ipv4Addr},
    sync::Arc,
    time::Duration,
};

use axum::{extract::State, routing::put, Json, Router};
use hyper::StatusCode;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::ServerConfig;
use log::{debug, error, info, warn};
use modama_common::tokens::{TokenError, TokenFetcher, TokenSignResponse};
use network_interface::{NetworkInterface, NetworkInterfaceConfig};
use tokio::{
    net::{TcpListener, UdpSocket},
    spawn,
    sync::Mutex,
    task::JoinHandle,
    time::sleep,
};

use crate::local_net_common::{
    create_mcast_socket_for_sending, DiscoveryInner, DiscoveryMessage, NOTIFY_CURRENT_VERSION,
    NOTIFY_MCAST_ADDR, NOTIFY_PORT,
};

pub fn get_private_addresses() -> Result<Vec<Ipv4Addr>, network_interface::Error> {
    let mut result: Vec<Ipv4Addr> = Vec::new();
    let interfaces = NetworkInterface::show()?;
    for interface in interfaces.iter() {
        let addrs = interface
            .addr
            .iter()
            .filter_map(|addr| {
                if let IpAddr::V4(addr) = addr.ip() {
                    Some(addr)
                } else {
                    None
                }
            })
            .filter(|a| a.is_private());
        for addr in addrs {
            result.push(addr);
        }
    }
    Ok(result)
}

#[derive(Clone)]
struct ProxyState {
    token_fetcher: Arc<TokenFetcher>,
}

pub async fn proxy_sign_request(
    State(state): State<ProxyState>,
) -> Result<Json<TokenSignResponse>, StatusCode> {
    match state.token_fetcher.fetch_token().await {
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

/// Local token server for other agents that do not have a client certificate
pub async fn local_token_server(
    addr: Ipv4Addr,
    token_fetcher: Arc<TokenFetcher>,
    state: Arc<Mutex<SharedState>>,
    tls_config: Arc<ServerConfig>,
) -> JoinHandle<()> {
    let api_state = ProxyState { token_fetcher };
    let app: Router<ProxyState> = Router::<ProxyState>::new()
        .route("/sign/", put(proxy_sign_request))
        .with_state(api_state);

    let listener = TcpListener::bind((addr, 0)).await.unwrap();
    let acceptor = TlsAcceptor::from(Arc::clone(&tls_config));

    let listen_port = listener.local_addr().unwrap().port();
    let server_handle = tokio::spawn(async move {
        axum_server::bind_rustls
    });

    state
        .lock()
        .await
        .api_urls
        .insert(addr, format!("https://{addr}:{listen_port}"));

    server_handle
}

/// State shared to inform the advertisement task about the URLs for the running
/// services on each interface
pub struct SharedState {
    pub api_urls: HashMap<Ipv4Addr, String>,
}

pub async fn try_send_discovery_message(
    socket: &UdpSocket,
    _our_addr: Ipv4Addr,
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

/// Advertise token API on a single interface (`local`)
pub async fn advertisement_loop(local: Ipv4Addr, state: Arc<Mutex<SharedState>>) -> JoinHandle<()> {
    spawn(async move {
        'outer: loop {
            let socket = match create_mcast_socket_for_sending(NOTIFY_MCAST_ADDR, local).await {
                Ok(socket) => socket,
                Err(err) => {
                    error!("error creating multicast socket (`{err}`), retrying...");
                    sleep(Duration::from_millis(5000)).await;
                    continue 'outer;
                }
            };
            'inner: loop {
                let this_api_url = match state.lock().await.api_urls.get(&local) {
                    Some(url) => url.clone(),
                    None => {
                        warn!("could not get API URL for interface {local}, retrying...");
                        sleep(Duration::from_millis(1000)).await;
                        continue 'outer;
                    }
                };
                let msg = &DiscoveryMessage {
                    inner: DiscoveryInner {
                        version: NOTIFY_CURRENT_VERSION,
                        api: this_api_url,
                    },
                };
                match try_send_discovery_message(&socket, local, msg).await {
                    Ok(_) => {
                        info!("discovery message sent, sleeping...");
                        sleep(Duration::from_secs(5)).await;
                        continue 'inner;
                    }
                    Err(err) => {
                        error!("error sending multicast message (`{err}`), re-creating socket...");
                        sleep(Duration::from_millis(1000)).await;
                        continue 'outer;
                    }
                }
            }
        }
    })
}

pub async fn task_exists_and_running(
    map: &HashMap<Ipv4Addr, JoinHandle<()>>,
    key: Ipv4Addr,
) -> bool {
    match map.get(&key) {
        Some(handle) => !handle.is_finished(),
        None => false,
    }
}

pub async fn start_publisher_loop(token_fetcher: Arc<TokenFetcher>, tls_config: Arc<ServerConfig>) {
    let mut advertisement_tasks: HashMap<Ipv4Addr, JoinHandle<()>> = HashMap::new();
    let mut token_server_tasks: HashMap<Ipv4Addr, JoinHandle<()>> = HashMap::new();

    let shared_state = Arc::new(Mutex::new(SharedState {
        api_urls: Default::default(),
    }));

    loop {
        // 1) discover local interfaces, ignore non-private addresses
        match get_private_addresses() {
            Ok(addrs) => {
                // when new interfaces appear, start token servers and
                // advertisement tasks on them; when interfaces disappear, the
                // tasks should just kill itself
                for addr in addrs.iter() {
                    if !task_exists_and_running(&token_server_tasks, *addr).await {
                        let handle = local_token_server(
                            *addr,
                            Arc::clone(&token_fetcher),
                            Arc::clone(&shared_state),
                            Arc::clone(&tls_config),
                        )
                        .await;
                        token_server_tasks.insert(*addr, handle);
                    }
                    if !task_exists_and_running(&advertisement_tasks, *addr).await {
                        let handle = advertisement_loop(*addr, Arc::clone(&shared_state)).await;
                        advertisement_tasks.insert(*addr, handle);
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
