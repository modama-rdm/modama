use std::{ops::ControlFlow, sync::Arc, time::Duration};

use anyhow::{Context, Result as AnyResult};
use tungstenite::Message;

use crate::agent_state::{
    AgentState, ConnectionState, RemoteInfo, ShareState, StateUpdateToSession,
};
use futures_util::{SinkExt, StreamExt};
use log::{error, info, warn};
use modama_common::{
    messages::{EventChannelInitialMessages, EventChannelMessage, Share},
    tokens::TokenProvider,
    version::modama_git_version,
};
use tokio::sync::Mutex;
use tokio_rustls::rustls::ClientConfig;
use tokio_tungstenite::connect_async_tls_with_config;

async fn handle_message(state: &Arc<Mutex<AgentState>>, msg: &str) -> AnyResult<()> {
    let event_msg: EventChannelMessage = serde_json::from_str(msg)
        .with_context(|| format!("Invalid message encountered: {}", msg))?;
    info!("Received EventChannelMessage: {event_msg:?}");

    match event_msg {
        EventChannelMessage::UpdateShareList {
            shares,
            foreground_share,
            connected_agents,
        } => {
            let desired_shares: ShareState<Share> = ShareState {
                foreground_share,
                shares: shares.into_iter().collect(),
            };
            {
                let mut state = state.lock().await;
                state.remote_info = Some(RemoteInfo {
                    connected_agents,
                    desired_shares: desired_shares.clone(),
                });
                // forward desired state to session helpers
                if let Err(_e) =
                    state
                        .desired_state_channel
                        .send(StateUpdateToSession::DesiredState {
                            foreground_share_id: desired_shares
                                .foreground_share
                                .clone()
                                .map(|share| share.share_id),
                            shares: desired_shares.shares.clone(),
                        })
                {
                    warn!("tried to update session helpers but no one cared");
                }
            }
            Ok(())
        }
    }
}

async fn receive_inner(
    message: Option<Result<Message, tokio_tungstenite::tungstenite::Error>>,
    state: &Arc<Mutex<AgentState>>,
) -> ControlFlow<(), ()> {
    match message {
        Some(Ok(Message::Text(utf8_bytes))) => {
            if let Err(e) = handle_message(state, &utf8_bytes.as_str()).await {
                error!("Error while handing message: {e}");
            }
            ControlFlow::Continue(())
        }
        Some(Ok(_)) => {
            warn!("Message is not text, ignoring...");
            ControlFlow::Continue(())
        }
        Some(Err(e)) => {
            error!("Could not receive message ({e}), reconnecting...");
            tokio::time::sleep(Duration::from_secs(5)).await;
            ControlFlow::Break(())
        }
        None => {
            info!("Disconnected, trying to reconnect...");
            tokio::time::sleep(Duration::from_secs(5)).await;
            ControlFlow::Break(())
        }
    }
}

pub async fn message_loop(
    state: Arc<Mutex<AgentState>>,
    tls_config: Arc<ClientConfig>,
    token_provider: Arc<dyn TokenProvider + Sync + Send>,
) {
    let config = state.lock().await.config.clone();

    loop {
        state.lock().await.connection = ConnectionState::Connecting;
        let (conn, _response) = match connect_async_tls_with_config(
            &config.events_api_url.to_string(),
            None,
            false,
            Some(tokio_tungstenite::Connector::Rustls(Arc::clone(
                &tls_config,
            ))),
        )
        .await
        {
            Ok(s) => s,
            Err(e) => {
                let url = &config.events_api_url;
                error!("Could not connect to {url} ({e}) reconnecting...");
                error!("{}", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        state.lock().await.connection = ConnectionState::Authenticating;
        let (mut ws_write, mut ws_read) = conn.split();

        let token = match Arc::clone(&token_provider).fetch_token().await {
            Ok(t) => t,
            Err(e) => {
                error!("Could not acquire token (`{e}`), reconnecting...");
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };
        let login_msg = match serde_json::to_string(&EventChannelInitialMessages::Login {
            token: token.token,
            version: modama_git_version().to_owned(),
        }) {
            Ok(msg) => msg,
            Err(e) => {
                error!("Could not serialize authentication message, reconnecting...");
                error!("{}", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        if let Err(e) = ws_write
            .send(tungstenite::Message::Text(login_msg.into()))
            .await
        {
            let url = &config.events_api_url;
            error!("Error while authenticating to {url}, reconnecting...");
            error!("{}", e);
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        }

        state.lock().await.connection = ConnectionState::Connected;
        loop {
            let msg = ws_read.next().await;
            match receive_inner(msg, &state).await {
                ControlFlow::Continue(_) => continue, // success or ignored message
                ControlFlow::Break(_) => break,       // error or disconnected => reconnect
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AgentConfig;
    use modama_common::ca::Ca;
    use modama_common::logging::init_logging;
    use modama_common::messages;
    use modama_common::test_helpers::MockTokenProvider;
    use modama_common::tokens::{Claims, LocalTokenSigner};
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    use rustls::{ClientConfig, RootCertStore, server::WebPkiClientVerifier};
    use smbdrop::events::start_event_server;
    use smbdrop::share_manager::SmbDropState;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::Mutex;
    use tokio::sync::broadcast::{self, Sender};
    use tokio::task::JoinHandle;
    use url::Url;

    struct MessageLoopTestSetup {
        message_loop_task: JoinHandle<()>,
        event_server_task: JoinHandle<()>,
        pub initial_state: SmbDropState,
        pub event_queue: Sender<SmbDropState>,
        pub state: Arc<Mutex<AgentState>>,
        pub to_sessions: broadcast::Receiver<StateUpdateToSession>,
    }

    impl MessageLoopTestSetup {
        async fn start() -> Self {
            init_logging();

            let ca = Ca::new::<String>(None, "hunter42").unwrap();
            let server_cert = ca.server_cert(&["localhost".to_string()]).unwrap();
            let mut roots = RootCertStore::empty();
            let ca_cert = ca.get_ca_cert_pem();
            roots
                .add(CertificateDer::from_pem_slice(ca_cert.as_bytes()).unwrap())
                .unwrap();
            let client_cert_verifier = WebPkiClientVerifier::builder(roots.clone().into())
                .build()
                .unwrap();

            // server config
            let server_chain: Vec<CertificateDer> = vec![
                server_cert.cert.clone().into(),
                CertificateDer::from_pem_slice(ca_cert.as_bytes()).unwrap(),
            ];
            let server_pk_der = PrivateKeyDer::Pkcs8(server_cert.key_pair.serialize_der().into());
            let server_config = rustls::server::ServerConfig::builder()
                .with_client_cert_verifier(client_cert_verifier)
                .with_single_cert(server_chain, server_pk_der.clone_key())
                .unwrap();

            let initial_state = SmbDropState {
                shares: std::collections::HashMap::new(),
                users: std::collections::HashMap::new(),
            };
            let (event_server_task, event_queue, port) = start_event_server(
                "localhost:0",
                server_config,
                server_pk_der.secret_der(),
                &initial_state,
            )
            .await
            .expect("start_event_server");

            // build AgentConfig pointing at our server
            let api_url = Url::parse(&format!("wss://localhost:{port}")).expect("url");
            let root = RootCertStore::empty();
            let config = AgentConfig {
                events_api_url: api_url,
                mgmt_api_url: Url::parse("http://localhost").unwrap(),
                smb_hostname: "example.local".to_string(),
                smb_ip: tokio::sync::OnceCell::new(),
                smb_port: 445,
                smb_domain: "WORKGROUP".to_string(),
                identity: crate::config::MTLSIdentity::AgentId {
                    agent_name: "test-agent".to_string(),
                    resource_name: "test_resource".to_string(),
                },
                listen_local_api: "localhost:3001".to_owned(),
                token_service_url: Url::parse("http://localhost").unwrap(),
                ca_root_store: root.clone(),
                ca_root_cert: "".to_owned(),
                client_config: Arc::new(
                    ClientConfig::builder()
                        .with_root_certificates(root)
                        .with_no_client_auth(),
                ),
                client_cert: None,
            };

            let state = AgentState::new(&config).await;

            // subscribe before the message loop starts, so no update is missed
            let to_sessions = state.lock().await.desired_state_channel.subscribe();

            let client_cert = ca.client_cert("test_agent", "test_resource").unwrap();
            let client_chain: Vec<CertificateDer> = vec![
                client_cert.cert.clone().into(),
                CertificateDer::from_pem_slice(ca_cert.as_bytes()).unwrap(),
            ];
            let client_pk_der = PrivateKeyDer::Pkcs8(client_cert.key_pair.serialize_der().into());
            let client_conf: ClientConfig = ClientConfig::builder()
                .with_root_certificates(roots)
                .with_client_auth_cert(client_chain, client_pk_der)
                .unwrap();
            let client_config = Arc::new(client_conf);

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

            // run message_loop in background
            let message_loop_state = Arc::clone(&state);
            let message_loop_task = tokio::spawn(async move {
                // message_loop runs forever; it will return only if the connection loop ends
                // we run it and let it process the single message from our server
                message_loop(
                    message_loop_state,
                    client_config,
                    Arc::clone(&token_provider),
                )
                .await;
            });

            Self {
                message_loop_task,
                event_server_task,
                initial_state,
                event_queue,
                state,
                to_sessions,
            }
        }

        /// Wait until the desired shares in the agent state match the given share IDs
        /// (in any order) and foreground share ID.
        async fn wait_for_desired(&self, share_ids: &[&str], foreground_share_id: Option<&str>) {
            let mut expected: Vec<&str> = share_ids.to_vec();
            expected.sort();
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    let state = self.state.lock().await;
                    if let Some(remote_info) = &state.remote_info {
                        let desired = &remote_info.desired_shares;
                        let mut ids: Vec<&str> =
                            desired.shares.iter().map(|s| s.share_id.as_str()).collect();
                        ids.sort();
                        let fg = desired
                            .foreground_share
                            .as_ref()
                            .map(|s| s.share_id.as_str());
                        if ids == expected && fg == foreground_share_id {
                            return;
                        }
                    }
                    drop(state);
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("agent desired state did not reach the expected value");
        }

        /// Wait until the desired state forwarded to session helpers matches the given
        /// share IDs (in any order) and foreground share ID; earlier updates are skipped.
        async fn wait_for_forwarded(
            &mut self,
            share_ids: &[&str],
            foreground_share_id: Option<&str>,
        ) {
            let mut expected: Vec<&str> = share_ids.to_vec();
            expected.sort();
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    match self.to_sessions.recv().await {
                        Ok(StateUpdateToSession::DesiredState {
                            foreground_share_id: fg,
                            shares,
                        }) => {
                            let mut ids: Vec<&str> =
                                shares.iter().map(|s| s.share_id.as_str()).collect();
                            ids.sort();
                            if ids == expected && fg.as_deref() == foreground_share_id {
                                return;
                            }
                        }
                        Ok(StateUpdateToSession::MountUpdate { .. }) => {}
                        Err(broadcast::error::RecvError::Lagged(_)) => {}
                        Err(broadcast::error::RecvError::Closed) => {
                            panic!("desired state channel closed")
                        }
                    }
                }
            })
            .await
            .expect("forwarded desired state did not reach the expected value");
        }

        async fn abort(&self) {
            self.event_server_task.abort();
            self.message_loop_task.abort();
        }
    }

    fn test_share(id: &str, created_timestamp: u64) -> messages::Share {
        messages::Share {
            resource: Some("test_resource".to_owned()),
            share_id: id.to_owned(),
            share_name: format!("share-{id}"),
            dest_path: format!("/data/dest/{id}"),
            created_timestamp,
            username: "u".to_owned(),
            password: "p".to_owned(),
            uid: 1000,
            gid: 1000,
        }
    }

    #[tokio::test]
    async fn test_message_loop() {
        let mut test_setup = MessageLoopTestSetup::start().await;

        // send a state update via the event server's broadcast channel
        let share = test_share("test-id-1", 0);
        let mut new_state = test_setup.initial_state.clone();
        new_state.shares.insert(share.share_id.clone(), share);
        test_setup.event_queue.send(new_state).unwrap();

        test_setup
            .wait_for_desired(&["test-id-1"], Some("test-id-1"))
            .await;
        test_setup
            .wait_for_forwarded(&["test-id-1"], Some("test-id-1"))
            .await;

        test_setup.abort().await;
    }

    #[tokio::test]
    async fn test_message_loop_remove_share() {
        let mut test_setup = MessageLoopTestSetup::start().await;

        let share = test_share("test-id-1", 0);
        let mut new_state = test_setup.initial_state.clone();
        new_state.shares.insert(share.share_id.clone(), share);
        test_setup.event_queue.send(new_state).unwrap();

        test_setup
            .wait_for_desired(&["test-id-1"], Some("test-id-1"))
            .await;
        test_setup
            .wait_for_forwarded(&["test-id-1"], Some("test-id-1"))
            .await;

        // now remove again (back to initial state):
        test_setup
            .event_queue
            .send(test_setup.initial_state.clone())
            .unwrap();

        test_setup.wait_for_desired(&[], None).await;
        test_setup.wait_for_forwarded(&[], None).await;

        test_setup.abort().await;
    }

    #[tokio::test]
    async fn test_message_loop_newest_share_is_foreground() {
        let mut test_setup = MessageLoopTestSetup::start().await;

        let share_1 = test_share("test-id-1", 0);
        let mut new_state_1 = test_setup.initial_state.clone();
        new_state_1.shares.insert(share_1.share_id.clone(), share_1);
        test_setup.event_queue.send(new_state_1.clone()).unwrap();

        test_setup
            .wait_for_desired(&["test-id-1"], Some("test-id-1"))
            .await;
        test_setup
            .wait_for_forwarded(&["test-id-1"], Some("test-id-1"))
            .await;

        // one more share, created later => it becomes the foreground share
        let share_2 = test_share("test-id-2", 1);
        let mut new_state_2 = new_state_1.clone();
        new_state_2.shares.insert(share_2.share_id.clone(), share_2);
        test_setup.event_queue.send(new_state_2).unwrap();

        test_setup
            .wait_for_desired(&["test-id-1", "test-id-2"], Some("test-id-2"))
            .await;
        test_setup
            .wait_for_forwarded(&["test-id-1", "test-id-2"], Some("test-id-2"))
            .await;

        test_setup.abort().await;
    }
}
