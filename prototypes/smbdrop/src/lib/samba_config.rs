use std::path::{Path, PathBuf};

use tokio::{
    fs::{rename, File},
    io::AsyncWriteExt,
};

use crate::share_manager::{make_temporary_fname, Share};

#[derive(thiserror::Error, Debug)]
pub enum UpdateSharesError {
    #[error("I/O error")]
    IoError(#[from] std::io::Error),
}

fn get_config_snippet(dest_path_bind: &Path, share: &Share) -> String {
    let share_name = &share.share_name;
    let username = &share.username;
    let dest_path_bind = dest_path_bind.to_string_lossy();
    format!(
        "
[{share_name}]
path = \"{dest_path_bind}\"
read only = no
force create mode = 0600
force directory mode = 0700
valid users = {username}
"
    )
}
pub struct SambaConfigManager {
    config_path: PathBuf,
    bind_mount_prefix: PathBuf,
}

impl SambaConfigManager {
    pub fn new(config_path: &Path, bind_mount_prefix: &Path) -> Self {
        Self {
            config_path: config_path.to_owned(),
            bind_mount_prefix: bind_mount_prefix.to_owned(),
        }
    }

    async fn write_config(&self, contents: &str) -> Result<(), UpdateSharesError> {
        let rand_name = make_temporary_fname(&self.config_path);
        let mut opts = File::options();
        opts.mode(0o640).create_new(true).write(true);
        let mut f = opts.open(&rand_name).await?;

        f.write_all(contents.as_bytes()).await?;
        f.sync_all().await?;

        rename(&rand_name, &self.config_path).await?;

        Ok(())
    }

    pub async fn update_shares<'a, I>(&self, shares: I) -> Result<(), UpdateSharesError>
    where I: Iterator<Item = &'a Share> {
        let snippets: Vec<String> = shares.map(|s| {
            get_config_snippet(&s.to_mount(&self.bind_mount_prefix).mount_point, s)
        }).collect();
        let new_config = snippets.join("\n\n");
        self.write_config(&new_config).await?;

        Ok(())
    }
}
