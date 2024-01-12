use std::{path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, Result as AnyResult};
use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use axum_extra::{
    headers::{authorization::Basic, Authorization},
    typed_header::TypedHeaderRejection,
    TypedHeader,
};
use axum_macros::debug_handler;
use clap::Parser;
use futures_util::{future::join_all, StreamExt};
use log::{debug, error, info, warn};
use minijinja::{context, Environment};
use modama_agent::{
    config::{AgentConfig, AgentLinuxConfig},
    platform::{cleanup_mount_points, ensure_mounted, umount_old_shares},
    share::MountedShare,
};
use modama_common::{
    agent::AgentId,
    messages::{CurrentForegroundShare, EventChannelMessage, Share},
    mgmt_client::{MgmtClient, MgmtError},
    tokens::TokenFetcher,
};
use modama_common::{
    cert_helpers::{get_cert_chain, get_private_key, get_root_cert_store},
    logging::init_logging,
};
use tokio::sync::{Mutex, OnceCell};
use tokio_rustls::rustls::ClientConfig;
use tokio_tungstenite::connect_async_tls_with_config;
use url::Url;

#[derive(Debug, Default)]
enum ConnectionState {
    #[default]
    Connecting,
    Connected,
}

#[derive(Debug, Clone)]
struct ShareState<T>
where
    T: PartialEq + Clone,
{
    /// The current foreground share, if any.
    foreground_share: Option<T>,

    /// All shares, including the foreground share
    shares: Vec<T>,
}

impl<T> ShareState<T>
where
    T: PartialEq + Clone,
{
    fn set_foreground_share(&mut self, share: Option<T>) {
        self.foreground_share = share;
        self.ensure_consistent();
    }

    fn ensure_consistent(&mut self) {
        // ensure the foreground share is part of the list of all shares:
        if let Some(share) = &self.foreground_share {
            if !self.shares.contains(share) {
                self.shares.push(share.clone());
            }
        }
    }

    fn set_shares(&mut self, shares: Vec<T>) {
        self.shares = shares;
        self.ensure_consistent();
    }
}

impl<T> Default for ShareState<T>
where
    T: PartialEq + Clone,
{
    fn default() -> Self {
        Self {
            foreground_share: None,
            shares: Vec::new(),
        }
    }
}

#[derive(Debug)]
struct AgentState {
    connection: ConnectionState,

    current_shares: ShareState<MountedShare>,
    desired_shares: ShareState<Share>,

    connected_agents: Vec<AgentId>,

    /// Runtime configuration
    config: AgentConfig,
}

impl AgentState {
    fn new(config: &AgentConfig) -> Self {
        let config = config.clone();
        Self {
            connection: Default::default(),
            current_shares: Default::default(),
            desired_shares: Default::default(),
            connected_agents: Default::default(),
            config,
        }
    }
}

async fn update_share_list(
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
                match ensure_mounted(&share, &state.config).await {
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
            ensure_mounted(&s2, &state.config).await
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

    umount_old_shares(&state.config, &state.current_shares.shares).await;
    cleanup_mount_points(&state.config);

    Ok(())
}

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
                shares: shares.into_iter().map(Into::into).collect(),
            };
            state.lock().await.desired_shares = desired_shares.clone();
            state.lock().await.connected_agents = connected_agents;

            update_share_list(&mut *state.lock().await, desired_shares).await?;
            Ok(())
        }
    }
}

async fn message_loop(state: Arc<Mutex<AgentState>>, tls_config: Arc<ClientConfig>) {
    let config = state.lock().await.config.clone();

    loop {
        state.lock().await.connection = ConnectionState::Connecting;
        let (conn, _response) = match connect_async_tls_with_config(
            &config.api_url,
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
                error!("{}", e.to_string());
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
                        if let Err(e) = handle_message(&state, &msg).await {
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

fn create_env() -> Environment<'static> {
    let mut env = Environment::new();

    env.set_loader(|name| match name {
        "start_index_step3.html" => Ok(Some(
            "<html><form action='.' method='post'><input type='submit'></form></html>".to_owned(),
        )),
        _ => Ok(None),
    });

    env
}

/// Wrapper around the AgentState for the local API server
#[derive(Clone)]
struct ApiState {
    pub state: Arc<Mutex<AgentState>>,
    pub token_fetcher: Arc<TokenFetcher>,
    pub jinja_env: Environment<'static>,
}

impl ApiState {
    fn new(state: &Arc<Mutex<AgentState>>, token_fetcher: &Arc<TokenFetcher>) -> Self {
        Self {
            state: Arc::clone(state),
            token_fetcher: Arc::clone(token_fetcher),
            jinja_env: create_env(),
        }
    }
}

// #[debug_handler]
async fn handler_shares_foreground(
    State(api_state): State<ApiState>,
) -> Json<CurrentForegroundShare> {
    Json(
        match &api_state.state.lock().await.current_shares.foreground_share {
            None => CurrentForegroundShare::None,
            Some(s) => match s.mount_path.to_str() {
                None => {
                    let p = s.mount_path.to_string_lossy();
                    warn!("Mount path {p} contains invalid unicode, cannot get foreground share!");
                    CurrentForegroundShare::None
                }
                Some(mount_path) => CurrentForegroundShare::Share {
                    dest_path: s.share.dest_path.to_owned(),
                    mount_path: mount_path.to_owned(),
                },
            },
        },
    )
}

#[debug_handler]
async fn serve_index_page(
    State(agent_state): State<ApiState>,
    auth: Result<TypedHeader<Authorization<Basic>>, TypedHeaderRejection>,
) -> impl IntoResponse {
    let mut result = String::new();

    let auth = match auth {
        Ok(TypedHeader(auth)) => auth,
        Err(e) => {
            error!("error extracting authorization header: {e}");
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=modama-agent")],
            )
                .into_response();
        }
    };

    let state = agent_state.state.lock().await;

    let matching_agents = state
        .connected_agents
        .iter()
        .filter(|a| a.resource == state.config.resource_name);

    result += "Connected agents at this resource: ";
    for a in matching_agents {
        result += &(a.as_uri().to_string() + " ");
    }
    result += "\n";
    result += &format!(
        "This agent: {} at {}",
        &state.config.agent_name, &state.config.resource_name
    );

    result += "\n";

    let c = MgmtClient::new("http://localhost:5001", auth.username(), auth.password()).unwrap();

    let projects = match c.list_projects().await {
        Ok(p) => p,
        Err(MgmtError::Unauthorized) => {
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=modama-agent")],
            )
                .into_response();
        }
        Err(e) => {
            error!("error listing projects: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR).into_response();
        }
    };

    result += "Projects:\n";

    for p in projects
        .projects
        .iter()
        .filter(|p| p.members.contains(&auth.username().to_owned()) || p.owner == auth.username())
    {
        result += &p.name;
        result += "\n";
    }

    result.into_response()
}

async fn start_ingest_step1(
    Path(project_name): Path<String>,
    State(agent_state): State<ApiState>,
    auth: Result<TypedHeader<Authorization<Basic>>, TypedHeaderRejection>,
) -> impl IntoResponse {
    let auth = match auth {
        Ok(TypedHeader(auth)) => auth,
        Err(e) => {
            error!("error extracting authorization header: {e}");
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=modama-agent")],
            )
                .into_response();
        }
    };

    let c = MgmtClient::new("http://localhost:5001", auth.username(), auth.password()).unwrap();

    let details = match c.project_details(&project_name).await {
        Ok(p) => p,
        Err(MgmtError::Unauthorized) => {
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=modama-agent")],
            )
                .into_response();
        }
        Err(e) => {
            error!("error getting project details for {project_name}: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR).into_response();
        }
    };

    let mut resp = String::new();

    for d in &details.datasets {
        resp += &d.name;
        resp += "\n";
    }

    resp.into_response()
}

async fn start_ingest_step2(
    Path((project_name, session_name)): Path<(String, String)>,
    State(agent_state): State<ApiState>,
    auth: Result<TypedHeader<Authorization<Basic>>, TypedHeaderRejection>,
) -> impl IntoResponse {
    let auth = match auth {
        Ok(TypedHeader(auth)) => auth,
        Err(e) => {
            error!("error extracting authorization header: {e}");
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=modama-agent")],
            )
                .into_response();
        }
    };

    let c = MgmtClient::new("http://localhost:5001", auth.username(), auth.password()).unwrap();

    let details = match c.project_details(&project_name).await {
        Ok(p) => p,
        Err(MgmtError::Unauthorized) => {
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=modama-agent")],
            )
                .into_response();
        }
        Err(e) => {
            error!("error getting project details for {project_name}: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR).into_response();
        }
    };

    let mut resp = String::new();

    if !details.datasets.iter().any(|d| d.name == session_name) {
        return (StatusCode::NOT_FOUND).into_response();
    }

    resp += &format!(
        "Starting ingest for resource {} and session {}\n",
        agent_state.state.lock().await.config.resource_name,
        session_name,
    );

    let tpl = agent_state
        .jinja_env
        .get_template("start_index_step3.html")
        .unwrap();
    let res_name = &agent_state.state.lock().await.config.resource_name;
    (
        [(header::CONTENT_TYPE, "text/html")],
        tpl.render(context! { resource => res_name, session => session_name })
            .unwrap(),
    )
        .into_response()
}

async fn start_ingest_step3(
    Path((project_name, session_name)): Path<(String, String)>,
    State(agent_state): State<ApiState>,
    auth: Result<TypedHeader<Authorization<Basic>>, TypedHeaderRejection>,
) -> impl IntoResponse {
    let auth = match auth {
        Ok(TypedHeader(auth)) => auth,
        Err(e) => {
            error!("error extracting authorization header: {e}");
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=modama-agent")],
            )
                .into_response();
        }
    };

    let c = MgmtClient::new("http://localhost:5001", auth.username(), auth.password()).unwrap();

    let details = match c.project_details(&project_name).await {
        Ok(p) => p,
        Err(MgmtError::Unauthorized) => {
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=modama-agent")],
            )
                .into_response();
        }
        Err(e) => {
            error!("error getting project details for {project_name}: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR).into_response();
        }
    };

    let mut resp = String::new();

    if !details.datasets.iter().any(|d| d.name == session_name) {
        return (StatusCode::NOT_FOUND).into_response();
    }

    let agent_token = agent_state.token_fetcher.fetch_token().await.unwrap();

    c.create_share(
        &project_name,
        &session_name,
        &agent_state.state.lock().await.config.resource_name,
        &agent_token,
    )
    .await
    .unwrap();

    resp += "share created!";

    resp.into_response()
}

async fn serve_local_web_api(
    state: Arc<Mutex<AgentState>>,
    listen_addr: &str,
    token_fetcher: TokenFetcher,
) {
    let app = Router::new()
        .route("/shares/foreground/", get(handler_shares_foreground))
        .route("/", get(serve_index_page))
        .route("/start-ingest/:project_name/", get(start_ingest_step1))
        .route(
            "/start-ingest/:project_name/:session_name/",
            get(start_ingest_step2),
        )
        .route(
            "/start-ingest/:project_name/:session_name/",
            post(start_ingest_step3),
        )
        .with_state(ApiState::new(&state, &Arc::new(token_fetcher)));
    let listener = tokio::net::TcpListener::bind(listen_addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

#[derive(Parser, Debug)]
struct Args {
    #[arg(long, default_value = "wss://localhost:5052")]
    event_url: String,

    #[arg(long, default_value = "https://localhost:5053")]
    token_service: String,

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
            .get(0)
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
