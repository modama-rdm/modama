use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result as AnyResult};

use crate::{
    agent_state::{AgentState, ConnectionState, ShareState},
    mount_backend::MountBackend,
};
use futures_util::{future::join_all, StreamExt};
use log::{debug, error, info, warn};
use modama_common::messages::{EventChannelMessage, Share};
use tokio::sync::Mutex;
use tokio_rustls::rustls::ClientConfig;
use tokio_tungstenite::connect_async_tls_with_config;

async fn update_share_list(
    backend: &impl MountBackend,
    state: &mut AgentState,
    desired_shares: ShareState<Share>,
) -> AnyResult<()> {
    let new_fg_share: Option<Share> = desired_shares.foreground_share;
    let old_fg_share: Option<&Share> = state
        .current_shares
        .foreground_share
        .as_ref()
        .map(|ms| &ms.share);
    if old_fg_share != new_fg_share.as_ref() {
        if let Some(share) = new_fg_share {
            state.current_shares.set_foreground_share(
                match backend.ensure_mounted(&share, &state.config).await {
                    Ok(s) => Some(s),
                    Err(e) => {
                        error!("Mount failed: {e}");
                        None
                    }
                },
            );
        } else {
            state.current_shares.set_foreground_share(None);
        }
    }
    state.current_shares.set_shares(
        join_all(desired_shares.shares.into_iter().map(|s| async {
            let s2 = s;
            backend.ensure_mounted(&s2, &state.config).await
        }))
        .await
        .into_iter()
        .flat_map(|res| {
            if let Err(err) = &res {
                warn!("Mounting failed: {err}");
            }
            res
        })
        .collect(),
    );

    debug!(
        "desired mounts: {:?}",
        &state
            .current_shares
            .shares
            .iter()
            .map(|s| s.mount_path.to_string_lossy().clone())
            .collect::<Vec<_>>()
    );

    backend
        .umount_old_shares(&state.config, &state.current_shares.shares)
        .await;
    backend.cleanup_mount_points(&state.config).await;

    Ok(())
}

async fn handle_message(
    backend: &impl MountBackend,
    state: &Arc<Mutex<AgentState>>,
    msg: &str,
) -> AnyResult<()> {
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
            state.lock().await.desired_shares = desired_shares.clone();
            state.lock().await.connected_agents = connected_agents;

            update_share_list(backend, &mut *state.lock().await, desired_shares).await?;
            Ok(())
        }
    }
}

pub async fn message_loop(
    backend: &impl MountBackend,
    state: Arc<Mutex<AgentState>>,
    tls_config: Arc<ClientConfig>,
) {
    let config = state.lock().await.config.clone();

    loop {
        state.lock().await.connection = ConnectionState::Connecting;
        let (conn, _response) = match connect_async_tls_with_config(
            &config.api_url.to_string(),
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
                let url = &config.api_url;
                error!("Could not connect to {url} ({e}) reconnecting...");
                error!("{}", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };
        state.lock().await.connection = ConnectionState::Connected;

        let (mut _ws_write, mut ws_read) = conn.split();

        loop {
            match ws_read.next().await {
                Some(Ok(msg)) => {
                    if let Ok(msg) = msg.into_text() {
                        if let Err(e) = handle_message(backend, &state, &msg).await {
                            error!("Error while handing message: {e}");
                        }
                    } else {
                        warn!("Message is not text, ignoring...");
                    }
                }
                Some(Err(e)) => {
                    error!("Could not receive message ({e}), reconnecting...");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    break;
                }
                None => {
                    info!("Disconnected, trying to reconnect...");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::{config::AgentConfig, platform_common::EnsureMountedError, share::MountedShare};
    use modama_common::ca::Ca;
    use modama_common::messages;
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    use rustls::{server::WebPkiClientVerifier, ClientConfig, RootCertStore};
    use smbdrop::events::start_event_server;
    use smbdrop::share_manager::SmbDropState;
    use std::{path::PathBuf, sync::Arc};
    use tokio::sync::broadcast::Sender;
    use tokio::sync::{mpsc, Mutex};
    use tokio::task::JoinHandle;
    use url::Url;

    #[derive(Debug)]
    struct MockMountBackend {
        ensure_mounted_calls: mpsc::Sender<messages::Share>,
        umount_old_shares_calls: mpsc::Sender<Vec<MountedShare>>,
        base: PathBuf,
    }

    impl MockMountBackend {
        fn new(
            ensure_mounted_calls: mpsc::Sender<messages::Share>,
            umount_old_shares_calls: mpsc::Sender<Vec<MountedShare>>,
            base: PathBuf,
        ) -> Self {
            Self {
                ensure_mounted_calls,
                base,
                umount_old_shares_calls,
            }
        }
    }

    impl MountBackend for MockMountBackend {
        async fn cleanup_mount_points(&self, _config: &AgentConfig) {}

        async fn ensure_mounted(
            &self,
            share: &messages::Share,
            _config: &AgentConfig,
        ) -> Result<MountedShare, EnsureMountedError> {
            self.ensure_mounted_calls.send(share.clone()).await.unwrap();
            Ok(MountedShare {
                share: share.clone(),
                mount_path: self.base.join("mounted"),
            })
        }

        async fn umount_old_shares(&self, _config: &AgentConfig, shares: &[MountedShare]) {
            self.umount_old_shares_calls
                .send(shares.to_vec())
                .await
                .unwrap()
        }
    }

    struct MessageLoopTestSetup {
        message_loop_task: JoinHandle<()>,
        event_server_task: JoinHandle<()>,
        pub initial_state: SmbDropState,
        pub event_queue: Sender<SmbDropState>,
        pub state: Arc<Mutex<AgentState>>,
        pub rx_mount: mpsc::Receiver<Share>,
        pub rx_umount: mpsc::Receiver<Vec<MountedShare>>,
    }

    impl MessageLoopTestSetup {
        async fn start() -> Self {
            let ca = Ca::new::<String>(None).unwrap();
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
                .with_single_cert(server_chain, server_pk_der)
                .unwrap();

            let initial_state = SmbDropState {
                shares: std::collections::HashMap::new(),
                users: std::collections::HashMap::new(),
            };
            let (event_server_task, event_queue, port) =
                start_event_server("localhost:0", server_config, &initial_state)
                    .await
                    .expect("start_event_server");

            // build AgentConfig pointing at our server
            let api_url = Url::parse(&format!("wss://localhost:{port}")).expect("url");
            let config = AgentConfig {
                api_url,
                mgmt_api_url: "http://localhost".to_string(),
                smb_hostname: "example.local".to_string(),
                smb_ip: tokio::sync::OnceCell::new(),
                smb_port: 445,
                smb_domain: "WORKGROUP".to_string(),
                agent_name: "test-agent".to_string(),
                resource_name: "test_resource".to_string(),
                linux_config: None,
            };

            let state = Arc::new(Mutex::new(AgentState::new(&config)));

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

            // create an mpsc channel to receive mount calls from the mock backend
            let (tx_mount, rx_mount) = mpsc::channel::<messages::Share>(8);
            let (tx_umount, rx_umount) = mpsc::channel::<Vec<MountedShare>>(8);
            let mock = Arc::new(MockMountBackend::new(
                tx_mount,
                tx_umount,
                std::env::temp_dir(),
            ));

            // run message_loop in background
            let message_loop_state = Arc::clone(&state);
            let message_loop_task = tokio::spawn(async move {
                // message_loop runs forever; it will return only if the connection loop ends
                // we run it and let it process the single message from our server
                message_loop(&*mock, message_loop_state, client_config).await;
            });

            Self {
                message_loop_task,
                event_server_task,
                initial_state,
                event_queue,
                state,
                rx_mount,
                rx_umount,
            }
        }

        async fn abort(&self) {
            self.event_server_task.abort();
            self.message_loop_task.abort();
        }
    }

    #[tokio::test]
    async fn test_message_loop() {
        let mut test_setup = MessageLoopTestSetup::start().await;

        // send a state update via the event server's broadcast channel
        let test_share = messages::Share {
            resource: Some("test_resource".to_owned()),
            share_id: "test-id-1".to_owned(),
            share_name: "share1".to_owned(),
            dest_path: "/data/dest".to_owned(),
            created_timestamp: 0,
            username: "u".to_owned(),
            password: "p".to_owned(),
            uid: 1000,
            gid: 1000,
        };
        let mut new_state = test_setup.initial_state.clone();
        new_state
            .shares
            .insert(test_share.share_id.clone(), test_share.clone());
        test_setup.event_queue.send(new_state).unwrap();

        // verify the mock backend was asked to mount the share (with timeout)
        let recv = tokio::time::timeout(Duration::from_secs(2), test_setup.rx_mount.recv())
            .await
            .expect("timeout waiting for mount call");
        let recorded_share = recv.expect("mock sender dropped");
        assert_eq!(recorded_share.share_id, "test-id-1");

        // assert state updated
        let st = test_setup.state.lock().await;
        println!(
            "agent state after update: connection={:?}, shares.len={}",
            st.connection,
            st.current_shares.shares.len()
        );
        assert_eq!(st.current_shares.shares.len(), 1);
        assert!(st.current_shares.foreground_share.is_some());
        let fg = st.current_shares.foreground_share.as_ref().unwrap();
        assert_eq!(fg.share.share_id, "test-id-1");

        test_setup.abort().await;
    }

    #[tokio::test]
    async fn test_message_loop_remove_share() {
        let mut test_setup = MessageLoopTestSetup::start().await;

        // send a state update via the event server's broadcast channel
        let test_share = messages::Share {
            resource: Some("test_resource".to_owned()),
            share_id: "test-id-1".to_owned(),
            share_name: "share1".to_owned(),
            dest_path: "/data/dest".to_owned(),
            created_timestamp: 0,
            username: "u".to_owned(),
            password: "p".to_owned(),
            uid: 1000,
            gid: 1000,
        };
        let mut new_state = test_setup.initial_state.clone();
        new_state
            .shares
            .insert(test_share.share_id.clone(), test_share.clone());
        test_setup.event_queue.send(new_state).unwrap();

        // verify the mock backend was asked to mount the share (with timeout)
        let recv = tokio::time::timeout(Duration::from_secs(2), test_setup.rx_mount.recv())
            .await
            .expect("timeout waiting for mount message");
        let recorded_share = recv.expect("mock sender dropped");
        assert_eq!(recorded_share.share_id, "test-id-1");

        // verify the mock backend was asked to umount (i.e. make sure nothing extra is mounted)
        let recv = tokio::time::timeout(Duration::from_secs(2), test_setup.rx_umount.recv())
            .await
            .expect("timeout waiting for umount");
        let vec_of_still_to_be_mounted_shares = recv.expect("mock sender dropped");
        assert_eq!(vec_of_still_to_be_mounted_shares.len(), 1);

        // assert state updated
        {
            let st = test_setup.state.lock().await;
            println!(
                "agent state after update: connection={:?}, shares.len={}",
                st.connection,
                st.current_shares.shares.len()
            );
            assert_eq!(st.current_shares.shares.len(), 1);
            assert!(st.current_shares.foreground_share.is_some());
            let fg = st.current_shares.foreground_share.as_ref().unwrap();
            assert_eq!(fg.share.share_id, "test-id-1");
        }

        // now remove again (back to initial state):
        test_setup
            .event_queue
            .send(test_setup.initial_state.clone())
            .unwrap();

        // verify the mock backend was asked to umount
        let recv = tokio::time::timeout(Duration::from_secs(1), test_setup.rx_umount.recv())
            .await
            .expect("timeout waiting for umount");
        let vec_of_still_to_be_mounted_shares = recv.expect("mock sender dropped");
        assert_eq!(vec_of_still_to_be_mounted_shares.len(), 0);

        // assert state updated
        {
            let st = test_setup.state.lock().await;
            println!(
                "agent state after update: connection={:?}, shares.len={}",
                st.connection,
                st.current_shares.shares.len()
            );
            assert_eq!(st.current_shares.shares.len(), 0);
            assert!(st.current_shares.foreground_share.is_none());
        }

        test_setup.event_server_task.abort();
        test_setup.message_loop_task.abort();
    }


    #[tokio::test]
    async fn test_message_loop_swith_out_fg_share() {
        let mut test_setup = MessageLoopTestSetup::start().await;

        // send a state update via the event server's broadcast channel
        let test_share = messages::Share {
            resource: Some("test_resource".to_owned()),
            share_id: "test-id-1".to_owned(),
            share_name: "share1".to_owned(),
            dest_path: "/data/dest/1".to_owned(),
            created_timestamp: 0,
            username: "u".to_owned(),
            password: "p".to_owned(),
            uid: 1000,
            gid: 1000,
        };
        let mut new_state_1 = test_setup.initial_state.clone();
        new_state_1
            .shares
            .insert(test_share.share_id.clone(), test_share.clone());
        test_setup.event_queue.send(new_state_1.clone()).unwrap();

        // verify the mock backend was asked to mount the share (with timeout)
        let recv = tokio::time::timeout(Duration::from_secs(2), test_setup.rx_mount.recv())
            .await
            .expect("timeout waiting for mount message");
        let recorded_share = recv.expect("mock sender dropped");
        assert_eq!(recorded_share.share_id, "test-id-1");

        // verify the mock backend was asked to umount (i.e. make sure nothing extra is mounted)
        let recv = tokio::time::timeout(Duration::from_secs(2), test_setup.rx_umount.recv())
            .await
            .expect("timeout waiting for umount");
        let vec_of_still_to_be_mounted_shares = recv.expect("mock sender dropped");
        assert_eq!(vec_of_still_to_be_mounted_shares.len(), 1);

        // assert state updated
        {
            let st = test_setup.state.lock().await;
            println!(
                "agent state after update: connection={:?}, shares.len={}",
                st.connection,
                st.current_shares.shares.len()
            );
            assert_eq!(st.current_shares.shares.len(), 1);
            assert!(st.current_shares.foreground_share.is_some());
            let fg = st.current_shares.foreground_share.as_ref().unwrap();
            assert_eq!(fg.share.share_id, "test-id-1");
        }

        // one more share
        let test_share = messages::Share {
            resource: Some("test_resource".to_owned()),
            share_id: "test-id-2".to_owned(),
            share_name: "share2".to_owned(),
            dest_path: "/data/dest/2".to_owned(),
            created_timestamp: 1, // NOTE the updated timestamp here!
            username: "u".to_owned(),
            password: "p".to_owned(),
            uid: 1000,
            gid: 1000,
        };
        let mut new_state_2 = new_state_1.clone();
        new_state_2
            .shares
            .insert(test_share.share_id.clone(), test_share.clone());
        test_setup.event_queue.send(new_state_2).unwrap();

        // verify the mock backend was asked to mount the share (with timeout)
        let recv = tokio::time::timeout(Duration::from_secs(2), test_setup.rx_mount.recv())
            .await
            .expect("timeout waiting for mount message");
        let recorded_share = recv.expect("mock sender dropped");
        assert_eq!(recorded_share.share_id, "test-id-1");
        let recv = tokio::time::timeout(Duration::from_secs(2), test_setup.rx_mount.recv())
            .await
            .expect("timeout waiting for mount message");
        let recorded_share = recv.expect("mock sender dropped");
        assert_eq!(recorded_share.share_id, "test-id-2");

        // verify the mock backend was asked to umount (i.e. make sure nothing extra is mounted)
        let recv = tokio::time::timeout(Duration::from_secs(2), test_setup.rx_umount.recv())
            .await
            .expect("timeout waiting for umount");
        let vec_of_still_to_be_mounted_shares = recv.expect("mock sender dropped");
        assert_eq!(vec_of_still_to_be_mounted_shares.len(), 2);

        // assert state updated
        {
            let st = test_setup.state.lock().await;
            println!(
                "agent state after update: connection={:?}, shares.len={}",
                st.connection,
                st.current_shares.shares.len()
            );
            assert_eq!(st.current_shares.shares.len(), 2);
            assert!(st.current_shares.foreground_share.is_some());
            let fg = st.current_shares.foreground_share.as_ref().unwrap();
            assert_eq!(fg.share.share_id, "test-id-2");
        }

        test_setup.event_server_task.abort();
        test_setup.message_loop_task.abort();
    }
}

