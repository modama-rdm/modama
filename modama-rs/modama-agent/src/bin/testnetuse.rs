#![allow(clippy::unwrap_used, clippy::expect_used)]
use modama_agent::{
    config::AgentConfig, mount_backend::MountBackend, platform::CurrentPlatformMountBackend,
};
use modama_common::{logging::init_logging, messages::Share};
use tokio::sync::OnceCell;
use url::Url;

#[tokio::main]
async fn main() {
    init_logging();
    let backend = CurrentPlatformMountBackend {};

    let share = Share {
        resource: Some("holo".to_owned()),
        share_id: "U-U-I-D".to_owned(),
        share_name: "share-c6cda885-8e95-4919-a2b8-6e9e3909aa64".to_owned(),
        dest_path: "/adhoc/holo-ingest-test/2024-04-11-ingest-test/".to_owned(),
        created_timestamp: 0,
        username: "sharef76b764d16a043ea805918305ce".to_owned(),
        password: "kubeFWCBMvw1AVTADZsM4wEXxECwBxgAAo8_9cAZ_5U".to_owned(),
        uid: 1000,
        gid: 1000,
    };
    let config = AgentConfig {
        api_url: Url::parse("wss://localhost:5052").unwrap(),
        mgmt_api_url: "http://localhost:5001".to_owned(),
        smb_hostname: "moellenstedt.iff.kfa-juelich.de".to_owned(),
        smb_ip: OnceCell::new(),
        smb_port: 445,
        smb_domain: "ER-C-INGEST".to_owned(),
        identity: modama_agent::config::Identity::AgentId {
            agent_name: "dld".to_owned(),
            resource_name: "holo".to_owned(),
        },
        linux_config: None,
    };
    if let Err(e) = backend.ensure_mounted(&share, &config).await {
        panic!("ensure_mounted: {e}");
    }
    backend.umount_old_shares(&config, &[]).await;
}
