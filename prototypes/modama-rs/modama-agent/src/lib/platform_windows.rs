use std::{
    collections::{HashMap, HashSet},
    ffi::c_void,
    io::{stdout, Write},
    mem::transmute,
    path::PathBuf,
};

use futures_util::future::join_all;
use log::{debug, warn};
use modama_common::messages::Share;
use widestring::U16CString;
use windows::{
    core::{HRESULT, PWSTR},
    Win32::{
        Foundation::{ERROR_NO_MORE_ITEMS, HANDLE},
        NetworkManagement::{
            NetManagement::{USE_DISKDEV, USE_INFO_2, USE_OK},
            WNet::{
                WNetCloseEnum, WNetEnumResourceW, WNetOpenEnumW, CONNECT_TEMPORARY, NETRESOURCEW,
                RESOURCETYPE_DISK, RESOURCE_CONNECTED, WNET_OPEN_ENUM_USAGE,
            },
        },
    },
};

use crate::{config::AgentConfig, platform_common::EnsureMountedError, share::MountedShare};

// TODO:
// - implement `ensure_umounted`
// - proper error handling (get rid of all `unwrap`s)
// - proper logging of debug info, warnings and errors (search for println and variants)
// - un-pub all the internal functions
// - double-check the unsafe blocks (miri?)
// - get rid of `transmute`, if possible!
// - check alignment requirements of the allocation for the enumeration of mapped drives

/// Clean up ingest mount prefix directory,
/// remove all directories that are not currently active mount points.
/// Best-effort; ignores and logs errors and continues to hopefully converge
/// in a clean situation eventually.
pub fn cleanup_mount_points(_config: &AgentConfig) {
    debug!("nothing to do on windows; mapped drives are used instead of mount points");
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

    let source = format!("\\\\{}\\{}", &config.smb_hostname, share.share_name);
    let mapped_drives = get_mapped_drives().unwrap();
    let remote_shares: HashMap<String, MappedDrive> = mapped_drives
        .iter()
        .cloned()
        .map(|md| (md.remote_share.to_owned(), md))
        .collect();

    // already mounted? okay!
    if let Some(md) = remote_shares.get(&source) {
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

    let drive = find_free_drive().unwrap();

    eprintln!("mapping {source} at {drive}");

    let net_use = NetUseParams {
        remote_name: U16CString::from_str(source).unwrap(),
        local_name: U16CString::from_str(&drive).unwrap(),
        domain: U16CString::from_str(domain).unwrap(),
        username: U16CString::from_str(username).unwrap(),
        password: U16CString::from_str(&share.password).unwrap(),
    };
    if let Err(e) = perform_net_use(net_use) {
        Err(EnsureMountedError::MountFailed(e.message()))
    } else {
        Ok(MountedShare {
            share: share.clone(),
            mount_path: PathBuf::from(&drive),
        })
    }
}

/// Pick a free drive, returns for example "Z:". Returns `None` if there is no
/// free drive.
pub fn find_free_drive() -> Option<String> {
    let drives = get_mapped_drives().unwrap();

    let used_drives: HashSet<_> = drives.into_iter().map(|md| md.drive).collect();
    let all_drives: HashSet<_> = ('A'..='Z').map(|c| format!("{c}:")).collect();

    all_drives
        .difference(&used_drives)
        .max()
        .map(|i| i.to_owned())
}

/// Umount all shares that are no longer supposed to be mounted
pub async fn umount_old_shares(config: &AgentConfig, shares: &[MountedShare]) {
    let all_drives = get_mapped_drives().unwrap();
    let our_prefix = format!("\\\\{}", &config.smb_hostname);
    let our_drives: Vec<_> = all_drives
        .iter()
        .filter(|md| md.remote_share.starts_with(&our_prefix))
        .collect();
    let should_be_mapped: HashSet<PathBuf> = shares.iter().map(|s| s.mount_path.clone()).collect();

    join_all(
        our_drives
            .clone()
            .into_iter()
            .filter(|m| should_be_mapped.contains(&PathBuf::from(&m.drive)))
            .map(|m| async move { ensure_umounted(&m.drive, &config).await }),
    )
    .await
    .into_iter()
    .for_each(|result| {
        if let Err(err) = result {
            warn!("Umounting failed: {err}");
        }
    });
}

pub async fn ensure_umounted(drive: &str, config: &AgentConfig) -> Result<(), EnsureMountedError> {
    todo!("implement me!")
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
    println!("return value: {result}");

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

pub fn get_mapped_drives() -> Result<Vec<MappedDrive>, HRESULT> {
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

    let mut mapped_drives = Vec::new();

    loop {
        stdout().flush().unwrap();

        let mut count: u32 = 1; // docs say this should be -1 for "as many as possible". for a u32. unsigned. yay?

        // WTF microsoft, I don't want that to be mutable! if it is indeed changed, we just throw it away anyway.
        let mut buffer_size: u32 = 32 * 1024;
        let mut buffer_vec: Vec<u8> = vec![0; buffer_size as usize];

        unsafe {
            let lpbuffer_ptr = buffer_vec.as_mut_ptr();
            let lpbuffer_c_void: *mut c_void = transmute(lpbuffer_ptr);

            let res = WNetEnumResourceW(
                enum_handle,
                std::ptr::from_mut(&mut count),
                lpbuffer_c_void,
                std::ptr::from_mut(&mut buffer_size),
            );

            if res == ERROR_NO_MORE_ITEMS {
                break;
            }

            if res.is_err() {
                res.to_hresult().map(|| ())?
            }

            let res: *mut NETRESOURCEW = transmute(lpbuffer_c_void);
            let res = *res;

            mapped_drives.push(MappedDrive {
                drive: res.lpLocalName.to_string().unwrap(),
                remote_share: res.lpRemoteName.to_string().unwrap(),
            });
        }
    }

    unsafe {
        WNetCloseEnum(enum_handle).to_hresult().map(|| ())?;
    }

    eprintln!("mapped drives: {mapped_drives:?}");

    Ok(mapped_drives)
}
