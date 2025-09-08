use std::path::PathBuf;

use anyhow::{Context, Result as AnyResult};
use clap::Parser;
use log::info;
use modama_common::logging::init_logging;
use smbdrop::{
    events::start_event_server,
    server::{serve_smbdrop_api, ApiState},
    share_manager::ShareManager,
    token_server::start_token_server,
};
use tokio::{select, signal::unix::SignalKind};

#[derive(Parser, Debug)]
struct Args {
    #[arg(long, default_value = "/etc/smbdrop/shares.json")]
    shares_config: PathBuf,

    #[arg(long, default_value = "/etc/smbdrop/certs/ca-cert.pem")]
    ca_cert: PathBuf,

    #[arg(long, default_value = "/etc/smbdrop/certs/privkey.pem")]
    privkey: PathBuf,

    #[arg(long, default_value = "/etc/smbdrop/certs/chain.pem")]
    chain: PathBuf,

    #[arg(long, default_value = "/etc/samba/smb-dynamic.conf")]
    samba_config_file: PathBuf,

    #[arg(long, default_value = "/run/smbdrop/")]
    bind_mount_prefix: PathBuf,

    #[arg(long, default_value = "localhost:5051")]
    api_listen: String,

    #[arg(long, default_value = "localhost:5052")]
    ws_listen: String,

    #[arg(long, default_value = "localhost:5053")]
    token_api_listen: String,
}

#[tokio::main]
async fn main() -> AnyResult<()> {
    init_logging();
    let args = Args::parse();
    let mut sm = ShareManager::new(
        args.bind_mount_prefix,
        args.shares_config,
        args.samba_config_file,
    )
    .await;
    sm.start().await?;
    let initial_state = sm.get_state().await.context("get initial state")?;

    info!("initial state: {initial_state:?}");

    let state_updates_tx = start_event_server(
        &args.ws_listen,
        &args.ca_cert,
        &args.chain,
        &args.privkey,
        &initial_state,
    )
    .await
    .context("start event server")?;

    let api_state: ApiState = ApiState::new(sm, state_updates_tx);

    let api_listen = args.api_listen.clone();
    let smbdrop_join_handle = tokio::spawn(async move {
        serve_smbdrop_api(api_state, &api_listen).await
    });

    let token_api_listen = args.token_api_listen.clone();
    let token_join_handle = tokio::spawn(async move {
        start_token_server(&token_api_listen, &args.ca_cert, &args.chain, &args.privkey)
            .await
    });

    let mut term_stream = tokio::signal::unix::signal(SignalKind::terminate())?;

    select! {
        _ = tokio::signal::ctrl_c() => {
            eprintln!("SIGINT received, exiting");
        },
        _ = term_stream.recv() => {
            eprintln!("SIGTERM received, exiting");
        }
        token_res = token_join_handle => {
            eprintln!("token server terminated");
            token_res??;  // propagate JoinError and ServerStartError
        }
        smbdrop_res = smbdrop_join_handle => {
            eprintln!("smbdrop server terminated");
            smbdrop_res??;  // propagate JoinError and ServerStartError
        }
    }

    Ok(())
}
