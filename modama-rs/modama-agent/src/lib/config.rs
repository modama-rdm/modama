use std::path::{Path, PathBuf};

use clap::{Args, Parser};
use thiserror::Error;
use tokio::sync::OnceCell;
use url::Url;

#[derive(Args, Debug)]
pub struct ClientCertArgs {
    #[arg(long, default_value = "chain.pem")]
    chain: PathBuf,

    #[arg(long, default_value = "priv.pem")]
    key: PathBuf,
}

#[derive(Parser, Debug)]
pub struct CliArgs {
    #[arg(long, default_value = "wss://localhost:5052")]
    event_url: String,

    #[arg(long, default_value = "https://localhost:5053")]
    token_service: String,

    #[arg(long, default_value = "http://localhost:5001")]
    mgmt_api_url: String,

    #[arg(long, default_value = "localhost:3003")]
    listen_local_api: String,

    #[arg(long, default_value = "moellenstedt.iff.kfa-juelich.de")]
    smb_hostname: String,

    #[arg(long, default_value = "445")]
    smb_port: u32,

    #[arg(long, default_value = "ER-C-INGEST")]
    smb_domain: String,

    #[arg(long, default_value = "ca.cert.pem")]
    ca_cert: PathBuf,

    #[command(flatten)]
    client_cert: Option<ClientCertArgs>,
}

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
pub enum Identity {
    Anonymous {
        hostname: String,
    },
    AgentId {
        /// A user-readable short description of the data producer, like "DLD"
        agent_name: String,

        /// A short name of the resouce this agent belongs to, for example "Holo"
        resource_name: String,
    },
}

#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// The base URL for the websocket API (ws:// or wss://)
    pub api_url: Url,

    /// The base URL of the management API
    pub mgmt_api_url: String, // FIXME: make this a `Url`?

    /// The hostname of the ingest Samba server
    pub smb_hostname: String,

    /// The resolved IP address of the Samba server (lazily looked up on first mount)
    pub smb_ip: OnceCell<String>,

    /// The SMB port to connect to
    pub smb_port: u32,

    /// The Samba domain to use for mounting
    pub smb_domain: String,

    /// The identity (agent and resource, or anonymous)
    pub identity: Identity,

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

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("invalid url: {url}")]
    InvalidUrl {
        url: String,
        source: url::ParseError,
    },
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

    pub fn from_file(path: &Path) -> Result<Self, ()> {
        todo!();
    }

    pub fn from_args(args: &CliArgs) -> Result<Self, ConfigError> {
        Ok(Self {
            api_url: Url::parse(&args.event_url).map_err(|e| ConfigError::InvalidUrl {
                url: args.event_url.to_owned(),
                source: e,
            })?,
            mgmt_api_url: args.mgmt_api_url,
            smb_hostname: args.smb_hostname,
            smb_domain: args.smb_domain,
            smb_ip: OnceCell::new(),
            smb_port: args.smb_port,
            identity,
            linux_config: Some(AgentLinuxConfig::default()),
        })
    }

    pub fn update_from_args(&mut self, args: &CliArgs) {
        todo!();
    }
}
