use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use futures_util::future::join_all;
use log::{debug, error, info, warn};
use modama_common::messages::Share;
use widestring::U16CString;

use crate::{config::AgentConfig, platform_common::EnsureMountedError, share::MountedShare};

// TODO:
// - double-check the unsafe blocks (miri?)
// - get rid of `transmute`, if possible!
// - check alignment requirements of the allocation for the enumeration of mapped drives

/// Clean up ingest mount prefix directory,
/// remove all directories that are not currently active mount points.
/// Best-effort; ignores and logs errors and continues to hopefully converge
/// in a clean situation eventually.
pub fn cleanup_mount_points(_config: &AgentConfig) {
    debug!("cleanup_mount_points: nothing to do on windows; mapped drives are used instead of mount points");
}

/// Ensure that `share` is mounted, and return a `MountedShare` if the mount succeeded.
pub async fn ensure_mounted(
    share: &Share,
    config: &AgentConfig,
) -> Result<MountedShare, EnsureMountedError> {
    // On Windows:
    //
    // - check if a drive is already mapped for the given share
    // - perform the `net use` equivalent win32 call

    let resource_string = format!("\\\\{}\\{}", &config.smb_hostname, share.share_name);
    let mapped_drives =
        internal::get_mapped_drives().map_err(|e| EnsureMountedError::MountFailed(e.message()))?;
    let mapped_remote_shares: HashMap<String, internal::MappedDrive> = mapped_drives
        .iter()
        .cloned()
        .map(|md| (md.remote_share.to_owned(), md))
        .collect();

    // already mounted? okay!
    if let Some(md) = mapped_remote_shares.get(&resource_string) {
        info!("share {} already mounted at {}", &share.share_name, &md.drive);
        return Ok(MountedShare {
            share: share.clone(),
            mount_path: PathBuf::from(&md.drive),
        });
    }

    let (domain, username) = if share.username.contains('\\') {
        let parts: Vec<_> = share.username.split('\\').collect();
        if parts.len() != 2 {
            return Err(EnsureMountedError::CredentialsError(
                "username cannot contain more than one backslash".to_owned(),
            ));
        }
        (parts[0], parts[1])
    } else {
        ("", share.username.as_str())
    };

    let drive =
        internal::find_free_drive().map_err(|e| EnsureMountedError::MountFailed(e.to_string()))?;

    info!("mapping {resource_string} at {drive}");

    let net_use = internal::NetUseParams {
        remote_name: U16CString::from_str_truncate(resource_string),
        local_name: U16CString::from_str_truncate(&drive),
        domain: U16CString::from_str_truncate(domain),
        username: U16CString::from_str_truncate(username),
        password: U16CString::from_str_truncate(&share.password),
    };
    if let Err(e) = internal::perform_net_use(net_use) {
        Err(EnsureMountedError::MountFailed(e.message()))
    } else {
        Ok(MountedShare {
            share: share.clone(),
            mount_path: PathBuf::from(&drive),
        })
    }
}

/// Umount all shares that are no longer supposed to be mounted
pub async fn umount_old_shares(config: &AgentConfig, shares: &[MountedShare]) {
    let all_drives = match internal::get_mapped_drives() {
        Ok(drives) => drives,
        Err(e) => {
            error!("failed to get mapped drives: {e}");
            return;
        }
    };
    let our_prefix = format!("\\\\{}", &config.smb_hostname);
    let our_drives: Vec<_> = all_drives
        .iter()
        .filter(|md| md.remote_share.starts_with(&our_prefix))
        .collect();
    let should_be_mapped: HashSet<PathBuf> = shares.iter().map(|s| s.mount_path.clone()).collect();

    info!("should_be_mapped: {:?}", should_be_mapped);

    join_all(
        our_drives
            .clone()
            .into_iter()
            .filter(|m| !should_be_mapped.contains(&PathBuf::from(&m.drive)))
            .map(|m| async move {
                internal::ensure_umounted(&m.drive, &config).await
            }),
    )
    .await
    .into_iter()
    .for_each(|result| {
        if let Err(err) = result {
            warn!("Umounting failed: {err}");
        }
    });
}

pub mod internal {
    use std::{collections::HashSet, ffi::c_void, mem::transmute};

    use log::{debug, error, info};
    use thiserror::Error;
    use widestring::U16CString;
    use windows::{
        core::{HRESULT, PCWSTR, PWSTR},
        Win32::{
            Foundation::{ERROR_NO_MORE_ITEMS, HANDLE},
            NetworkManagement::{
                NetManagement::{USE_DISKDEV, USE_INFO_2, USE_OK},
                WNet::{
                    WNetCancelConnection2W, WNetCloseEnum, WNetEnumResourceW, WNetOpenEnumW,
                    CONNECT_TEMPORARY, CONNECT_UPDATE_PROFILE, NETRESOURCEW, RESOURCETYPE_DISK,
                    RESOURCE_CONNECTED, WNET_OPEN_ENUM_USAGE,
                },
            },
        },
    };

    use crate::{config::AgentConfig, platform_common::EnsureMountedError};

    #[derive(Error, Debug)]
    pub enum FreeDriveError {
        #[error("no free drive letter available")]
        NoFreeDrive,

        #[error("could not get mapped drives: {0}")]
        OSError(String),
    }

    /// Pick a free drive, returns for example "Z:". Returns
    /// `FreeDriveError::NoFreeDrive` if there is no free drive available
    pub fn find_free_drive() -> Result<String, FreeDriveError> {
        let drives = get_mapped_drives().map_err(|e| FreeDriveError::OSError(e.message()))?;

        let used_drives: HashSet<_> = drives.into_iter().map(|md| md.drive).collect();
        let all_drives: HashSet<_> = ('A'..='Z').map(|c| format!("{c}:")).collect();

        Ok(all_drives
            .difference(&used_drives)
            .max()
            .map_or_else(|| Err(FreeDriveError::NoFreeDrive), |i| Ok(i.to_owned()))?)
    }

    pub async fn ensure_umounted(
        drive: &str,
        _config: &AgentConfig,
    ) -> Result<(), EnsureMountedError> {
        info!("umounting drive {drive}");
        if net_use_delete(drive, false).is_err() {
            Err(EnsureMountedError::InUseError)
        } else {
            Ok(())
        }
    }

    pub fn net_use_delete(drive: &str, force: bool) -> Result<(), HRESULT> {
        let name: U16CString = U16CString::from_str_truncate(drive);
        let name_ptr = name.as_ptr();
        Ok(unsafe {
            WNetCancelConnection2W(PCWSTR::from_raw(name_ptr), CONNECT_UPDATE_PROFILE.0, force)
        }
        .to_hresult()
        .map(|| ())?)
    }

    pub struct NetUseParams {
        /// Something like \\host\share
        pub remote_name: U16CString,

        /// The local drive letter to redirect to the remote system
        pub local_name: U16CString,

        /// Windows Domain
        pub domain: U16CString,

        /// Username without the domain prefix
        pub username: U16CString,

        /// Password
        pub password: U16CString,
    }

    /// At least on Windows Server 2019, drives mapped don't immediately appear in the explorer,
    /// it has to be restarted before the action takes effect.
    pub fn perform_net_use_variant(mut params: NetUseParams) -> Result<(), u32> {
        let buf = USE_INFO_2 {
            ui2_local: PWSTR::from_raw(params.local_name.as_mut_ptr()),
            ui2_remote: PWSTR::from_raw(params.remote_name.as_mut_ptr()),
            ui2_password: PWSTR::from_raw(params.password.as_mut_ptr()),
            ui2_status: USE_OK, // unused
            ui2_asg_type: USE_DISKDEV,
            ui2_refcount: 0, // unused
            ui2_usecount: 1, // ???
            ui2_username: PWSTR::from_raw(params.username.as_mut_ptr()),
            ui2_domainname: PWSTR::from_raw(params.domain.as_mut_ptr()),
        };

        // SAFETY: `buf` needs to live until the function returns
        let result = unsafe {
            let buf_ptr = std::ptr::from_ref(&buf) as *const u8;
            windows::Win32::NetworkManagement::NetManagement::NetUseAdd(None, 2, buf_ptr, None)
        };
        debug!("return value: {result}");

        Ok(())
    }

    pub fn perform_net_use(mut params: NetUseParams) -> Result<(), HRESULT> {
        let resource = NETRESOURCEW {
            dwType: RESOURCETYPE_DISK,
            lpLocalName: PWSTR::from_raw(params.local_name.as_mut_ptr()),
            lpRemoteName: PWSTR::from_raw(params.remote_name.as_mut_ptr()),
            ..Default::default()
        };

        // SAFETY: `params` and `resource` need to live until after the `unsafe` block
        let result = unsafe {
            let resource_ptr = std::ptr::from_ref(&resource);
            windows::Win32::NetworkManagement::WNet::WNetAddConnection2W(
                resource_ptr,
                PWSTR::from_raw(params.password.as_mut_ptr()),
                PWSTR::from_raw(params.username.as_mut_ptr()),
                CONNECT_TEMPORARY.0,
            )
        };

        if result.is_ok() {
            Ok(())
        } else {
            Err(result.to_hresult())
        }
    }

    #[derive(Debug, Clone)]
    pub struct MappedDrive {
        /// The drive where the share is mapped to, including the colon
        pub drive: String,

        /// The remote share name, in the format "\\host\share"
        pub remote_share: String,
    }

    struct MappedDriveIter {
        enum_handle: HANDLE,
    }

    impl MappedDriveIter {
        fn new() -> Result<Self, HRESULT> {
            let mut enum_handle = HANDLE::default();

            unsafe {
                WNetOpenEnumW(
                    RESOURCE_CONNECTED,
                    RESOURCETYPE_DISK,
                    WNET_OPEN_ENUM_USAGE(0),
                    None,
                    std::ptr::from_mut(&mut enum_handle),
                )
                .to_hresult()
                .map(|| ())?;
            }

            Ok(Self { enum_handle })
        }
    }

    impl Drop for MappedDriveIter {
        fn drop(&mut self) {
            unsafe {
                let res = WNetCloseEnum(*&mut self.enum_handle)
                    .to_hresult()
                    .map(|| ());
                if let Err(e) = res {
                    error!("WNetCloseEnum failed: {e}");
                }
            }
        }
    }

    impl Iterator for MappedDriveIter {
        type Item = Result<MappedDrive, HRESULT>;

        fn next(&mut self) -> Option<Self::Item> {
            let mut count: u32 = 1; // docs say this should be -1 for "as many as possible". for a u32. unsigned. yay?
                                    // WTF microsoft, I don't want that to be mutable! if it is indeed changed, we just throw it away anyway.
            let mut buffer_size: u32 = 32 * 1024;
            let mut buffer_vec: Vec<u8> = vec![0; buffer_size as usize];

            unsafe {
                let lpbuffer_ptr = buffer_vec.as_mut_ptr();
                let lpbuffer_c_void: *mut c_void = transmute(lpbuffer_ptr);

                let res = WNetEnumResourceW(
                    *&mut self.enum_handle,
                    std::ptr::from_mut(&mut count),
                    lpbuffer_c_void,
                    std::ptr::from_mut(&mut buffer_size),
                );

                if res == ERROR_NO_MORE_ITEMS {
                    return None;
                }

                if res.is_err() {
                    return Some(Err(res.to_hresult()));
                }

                let res: *mut NETRESOURCEW = transmute(lpbuffer_c_void);
                let res = *res;

                // be sure to copy out any data needed, as it will become
                // unavailable after `buffer_vec` is dropped!
                let drive = res
                    .lpLocalName
                    .to_string()
                    .inspect_err(|e| {
                        error!(
                            "invalid utf16 encountered in local name, returning dummy value! ({e})"
                        );
                    })
                    .unwrap_or("".to_owned());
                let remote_share = res.lpRemoteName.to_string().inspect_err(|e| {
                    error!("invalid utf16 encountered in remote name, returning dummy value! ({e})");
                }).unwrap_or("".to_owned());
                return Some(Ok(MappedDrive {
                    drive,
                    remote_share,
                }));
            }
        }
    }

    pub fn get_mapped_drives() -> Result<Vec<MappedDrive>, HRESULT> {
        let mapped_drives = MappedDriveIter::new()?.collect::<Result<Vec<MappedDrive>, _>>()?;
        debug!("mapped drives: {mapped_drives:?}");
        Ok(mapped_drives)
    }
}

pub struct CurrentPlatformMountBackend {
}

impl MountBackend for CurrentPlatformMountBackend {
    async fn initialize(&self, config: &AgentConfig) {
        // nothing to do yet
    }

    async fn cleanup_mount_points(&self, config: &AgentConfig) {
        cleanup_mount_points(config)
    }

    async fn ensure_mounted(&self, share: &Share, config: &AgentConfig) -> Result<MountedShare, EnsureMountedError> {
        ensure_mounted(share, config).await
    }

    async fn umount_old_shares(&self, config: &AgentConfig, shares: &[MountedShare]) {
        umount_old_shares(config, shares).await
    }
}