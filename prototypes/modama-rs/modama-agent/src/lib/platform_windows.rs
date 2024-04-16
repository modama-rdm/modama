use log::{debug, info};
use modama_common::messages::Share;
use widestring::U16CString;
use windows::{core::{s, w, PSTR, PWSTR}, Win32::NetworkManagement::{NetManagement::{USE_DISKDEV, USE_INFO_2, USE_OK}, WNet::{CONNECT_TEMPORARY, NETRESOURCEA, NETRESOURCEW, RESOURCETYPE_DISK}}};

use crate::{config::AgentConfig, platform_common::EnsureMountedError, share::MountedShare};

/// Clean up ingest mount prefix directory,
/// remove all directories that are not currently active mount points.
/// Best-effort; ignores and logs errors and continues to hopefully converge
/// in a clean situation eventually.
pub fn cleanup_mount_points(config: &AgentConfig) {
    debug!("nothing to do on windows; mapped drives are used instead of mount points");
}

/// Ensure that `share` is mounted, and return a `MountedShare` if the mount succeeded.
pub async fn ensure_mounted(
    share: &Share,
    config: &AgentConfig,
) -> Result<MountedShare, EnsureMountedError> {
    // On Linux, we have to:
    //
    // - take care of creating mount points
    // - check if the mount already exists (here we assume that an existing
    //   mount with the given share name )
    // - specific config for UID/GID handling
    // - perform IP address lookup, as this is done in userspace, not by the kernel

    todo!("figure out how to do this on windows");
}

/// Umount all shares that are no longer supposed to be mounted
pub async fn umount_old_shares(config: &AgentConfig, shares: &[MountedShare]) {
    todo!("figure out how to do this on windows");
}

pub struct NetUseParams {
    /// Something like \\host\share
    pub remote_name: U16CString,

    /// The local drive letter to redirect to the remote system
    pub local_name: U16CString,

    /// Windows Domain
    pub domain: U16CString,

    /// Username
    pub username: U16CString,

    /// Password
    pub password: U16CString,
}

pub fn perform_net_use(mut params: NetUseParams) -> Result<(), u32> {
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

pub fn perform_net_use_alt(mut params: NetUseParams) -> Result<(), String> {
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

    println!("return value: {result:?}: {}", result.to_hresult().message());

    if result.is_ok() {
        Ok(())
    } else {
        Err(result.to_hresult().message())
    }
}