use std::{ops::ControlFlow, sync::Arc, time::Duration};

use anyhow::{Context, Result as AnyResult};
use futures_util::{Sink, SinkExt, StreamExt};
use log::{error, info, warn};
use modama_common::{
    agent::AgentId,
    messages::{
        AgentMountStatus, EVENT_CHANNEL_PROTOCOL, EventChannelInitialMessages, EventChannelMessage,
        EventChannelRequests, MIN_EVENT_CHANNEL_PROTOCOL,
    },
    tokens::TokenProvider,
    version::modama_git_version,
};
use tokio::sync::Mutex;
use tokio_rustls::rustls::ClientConfig;
use tokio_tungstenite::connect_async_tls_with_config;
use tungstenite::Message;

use crate::{
    agent_state::{AgentState, ConnectionState},
    config::MTLSIdentity,
    local_fan_out::LocalFanOut,
};

/// Handle one message from smbdrop; returns the protocol version if it was a `Welcome`
fn handle_message(fan_out: &LocalFanOut, msg: &str) -> AnyResult<Option<u32>> {
    let event_msg: EventChannelMessage = serde_json::from_str(msg)
        .with_context(|| format!("Invalid message encountered: {}", msg))?;
    info!("Received EventChannelMessage: {event_msg:?}");

    match event_msg {
        EventChannelMessage::UpdateShareList {
            shares,
            foreground_share,
            participants,
        } => {
            fan_out.apply_share_list(shares, foreground_share, participants);
            Ok(None)
        }
        EventChannelMessage::Welcome {
            protocol,
            connection,
        } => {
            info!("smbdrop welcomed us on connection {connection}, protocol {protocol}");
            fan_out.welcomed(connection);
            Ok(Some(protocol))
        }
    }
}

/// Whether `msg` is a `Welcome` this agent understands; one from an smbdrop older than protocol
/// 2 lacks the connection ID, and before protocol 1 there was none at all
fn is_welcome(msg: &str) -> bool {
    matches!(
        serde_json::from_str(msg),
        Ok(EventChannelMessage::Welcome { .. })
    )
}

/// Handle a message from smbdrop; `welcomed` is `true` once its `Welcome` arrived. Before that,
/// anything else means an smbdrop too old for this agent, which would never send a message we
/// understand.
async fn receive_inner(
    message: Option<Result<Message, tokio_tungstenite::tungstenite::Error>>,
    fan_out: &LocalFanOut,
    welcomed: bool,
) -> ControlFlow<(), Option<u32>> {
    match message {
        Some(Ok(Message::Text(utf8_bytes))) if !welcomed && !is_welcome(utf8_bytes.as_str()) => {
            error!(
                "smbdrop's first message is no Welcome of event channel protocol \
                 {MIN_EVENT_CHANNEL_PROTOCOL} or later: it speaks an older one; update smbdrop. \
                 Reconnecting..."
            );
            tokio::time::sleep(Duration::from_secs(5)).await;
            ControlFlow::Break(())
        }
        Some(Ok(Message::Text(utf8_bytes))) => match handle_message(fan_out, utf8_bytes.as_str()) {
            Ok(welcome) => ControlFlow::Continue(welcome),
            Err(e) => {
                error!("Error while handing message: {e}");
                ControlFlow::Continue(None)
            }
        },
        Some(Ok(Message::Close(frame))) => {
            // the stream ends right after, which reconnects
            match frame {
                Some(frame) if !frame.reason.is_empty() => {
                    error!("smbdrop closed the connection: {}", frame.reason);
                }
                _ => info!("smbdrop closed the connection"),
            }
            ControlFlow::Continue(None)
        }
        Some(Ok(_)) => {
            warn!("Message is not text, ignoring...");
            ControlFlow::Continue(None)
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

/// Mount status reporting for one connection to smbdrop
#[derive(Default)]
struct StatusReporter {
    /// The negotiated protocol version; 0 until smbdrop sent `Welcome`
    protocol: u32,

    /// What we reported last on this connection
    reported: Option<AgentMountStatus>,
}

impl StatusReporter {
    /// Report our mount `status` if smbdrop understands it and it changed; Err if sending failed
    async fn report<S>(
        &mut self,
        status: Option<AgentMountStatus>,
        ws_write: &mut S,
    ) -> AnyResult<()>
    where
        S: Sink<Message> + Unpin,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        if self.protocol < 1 {
            return Ok(());
        }
        let Some(status) = status else {
            return Ok(());
        };
        if self.reported == Some(status) {
            return Ok(());
        }
        let msg = serde_json::to_string(&EventChannelRequests::ReportMountStatus { status })?;
        ws_write.send(Message::Text(msg.into())).await?;
        self.reported = Some(status);
        Ok(())
    }
}

pub async fn message_loop(
    state: Arc<Mutex<AgentState>>,
    tls_config: Arc<ClientConfig>,
    token_provider: Arc<dyn TokenProvider + Sync + Send>,
) {
    let (config, fan_out) = {
        let state = state.lock().await;
        (state.config.clone(), Arc::clone(&state.fan_out))
    };

    loop {
        state.lock().await.connection = ConnectionState::Connecting;
        fan_out.smbdrop_disconnected();
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
        let token_identity = AgentId {
            resource: token.resource,
            agent: token.agent_name,
            san_encoded: token.agent_uri,
        };
        // with a client certificate, `Login`; without one, `AnonymousLogin` (ADR 0001)
        let version = modama_git_version().to_owned();
        let login = match &config.identity {
            MTLSIdentity::AgentId { .. } => EventChannelInitialMessages::Login {
                token: token.token,
                version,
                protocol: EVENT_CHANNEL_PROTOCOL,
            },
            MTLSIdentity::Anonymous { hostname } => EventChannelInitialMessages::AnonymousLogin {
                token: token.token,
                version,
                protocol: EVENT_CHANNEL_PROTOCOL,
                name: hostname.clone(),
            },
        };
        let login_msg = match serde_json::to_string(&login) {
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
        fan_out.logged_in(token_identity);
        let mut reporter = StatusReporter::default();
        let mut mount_statuses = fan_out.mount_status_feed();
        loop {
            // Some(status) when our mount status changed; None after a message from smbdrop, after
            // which we read the current status
            let status = tokio::select! {
                // the reporter learns the protocol from the Welcome
                msg = ws_read.next() => {
                    match receive_inner(msg, &fan_out, reporter.protocol != 0).await {
                        ControlFlow::Continue(Some(protocol)) => {
                            if protocol < MIN_EVENT_CHANNEL_PROTOCOL {
                                error!(
                                    "smbdrop speaks event channel protocol {protocol}, this agent \
                                     needs {MIN_EVENT_CHANNEL_PROTOCOL}; update smbdrop. Reconnecting..."
                                );
                                tokio::time::sleep(Duration::from_secs(5)).await;
                                break;
                            }
                            reporter.protocol = protocol.min(EVENT_CHANNEL_PROTOCOL);
                        }
                        ControlFlow::Continue(None) => {}
                        ControlFlow::Break(_) => break, // error or disconnected => reconnect
                    }
                    None
                }
                status = mount_statuses.next() => Some(status),
            };
            // after any message from smbdrop (a Welcome needs the status), or when our mount status
            // changed; the reporter sends only changes
            let status = status.unwrap_or_else(|| fan_out.mount_status());
            if let Err(e) = reporter.report(status, &mut ws_write).await {
                error!("Could not report mount status ({e}), reconnecting...");
                tokio::time::sleep(Duration::from_secs(5)).await;
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::caller_scope::HelperScope;
    use crate::config::{AgentConfig, MTLSIdentity};
    use crate::local_fan_out::{HelperFeed, HelperPresence};
    use modama_common::ca::Ca;
    use modama_common::internal_api_types::Shares;
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
    use tokio::sync::broadcast::Sender;
    use tokio::task::JoinHandle;
    use url::Url;

    struct MessageLoopTestSetup {
        message_loop_task: JoinHandle<()>,
        event_server_task: JoinHandle<()>,
        pub initial_state: SmbDropState,
        pub event_queue: Sender<SmbDropState>,
        pub fan_out: Arc<LocalFanOut>,
    }

    /// The certified agent `test-agent` at `test_resource`
    fn certified() -> MTLSIdentity {
        MTLSIdentity::AgentId {
            agent_name: "test-agent".to_string(),
            resource_name: "test_resource".to_string(),
        }
    }

    /// An `AgentConfig` pointing at an event server on `port`
    fn test_config(port: u16, identity: MTLSIdentity) -> AgentConfig {
        let api_url = Url::parse(&format!("wss://localhost:{port}")).expect("url");
        let root = RootCertStore::empty();
        AgentConfig {
            events_api_url: api_url,
            mgmt_api_url: Url::parse("http://localhost").unwrap(),
            smb_hostname: "example.local".to_string(),
            smb_ip: tokio::sync::OnceCell::new(),
            smb_port: 445,
            smb_domain: "WORKGROUP".to_string(),
            identity,
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
        }
    }

    impl MessageLoopTestSetup {
        /// A certified agent `test-agent` at `test_resource`
        async fn start() -> Self {
            Self::start_as(certified()).await
        }

        /// An agent with `identity`: with a client certificate for `test-agent` if certified,
        /// without one if anonymous; either way with a token for `test-agent`
        async fn start_as(identity: MTLSIdentity) -> Self {
            init_logging();

            let ca = Ca::new::<String>(None, "hunter42").unwrap();
            let server_cert = ca.server_cert(&["localhost".to_string()]).unwrap();
            let mut roots = RootCertStore::empty();
            let ca_cert = ca.get_ca_cert_pem();
            roots
                .add(CertificateDer::from_pem_slice(ca_cert.as_bytes()).unwrap())
                .unwrap();
            // like in production: clients with and without a certificate
            let client_cert_verifier = WebPkiClientVerifier::builder(roots.clone().into())
                .allow_unauthenticated()
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

            let anonymous = matches!(identity, MTLSIdentity::Anonymous { .. });
            let config = test_config(port, identity);

            let state = AgentState::new(&config);
            let fan_out = Arc::clone(&state.lock().await.fan_out);

            let client_cert = ca.client_cert("test-agent", "test_resource").unwrap();
            let client_chain: Vec<CertificateDer> = vec![
                client_cert.cert.clone().into(),
                CertificateDer::from_pem_slice(ca_cert.as_bytes()).unwrap(),
            ];
            let client_pk_der = PrivateKeyDer::Pkcs8(client_cert.key_pair.serialize_der().into());
            let client_builder = ClientConfig::builder().with_root_certificates(roots);
            let client_conf: ClientConfig = if anonymous {
                client_builder.with_no_client_auth()
            } else {
                client_builder
                    .with_client_auth_cert(client_chain, client_pk_der)
                    .unwrap()
            };
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
                fan_out,
            }
        }

        /// Wait until the desired shares in the agent state match the given share IDs
        /// (in any order) and foreground share ID, as a GUI sees them
        async fn wait_for_desired(&self, share_ids: &[&str], foreground_share_id: Option<&str>) {
            let mut gui = self.fan_out.subscribe_gui(HelperScope::Uid(1000));
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    let desired = gui.next().await.desired_shares;
                    if desired.is_some_and(|d| matches(&d, share_ids, foreground_share_id)) {
                        return;
                    }
                }
            })
            .await
            .expect("agent desired state did not reach the expected value");
        }

        /// A session helper connection that never reports. Returns once smbdrop lists our
        /// status as `Reconciling`, which then no longer changes. Until then, every status report
        /// makes smbdrop resend its share list, and an old list resent after a newer one would
        /// replace it (see `modama-rs/TODO.md`, "smbdrop event channel").
        async fn connect_helper(&self) -> (HelperPresence, HelperFeed) {
            let helper = self.fan_out.connect_helper(HelperScope::Uid(1000));
            wait_for_own_status(self, AgentMountStatus::Reconciling).await;
            helper
        }

        async fn abort(&self) {
            self.event_server_task.abort();
            self.message_loop_task.abort();
        }
    }

    /// Whether `shares` has the given share IDs (in any order) and foreground share ID
    fn matches(shares: &Shares, share_ids: &[&str], foreground_share_id: Option<&str>) -> bool {
        let mut expected: Vec<&str> = share_ids.to_vec();
        expected.sort();
        let mut ids: Vec<&str> = shares.shares.iter().map(|s| s.share_id.as_str()).collect();
        ids.sort();
        ids == expected && shares.foreground_share_id.as_deref() == foreground_share_id
    }

    /// Wait until the desired state forwarded to the session helper of `feed` matches the given
    /// share IDs (in any order) and foreground share ID; earlier updates are skipped
    async fn wait_for_forwarded(
        feed: &mut HelperFeed,
        share_ids: &[&str],
        foreground_share_id: Option<&str>,
    ) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let shares = feed.next().await.share_state;
                if shares.is_some_and(|s| matches(&s, share_ids, foreground_share_id)) {
                    return;
                }
            }
        })
        .await
        .expect("forwarded desired state did not reach the expected value");
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
            created_by: None,
        }
    }

    #[tokio::test]
    async fn test_message_loop() {
        let test_setup = MessageLoopTestSetup::start().await;
        let (_helper, mut feed) = test_setup.connect_helper().await;

        // send a state update via the event server's broadcast channel
        let share = test_share("test-id-1", 0);
        let mut new_state = test_setup.initial_state.clone();
        new_state.shares.insert(share.share_id.clone(), share);
        test_setup.event_queue.send(new_state).unwrap();

        test_setup
            .wait_for_desired(&["test-id-1"], Some("test-id-1"))
            .await;
        wait_for_forwarded(&mut feed, &["test-id-1"], Some("test-id-1")).await;

        test_setup.abort().await;
    }

    /// An agent without a client certificate logs in with `AnonymousLogin`; smbdrop would
    /// close a `Login` without one, and no shares would arrive
    #[tokio::test]
    async fn test_anonymous_agent_is_accepted() {
        let test_setup = MessageLoopTestSetup::start_as(MTLSIdentity::Anonymous {
            hostname: "neighbour-pc".to_owned(),
        })
        .await;
        let (_helper, mut feed) = test_setup.connect_helper().await;

        let share = test_share("test-id-1", 0);
        let mut new_state = test_setup.initial_state.clone();
        new_state.shares.insert(share.share_id.clone(), share);
        test_setup.event_queue.send(new_state).unwrap();
        wait_for_forwarded(&mut feed, &["test-id-1"], Some("test-id-1")).await;

        test_setup.abort().await;
    }

    #[tokio::test]
    async fn test_message_loop_remove_share() {
        let test_setup = MessageLoopTestSetup::start().await;
        let (_helper, mut feed) = test_setup.connect_helper().await;

        let share = test_share("test-id-1", 0);
        let mut new_state = test_setup.initial_state.clone();
        new_state.shares.insert(share.share_id.clone(), share);
        test_setup.event_queue.send(new_state).unwrap();

        test_setup
            .wait_for_desired(&["test-id-1"], Some("test-id-1"))
            .await;
        wait_for_forwarded(&mut feed, &["test-id-1"], Some("test-id-1")).await;

        // now remove again (back to initial state):
        test_setup
            .event_queue
            .send(test_setup.initial_state.clone())
            .unwrap();

        test_setup.wait_for_desired(&[], None).await;
        wait_for_forwarded(&mut feed, &[], None).await;

        test_setup.abort().await;
    }

    #[tokio::test]
    async fn test_message_loop_newest_share_is_foreground() {
        let test_setup = MessageLoopTestSetup::start().await;
        let (_helper, mut feed) = test_setup.connect_helper().await;

        let share_1 = test_share("test-id-1", 0);
        let mut new_state_1 = test_setup.initial_state.clone();
        new_state_1.shares.insert(share_1.share_id.clone(), share_1);
        test_setup.event_queue.send(new_state_1.clone()).unwrap();

        test_setup
            .wait_for_desired(&["test-id-1"], Some("test-id-1"))
            .await;
        wait_for_forwarded(&mut feed, &["test-id-1"], Some("test-id-1")).await;

        // one more share, created later => it becomes the foreground share
        let share_2 = test_share("test-id-2", 1);
        let mut new_state_2 = new_state_1.clone();
        new_state_2.shares.insert(share_2.share_id.clone(), share_2);
        test_setup.event_queue.send(new_state_2).unwrap();

        test_setup
            .wait_for_desired(&["test-id-1", "test-id-2"], Some("test-id-2"))
            .await;
        wait_for_forwarded(&mut feed, &["test-id-1", "test-id-2"], Some("test-id-2")).await;

        test_setup.abort().await;
    }

    /// smbdrop resends the shares when agents come and go; helpers only get actual changes
    #[tokio::test]
    async fn test_unchanged_desired_state_is_not_forwarded() {
        let test_setup = MessageLoopTestSetup::start().await;
        let (_helper, mut feed) = test_setup.connect_helper().await;

        let share_1 = test_share("test-id-1", 0);
        let mut state_1 = test_setup.initial_state.clone();
        state_1.shares.insert(share_1.share_id.clone(), share_1);
        test_setup.event_queue.send(state_1.clone()).unwrap();
        wait_for_forwarded(&mut feed, &["test-id-1"], Some("test-id-1")).await;

        // the same again, then a change: the change is the next thing helpers see
        test_setup.event_queue.send(state_1.clone()).unwrap();
        let share_2 = test_share("test-id-2", 1);
        let mut state_2 = state_1.clone();
        state_2.shares.insert(share_2.share_id.clone(), share_2);
        test_setup.event_queue.send(state_2).unwrap();

        let next = tokio::time::timeout(Duration::from_secs(2), feed.next())
            .await
            .unwrap();
        assert_eq!(next.share_state.map(|s| s.shares.len()), Some(2));

        test_setup.abort().await;
    }

    #[test]
    fn only_a_protocol_2_welcome_is_a_welcome() {
        assert!(is_welcome(
            r#"{"type": "Welcome", "protocol": 2, "connection": 7}"#
        ));
        // protocol 1 had no connection ID; protocol 0 started with an update
        assert!(!is_welcome(r#"{"type": "Welcome", "protocol": 1}"#));
        assert!(!is_welcome(
            r#"{"type": "UpdateShareList", "shares": [], "foreground_share": null, "connected_agents": []}"#
        ));
    }

    /// A change of only the participants reaches GUIs; session helpers, which only act on
    /// desired states, see nothing new
    #[test]
    fn test_participant_changes_reach_guis_only() {
        use modama_common::messages::{ConnectionId, Participant, ParticipantIdentity};

        let fan_out = Arc::new(LocalFanOut::new(certified()));
        let own_agent = AgentId::from_uri("x-modama-agent://test_resource#test-agent").unwrap();
        fan_out.logged_in(own_agent.clone());
        handle_message(
            &fan_out,
            r#"{"type": "Welcome", "protocol": 2, "connection": 1}"#,
        )
        .unwrap();
        let scope = HelperScope::Uid(1000);
        let (_helper, mut helper) = fan_out.connect_helper(scope);
        let mut gui = fan_out.subscribe_gui(scope);
        let participant = |id, agent: &str, status| Participant {
            connection: ConnectionId(id),
            identity: ParticipantIdentity::Certified {
                agent: AgentId::from_uri(&format!("x-modama-agent://test_resource#{agent}"))
                    .unwrap(),
            },
            peer: format!("192.0.2.{id}:50000").parse().unwrap(),
            status,
        };
        // we, and another agent with `status`
        let update = |status| {
            serde_json::to_string(&EventChannelMessage::UpdateShareList {
                shares: vec![],
                foreground_share: None,
                participants: vec![
                    participant(1, "test-agent", None),
                    participant(3, "other", status),
                ],
            })
            .unwrap()
        };
        // what they show before smbdrop told us anything
        assert_eq!(helper.try_next().unwrap().share_state, None);
        assert!(gui.try_next().is_some());

        handle_message(&fan_out, &update(None)).unwrap();
        assert!(helper.try_next().unwrap().share_state.is_some());
        let others = gui.try_next().unwrap().connected_agents.unwrap().others;
        assert_eq!(others.len(), 1);
        assert_eq!(others[0].status, None);

        // only the other agent's status changes
        handle_message(&fan_out, &update(Some(AgentMountStatus::Idle))).unwrap();
        assert_eq!(helper.try_next(), None);
        let others = gui.try_next().unwrap().connected_agents.unwrap().others;
        assert_eq!(others[0].status, Some(AgentMountStatus::Idle));

        handle_message(&fan_out, &update(Some(AgentMountStatus::Idle))).unwrap();
        assert!(gui.try_next().is_none(), "nothing changed");
        assert_eq!(helper.try_next(), None);
    }

    /// Our status as smbdrop sends it back to us
    async fn wait_for_own_status(test_setup: &MessageLoopTestSetup, expected: AgentMountStatus) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if test_setup.fan_out.own_status_at_smbdrop() == Some(expected) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("own status did not become {expected:?}"));
    }

    #[tokio::test]
    async fn test_mount_status_is_reported() {
        use modama_common::internal_api_types::SessionHelperState;

        let test_setup = MessageLoopTestSetup::start().await;
        wait_for_own_status(&test_setup, AgentMountStatus::NoHelper).await;

        // a session helper connects, but hasn't reported yet
        let (helper, _feed) = test_setup.fan_out.connect_helper(HelperScope::Uid(1000));
        wait_for_own_status(&test_setup, AgentMountStatus::Reconciling).await;

        // it reports for the current (empty) desired state
        helper.report(SessionHelperState {
            mounted_shares: vec![],
            errors: vec![],
            reconciled_share_ids: Some(vec![]),
        });
        wait_for_own_status(&test_setup, AgentMountStatus::Idle).await;

        // a share is desired: its pass is still to come
        let share = test_share("test-id-1", 0);
        let mut new_state = test_setup.initial_state.clone();
        new_state.shares.insert(share.share_id.clone(), share);
        test_setup.event_queue.send(new_state).unwrap();
        wait_for_own_status(&test_setup, AgentMountStatus::Reconciling).await;

        // the helper goes away
        drop(helper);
        wait_for_own_status(&test_setup, AgentMountStatus::NoHelper).await;

        test_setup.abort().await;
    }

    /// The requests the reporter sent into `sink`
    fn sent_requests(sink: &[Message]) -> Vec<EventChannelRequests> {
        sink.iter()
            .map(|msg| serde_json::from_str(msg.to_text().unwrap()).unwrap())
            .collect()
    }

    /// An smbdrop that never sent `Welcome` doesn't know the request
    #[tokio::test]
    async fn test_no_status_report_without_welcome() {
        let status = Some(AgentMountStatus::NoHelper);
        let mut sink: Vec<Message> = vec![];

        let mut reporter = StatusReporter::default();
        reporter.report(status, &mut sink).await.unwrap();
        assert!(sink.is_empty());

        // after Welcome, the status is sent once
        reporter.protocol = 1;
        reporter.report(status, &mut sink).await.unwrap();
        reporter.report(status, &mut sink).await.unwrap();
        assert_eq!(
            sent_requests(&sink),
            vec![EventChannelRequests::ReportMountStatus {
                status: AgentMountStatus::NoHelper
            }]
        );
    }

    /// Before smbdrop told us the desired state, we don't know our status
    #[tokio::test]
    async fn test_no_status_report_without_desired_state() {
        let fan_out = LocalFanOut::new(certified());
        let mut sink: Vec<Message> = vec![];
        let mut reporter = StatusReporter {
            protocol: 1,
            reported: None,
        };
        reporter
            .report(fan_out.mount_status(), &mut sink)
            .await
            .unwrap();
        assert!(sink.is_empty());
    }
}
