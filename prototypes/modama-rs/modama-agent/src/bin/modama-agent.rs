use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result as AnyResult};
use clap::{Args, Parser};
use log::info;
use modama_agent::{
    agent_state::AgentState,
    config::{AgentConfig, AgentLinuxConfig, Identity},
    local_http_service::serve_local_web_api,
    local_net_token_provider::{DiscoveryAddr, LocalNetTokenProvider},
    local_net_token_publisher::token_publisher_loop,
    message_loop::message_loop,
    platform::CurrentPlatformMountBackend,
};
use modama_common::{
    agent::AgentId, cert_helpers::load_tls_server_config, tokens::{TokenFetcher, TokenProvider}
};
use modama_common::{
    cert_helpers::{get_cert_chain, get_private_key, get_root_cert_store},
    logging::init_logging,
};
use tokio::{
    select, spawn,
    sync::{Mutex, OnceCell},
};
use tokio_rustls::rustls::ClientConfig;
use url::Url;

#[derive(Args, Debug)]
struct ClientCertArgs {
    #[arg(long, default_value = "chain.pem")]
    chain: PathBuf,

    #[arg(long, default_value = "priv.pem")]
    key: PathBuf,
}

#[derive(Parser, Debug)]
struct CliArgs {
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

fn get_tls_config(args: &CliArgs) -> AnyResult<(Arc<ClientConfig>, Identity)> {
    let root_store = get_root_cert_store(&args.ca_cert)?;

    let (client_config, identity) = match &args.client_cert {
        Some(client_cert) => {
            let client_chain = get_cert_chain(&client_cert.chain)?;
            let agent_id = AgentId::from_cert(
                client_chain
                    .first()
                    .context("need at least one cert in chain")?,
            )
            .context("loading agent id from client cert failed")?;
            (
                ClientConfig::builder()
                    .with_root_certificates(root_store)
                    .with_client_auth_cert(client_chain, get_private_key(&client_cert.key)?)?,
                Identity::AgentId {
                    agent_name: agent_id.agent,
                    resource_name: agent_id.resource,
                },
            )
        }
        None => (
            ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_no_client_auth(),
            Identity::Anonymous {
                hostname: hostname::get()?.to_string_lossy().to_string(),
            },
        ),
    };

    Ok((Arc::new(client_config), identity))
}

#[tokio::main]
async fn main() -> AnyResult<()> {
    init_logging();

    let args = CliArgs::parse();
    let (tls_config, identity) = get_tls_config(&args)?;

    let config = &AgentConfig {
        api_url: Url::parse(&args.event_url)
            .with_context(|| format!("invalid websocket URL `{}`", &args.event_url))?,
        mgmt_api_url: args.mgmt_api_url,
        smb_hostname: args.smb_hostname,
        smb_domain: args.smb_domain,
        smb_ip: OnceCell::new(),
        smb_port: args.smb_port,
        identity,
        linux_config: Some(AgentLinuxConfig::default()),
    };

    info!("This is modama-agent `{:?}`", &config.identity,);

    let state: Arc<Mutex<AgentState>> = Arc::new(Mutex::new(AgentState::new(config)));

    let sign_url = Url::parse(&args.token_service)
        .with_context(|| format!("invalid sign url `{}`", &args.token_service))?
        .join("/sign/")
        .with_context(|| format!("invalid sign url `{}`", &args.token_service))?;

    let token_provider = match &args.client_cert {
        Some(client_cert) => {
            let token_fetcher = TokenFetcher::new(
                sign_url.as_str(),
                &client_cert.key,
                &client_cert.chain,
                &args.ca_cert,
            )?;
            let token_provider: Arc<dyn TokenProvider + Sync + Send> = Arc::new(token_fetcher);
            token_provider
        }
        None => {
            let token_provider =
                LocalNetTokenProvider::new(&args.ca_cert, DiscoveryAddr::AnyPrivate).await?;
            let token_provider: Arc<dyn TokenProvider + Sync + Send> = Arc::new(token_provider);
            token_provider
        }
    };

    let another_token_provider = Arc::clone(&token_provider);
    let _local_token_server_handle = match &args.client_cert {
        Some(client_cert) => {
            let tls_config = Arc::new(
                load_tls_server_config(&args.ca_cert, &client_cert.chain, &client_cert.key, false)
                    .context("tls config for local token provider API")?,
            );
            Some(spawn(async move {
                token_publisher_loop(another_token_provider, tls_config).await;
            }))
        }
        None => None,
    };

    let state_for_api = Arc::clone(&state);
    let cloned_token_provider = Arc::clone(&token_provider);
    let local_api_handle = tokio::spawn(async move {
        serve_local_web_api(state_for_api, &args.listen_local_api, cloned_token_provider).await
    });

    let mount_backend = CurrentPlatformMountBackend {};

    select! {
        _ = message_loop(&mount_backend, Arc::clone(&state), Arc::clone(&tls_config), Arc::clone(&token_provider)) => {
            Ok(())
        }
        res = local_api_handle => {
            res?
        }
    }?;

    Ok(())
}
