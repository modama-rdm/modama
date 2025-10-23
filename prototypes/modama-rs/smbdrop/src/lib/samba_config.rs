use std::path::{Path, PathBuf};

use modama_common::messages::Share;
use tokio::{
    fs::{rename, File},
    io::AsyncWriteExt,
};

use crate::{
    mount::BindMount,
    share_manager::{make_temporary_fname, TempConfigError},
};

#[derive(thiserror::Error, Debug)]
pub enum UpdateSharesError {
    #[error("I/O error")]
    IoError(#[from] std::io::Error),

    #[error("could not write temporary config: {0}")]
    TempConfig(#[from] TempConfigError),
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
#[derive(Clone, Debug)]
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
        let rand_name = make_temporary_fname(&self.config_path)?;
        let mut opts = File::options();
        opts.mode(0o640).create_new(true).write(true);
        let mut f = opts.open(&rand_name).await?;

        f.write_all(contents.as_bytes()).await?;
        f.sync_all().await?;

        rename(&rand_name, &self.config_path).await?;

        Ok(())
    }

    pub async fn update_shares<'a, I>(&self, shares: I) -> Result<(), UpdateSharesError>
    where
        I: Iterator<Item = &'a Share>,
    {
        let snippets: Vec<String> = shares
            .map(|s| {
                let mount = BindMount::from_share(s, &self.bind_mount_prefix);
                get_config_snippet(&mount.mount_point, s)
            })
            .collect();
        let new_config = snippets.join("\n\n");
        self.write_config(&new_config).await?;

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[allow(clippy::expect_used)]
mod tests {
    use std::{path::PathBuf, str::FromStr};

    use modama_common::{messages::Share, test_helpers::TempDir};
    use tokio::{fs::File, io::AsyncReadExt};

    use crate::samba_config::SambaConfigManager;

    #[tokio::test]
    async fn test_empty_config() {
        let tmp = TempDir::new("samba_config");
        let smb_cfg = tmp.join("smb.conf");
        let scm = SambaConfigManager::new(&smb_cfg, &PathBuf::from_str("/run/mounts/").unwrap());
        scm.update_shares([].iter()).await.unwrap();

        let mut f = File::open(&smb_cfg).await.unwrap();
        let mut contents = String::new();
        f.read_to_string(&mut contents).await.unwrap();

        assert_eq!(contents, "");
    }

    #[tokio::test]
    async fn test_simple_config() {
        let tmp = TempDir::new("samba_config");
        let smb_cfg = tmp.join("smb.conf");
        let scm = SambaConfigManager::new(&smb_cfg, &PathBuf::from_str("/run/mounts/").unwrap());
        scm.update_shares(
            [Share {
                resource: Some("res".to_owned()),
                share_id: "share-123".to_owned(),
                share_name: "share-123".to_owned(),
                dest_path: "/adhoc/the-project/test-session/".to_owned(),
                created_timestamp: 0,
                username: "share-user".to_owned(),
                password: "share-pass".to_owned(),
                uid: 1001,
                gid: 1001,
            }]
            .iter(),
        )
        .await
        .unwrap();

        let mut f = File::open(&smb_cfg).await.unwrap();
        let mut contents = String::new();
        f.read_to_string(&mut contents).await.unwrap();

        assert_eq!(contents, "\n[share-123]\npath = \"/run/mounts/shares/share-123\"\nread only = no\nforce create mode = 0600\nforce directory mode = 0700\nvalid users = share-user\n")
    }
}
