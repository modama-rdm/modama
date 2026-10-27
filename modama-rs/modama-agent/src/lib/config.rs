use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use clap::{Parser, Subcommand};
use modama_common::{
    agent::{AgentId, ClientCertError},
    cert_helpers::{
        CertHelperError, cert_chain_from_reader, get_root_cert_store, private_key_from_reader,
    },
    config::{AgentConfigFile, AgentConfigFileClientCert, AgentConfigFileLinux},
    tokens::{TokenError, TokenFetcher, TokenProvider},
};
use thiserror::Error;
use tokio::{io::AsyncReadExt, sync::OnceCell};
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use url::Url;

use crate::local_net_token_provider::{DiscoveryAddr, LocalNetTokenProvider};

#[derive(Parser, Debug)]
pub struct CliArgs {
    #[arg(long, global = true)]
    pub config_path: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Clone, Debug, Subcommand)]
pub enum Command {
    /// Run under an operating-system service manager.
    RunService,

    /// Install and start the Windows service.
    #[cfg(windows)]
    InstallAsService,

    /// Stop and unregister the Windows service, preserving installed files.
    #[cfg(windows)]
    UninstallService,

    /// Start the installed Windows service.
    #[cfg(windows)]
    StartService,

    /// Stop the installed Windows service.
    #[cfg(windows)]
    StopService,

    /// Print the installed Windows service status.
    #[cfg(windows)]
    ServiceStatus,
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

impl From<AgentConfigFileLinux> for AgentLinuxConfig {
    fn from(value: AgentConfigFileLinux) -> Self {
        let defaults = Self::default();
        Self {
            mount_point_prefix: value
                .mount_point_prefix
                .unwrap_or(defaults.mount_point_prefix),
            uid: value.uid.unwrap_or(defaults.uid),
            gid: value.gid.unwrap_or(defaults.gid),
        }
    }
}

#[derive(Debug, Clone)]
pub enum MTLSIdentity {
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
pub struct ClientCertConfig {
    chain: String,
    private_key: String,
}

impl From<AgentConfigFileClientCert> for ClientCertConfig {
    fn from(value: AgentConfigFileClientCert) -> Self {
        Self {
            chain: value.chain,
            private_key: value.private_key,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Host:port where the local API should listen
    pub listen_local_api: String,

    /// Base URL for the websocket API (ws:// or wss://)
    pub events_api_url: Url,

    /// Base URL of the management API
    pub mgmt_api_url: Url,

    /// Base URL of the token service API
    pub token_service_url: Url,

    /// Hostname of the ingest Samba server
    pub smb_hostname: String,

    /// Resolved IP address of the Samba server (lazily looked up on first mount)
    pub smb_ip: OnceCell<String>,

    /// SMB port to connect to
    pub smb_port: u32,

    /// Samba domain to use for mounting
    pub smb_domain: String,

    // FIXME: find a better way?
    pub linux_config: Option<AgentLinuxConfig>,

    /// CA certificate
    pub ca_root_store: RootCertStore,

    /// CA root certificate as string
    pub ca_root_cert: String,

    /// TLS client config
    pub client_config: Arc<ClientConfig>,

    /// TLS identity
    pub identity: MTLSIdentity,

    /// Client certificate if available
    pub client_cert: Option<ClientCertConfig>,
}

#[derive(Error, Debug)]
pub enum DNSLookupFailure {
    #[error("no address found for `{hostname}`")]
    NoAddressFound { hostname: String },

    #[error("DNS lookup failure: {0}")]
    IOError(#[from] std::io::Error),
}

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("invalid url: {url}")]
    InvalidUrl {
        url: String,
        source: url::ParseError,
    },

    #[error("i/o error: {0}")]
    IO(#[from] std::io::Error),

    #[error("config parse error: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("certificate error: {0}")]
    CertError(#[from] CertHelperError),

    #[error("certificate error: {0}")]
    ClientCertError(#[from] ClientCertError),

    #[error("rustls error: {0}")]
    TLSError(#[from] tokio_rustls::rustls::Error),

    #[error("configuration error: {msg}")]
    Config { msg: String },

    #[error("token provider: {0}")]
    TokenProvider(#[from] TokenError),
}

fn parse_url(url_str: &str) -> Result<Url, ConfigError> {
    Url::parse(url_str).map_err(|e| ConfigError::InvalidUrl {
        url: url_str.to_owned(),
        source: e,
    })
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

    pub async fn from_file(path: &impl AsRef<Path>) -> Result<Self, ConfigError> {
        let mut cfg_str: String = String::new();
        tokio::fs::File::open(path)
            .await?
            .read_to_string(&mut cfg_str)
            .await?;
        let config_file: AgentConfigFile = toml::from_str(&cfg_str)?;
        let mut reader = config_file.ca.ca_cert.as_bytes();

        let ca_root_store: RootCertStore = get_root_cert_store(&mut reader, path.as_ref())?;

        let token_service_url = parse_url(&config_file.services.token_service_url)?;

        let client_cert: Option<ClientCertConfig> = config_file.client_cert.map(|c| c.into());

        let client_config = match &client_cert {
            Some(client_cert) => {
                let mut chain_bytes = client_cert.chain.as_bytes();
                let client_chain = cert_chain_from_reader(&mut chain_bytes)?;

                let mut pk_bytes = client_cert.private_key.as_bytes();
                let private_key = private_key_from_reader(&mut pk_bytes)?;
                ClientConfig::builder()
                    .with_root_certificates(ca_root_store.clone())
                    .with_client_auth_cert(client_chain, private_key)?
            }
            None => ClientConfig::builder()
                .with_root_certificates(ca_root_store.clone())
                .with_no_client_auth(),
        };

        let identity = match &client_cert {
            Some(client_cert) => {
                let mut chain_bytes = client_cert.chain.as_bytes();
                let client_chain = cert_chain_from_reader(&mut chain_bytes)?;
                let agent_cert = client_chain.first().ok_or(ConfigError::Config {
                    msg: "must have at least one certificate in the chain".to_owned(),
                })?;
                let agent_id = AgentId::from_cert(agent_cert)?;
                MTLSIdentity::AgentId {
                    agent_name: agent_id.agent,
                    resource_name: agent_id.resource,
                }
            }
            None => MTLSIdentity::Anonymous {
                hostname: hostname::get()?.to_string_lossy().to_string(),
            },
        };

        Ok(Self {
            events_api_url: parse_url(&config_file.services.event_url)?,
            mgmt_api_url: parse_url(&config_file.services.mgmt_api_url)?,
            smb_hostname: config_file.services.smb_hostname.clone(),
            smb_ip: OnceCell::new(),
            smb_port: config_file.services.smb_port,
            smb_domain: config_file.services.smb_domain.clone(),
            linux_config: Some(config_file.linux.map(Into::into).unwrap_or_default()),
            ca_root_store,
            listen_local_api: config_file.listen_local_api.clone(),
            token_service_url,
            client_cert,
            ca_root_cert: config_file.ca.ca_cert.clone(),
            client_config: Arc::new(client_config),
            identity,
        })
    }

    pub async fn make_token_provider(
        &self,
    ) -> Result<Arc<dyn TokenProvider + Sync + Send>, ConfigError> {
        let sign_url =
            self.token_service_url
                .join("/sign/")
                .map_err(|e| ConfigError::InvalidUrl {
                    url: self.token_service_url.to_string(),
                    source: e,
                })?;

        Ok(match &self.client_cert {
            Some(client_cert) => {
                let mut chain_reader = client_cert.chain.as_bytes();
                let mut key_reader = client_cert.private_key.as_bytes();
                let mut ca_cert_reader = self.ca_root_cert.as_bytes();
                let token_fetcher = TokenFetcher::new(
                    sign_url.as_str(),
                    &mut key_reader,
                    &mut chain_reader,
                    &mut ca_cert_reader,
                )?;
                let token_provider: Arc<dyn TokenProvider + Sync + Send> = Arc::new(token_fetcher);
                token_provider
            }
            None => {
                let mut ca_cert_reader = self.ca_root_cert.as_bytes();
                let token_provider =
                    LocalNetTokenProvider::new(&mut ca_cert_reader, DiscoveryAddr::AnyPrivate)
                        .await?;
                let token_provider: Arc<dyn TokenProvider + Sync + Send> = Arc::new(token_provider);
                token_provider
            }
        })
    }
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn foreground_invocation_remains_compatible() -> Result<(), clap::Error> {
        let args = CliArgs::try_parse_from(["modama-agent", "--config-path", "agent.toml"])?;
        assert_eq!(args.config_path, Some(PathBuf::from("agent.toml")));
        assert!(args.command.is_none());
        Ok(())
    }

    #[test]
    fn run_service_accepts_global_config_after_subcommand() -> Result<(), clap::Error> {
        let args = CliArgs::try_parse_from([
            "modama-agent",
            "run-service",
            "--config-path",
            "agent.toml",
        ])?;
        assert!(matches!(args.command, Some(Command::RunService)));
        assert_eq!(args.config_path, Some(PathBuf::from("agent.toml")));
        Ok(())
    }
}
