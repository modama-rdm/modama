use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use modama_common::logging::init_logging;
use smbdrop::{
    events::start_event_server,
    server::{serve_smbdrop_api, ApiState, serve_smbdrop_privileged_api},
    share_manager::ShareManager,
};
use tokio::{signal::unix::SignalKind, select};

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
    api_priv_listen: String,
}

#[tokio::main]
async fn main() {
    init_logging();
    let args = Args::parse();
    let mut sm = ShareManager::new(args.bind_mount_prefix, args.shares_config, args.samba_config_file).await;
    sm.start().await.expect("could not start share manager");
    let state = sm.get_state().await.context("get initial state").unwrap();

    let state_updates_tx = start_event_server(
        &args.ws_listen,
        &args.ca_cert,
        &args.chain,
        &args.privkey,
        &state,
    )
    .await
    .context("start event server")
    .unwrap();

    let api_state: ApiState = ApiState::new(sm, state_updates_tx);
    let priv_state = api_state.clone();

    let api_listen = args.api_listen.clone();
    let _join_handle = tokio::spawn(async move {
        serve_smbdrop_api(api_state, &api_listen).await;
    });

    let api_priv_listen = args.api_priv_listen.clone();
    let _join_handle = tokio::spawn(async move {
        serve_smbdrop_privileged_api(priv_state, &api_priv_listen).await;
    });

    let mut term_stream = tokio::signal::unix::signal(SignalKind::terminate()).unwrap();

    select! {
        _ = tokio::signal::ctrl_c() => {
            eprintln!("SIGINT received, exiting");
        },
        _ = term_stream.recv() => {
            eprintln!("SIGTERM received, exiting");
        }
    }

}
