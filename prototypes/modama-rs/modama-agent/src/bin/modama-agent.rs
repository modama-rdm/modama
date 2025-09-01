use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result as AnyResult};
use clap::Parser;
use log::info;
use modama_agent::{
    agent_state::AgentState,
    config::{AgentConfig, AgentLinuxConfig},
    local_http_service::serve_local_web_api,
    message_loop::message_loop,
};
use modama_common::{agent::AgentId, tokens::TokenFetcher};
use modama_common::{
    cert_helpers::{get_cert_chain, get_private_key, get_root_cert_store},
    logging::init_logging,
};
use tokio::sync::{Mutex, OnceCell};
use tokio_rustls::rustls::ClientConfig;
use url::Url;

#[derive(Parser, Debug)]
struct Args {
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

    #[arg(long, default_value = "chain.pem")]
    chain: PathBuf,

    #[arg(long, default_value = "priv.pem")]
    key: PathBuf,
}

fn get_tls_config(args: &Args) -> AnyResult<(Arc<ClientConfig>, AgentId)> {
    let root_store = get_root_cert_store(&args.ca_cert)?;
    let client_chain = get_cert_chain(&args.chain)?;
    let agent_id = AgentId::from_cert(
        client_chain
            .first()
            .context("need at least one cert in chain")
            .unwrap(),
    )
    .context("loading agent id from client cert failed")
    .unwrap();
    Ok((
        Arc::new(
            ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_client_auth_cert(client_chain, get_private_key(&args.key)?)?,
        ),
        agent_id,
    ))
}

#[tokio::main]
async fn main() {
    init_logging();

    let args = Args::parse();
    let (tls_config, agent_id) = get_tls_config(&args).unwrap();

    let config = &AgentConfig {
        api_url: Url::parse(&args.event_url)
            .unwrap_or_else(|e| panic!("invalid websocket URL `{}`: {e}", &args.event_url)),
        mgmt_api_url: args.mgmt_api_url,
        smb_hostname: args.smb_hostname,
        smb_domain: args.smb_domain,
        smb_ip: OnceCell::new(),
        smb_port: args.smb_port,
        agent_name: agent_id.agent.to_owned(),
        resource_name: agent_id.resource.to_owned(),
        linux_config: Some(AgentLinuxConfig::default()),
    };

    info!(
        "This is modama-agent `{}` running for resource `{}`",
        &config.agent_name, config.resource_name
    );

    let state: Arc<Mutex<AgentState>> = Arc::new(Mutex::new(AgentState::new(config)));

    let sign_url = Url::parse(&args.token_service)
        .unwrap_or_else(|e| panic!("invalid sign url `{}`: {e}", &args.token_service))
        .join("/sign/")
        .unwrap_or_else(|e| panic!("invalid sign url `{}`: {e}", &args.token_service));

    let token_fetcher =
        TokenFetcher::new(sign_url.as_str(), &args.key, &args.chain, &args.ca_cert).unwrap();

    let state_for_api = Arc::clone(&state);
    tokio::spawn(async move {
        serve_local_web_api(state_for_api, &args.listen_local_api, token_fetcher).await;
    });

    message_loop(Arc::clone(&state), Arc::clone(&tls_config)).await;
}
