use std::path::PathBuf;

use tokio::sync::mpsc::Sender;

use crate::{process_manager::{SambaProcessManager, PmCommand}, mount_manager::MountManager};

pub struct Share {
    share_id: String,
    share_name: String,
    dest_path: PathBuf,
    created_timestamp: u64,  // unix timestamp

    // temporary credentials:
    username: String,
    password: String,

    // mapped to this unix user:
    uid: u32,
    gid: u32,
}

pub struct ShareManager {
    mm: MountManager,
    pm_tx: Sender<PmCommand>,
}

impl ShareManager {
    async fn new(bind_mount_prefix: PathBuf) -> Self {
        let pm_tx = SambaProcessManager::start_in_background().await;

        Self {
            pm_tx,
            mm: MountManager::new(bind_mount_prefix)
        }
    }
}