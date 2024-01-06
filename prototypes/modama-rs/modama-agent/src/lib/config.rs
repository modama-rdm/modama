use std::path::PathBuf;

use thiserror::Error;
use tokio::sync::OnceCell;
use url::Url;

#[derive(Debug, Clone)]
pub struct AgentLinuxConfig {
    pub mount_point_prefix: PathBuf,

    // we assume that this system is using some shared unix user,
    // which should have access to the ingest share(s):
    pub uid: u32,
    pub gid: u32,
}

impl Default for AgentLinuxConfig {
    fn default() -> Self {
        Self {
            mount_point_prefix: PathBuf::from("/run/modama/ingest/"),
            uid: 1000,
            gid: 1000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// The base URL for the websocket API (ws:// or wss://)
    pub api_url: Url,

    /// The hostname of the ingest Samba server
    pub smb_hostname: String,

    /// The resolved IP address of the Samba server (lazily looked up on first mount)
    pub smb_ip: OnceCell<String>,

    /// The Samba domain to use for mounting
    pub smb_domain: String,

    /// A user-readable short description of the data producer, like "DLD"
    pub agent_name: String,

    /// A short name of the resouce this agent belongs to, for example "Holo"
    pub resource_name: String,

    // FIXME: find a better way?
    pub linux_config: Option<AgentLinuxConfig>,
    // TODO: add mTLS certs/keys etc. for resource identification and auth
}

#[derive(Error, Debug)]
pub enum DNSLookupFailure {
    #[error("no address found for `{hostname}`")]
    NoAddressFound { hostname: String },

    #[error("DNS lookup failure")]
    IOError(#[from] std::io::Error),
}

impl AgentConfig {
    #[cfg(target_os = "linux")]
    pub async fn get_smb_ip(&self) -> Result<&str, DNSLookupFailure> {
        self.smb_ip
            .get_or_try_init(|| async {
                let mut addrs = async_dns::lookup(&self.smb_hostname).await?;
                let first = addrs.next().ok_or(DNSLookupFailure::NoAddressFound {
                    hostname: self.smb_hostname.to_owned(),
                })?;
                Ok(first.ip_address.to_string())
            })
            .await
            .map(String::as_str)
    }

    pub fn get_linux_config(&self) -> AgentLinuxConfig {
        let c = &self.linux_config;
        c.clone().unwrap_or_default()
    }
}
