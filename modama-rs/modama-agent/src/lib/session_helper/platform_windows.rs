//! Windows mount backend: WNet drive mappings in the helper's (non-elevated) logon session.
//! The pass logic is in `wnet_reconcile`; this is the FFI underneath it.

use std::{
    collections::BTreeSet,
    ffi::c_void,
    sync::{Arc, Mutex},
};

use log::{debug, warn};
use modama_common::{internal_api_types::SessionHelperConfig, messages::Share};
use widestring::U16CString;
use windows::{
    Win32::{
        Foundation::{
            ERROR_ACCOUNT_DISABLED, ERROR_ACCOUNT_LOCKED_OUT, ERROR_ACCOUNT_RESTRICTION,
            ERROR_ALREADY_ASSIGNED, ERROR_DEVICE_ALREADY_REMEMBERED, ERROR_INVALID_LOGON_HOURS,
            ERROR_INVALID_PASSWORD, ERROR_INVALID_WORKSTATION, ERROR_LOGON_FAILURE,
            ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_PASSWORD_EXPIRED,
            ERROR_SESSION_CREDENTIAL_CONFLICT, HANDLE, NO_ERROR, WIN32_ERROR,
        },
        NetworkManagement::{
            NetManagement::{
                MAX_PREFERRED_LENGTH, NERR_Success, NetApiBufferFree, NetUseEnum, USE_INFO_1,
            },
            WNet::{
                CONNECT_TEMPORARY, NET_CONNECT_FLAGS, NETRESOURCEW, RESOURCE_CONNECTED,
                RESOURCETYPE_ANY, RESOURCETYPE_DISK, WNET_OPEN_ENUM_USAGE, WNetAddConnection2W,
                WNetCancelConnection2W, WNetCloseEnum, WNetEnumResourceW, WNetOpenEnumW,
            },
        },
        Storage::FileSystem::GetLogicalDrives,
    },
    core::{PCWSTR, PWSTR},
};

use crate::session_helper::{
    mount_backend::{EnumerationError, MountBackend, PassResult},
    wnet_reconcile::{AddError, Connection, WNetApi, reconcile_pass},
};

fn win32_message(error: WIN32_ERROR) -> String {
    format!(
        "{} (error {})",
        windows::core::Error::from_hresult(error.to_hresult()).message(),
        error.0
    )
}

/// # Safety
/// `p` must be null or point to a NUL-terminated UTF-16 string
unsafe fn read_pwstr(p: PWSTR) -> Option<String> {
    if p.is_null() {
        return None;
    }
    // SAFETY: non-null and NUL-terminated per the caller's contract
    let s = unsafe { p.to_string() }
        .inspect_err(|e| warn!("invalid UTF-16 in a connection name: {e}"))
        .ok()?;
    (!s.is_empty()).then_some(s)
}

/// All connections of this logon session, including deviceless ones and `IPC$`
/// (`NetUseEnum`, level 1)
fn net_use_enum() -> Result<Vec<Connection>, String> {
    let mut connections = Vec::new();
    let mut resume = 0u32;
    loop {
        let mut buffer: *mut u8 = std::ptr::null_mut();
        let mut read = 0u32;
        let mut total = 0u32;
        // SAFETY: all out-pointers are live locals; on success `buffer` points to `read`
        // `USE_INFO_1` entries allocated by the API, which we free below.
        // https://learn.microsoft.com/windows/win32/api/lmuse/nf-lmuse-netuseenum
        let status = unsafe {
            NetUseEnum(
                PCWSTR::null(),
                1,
                Some(&raw mut buffer),
                MAX_PREFERRED_LENGTH,
                Some(&raw mut read),
                &raw mut total,
                Some(&raw mut resume),
            )
        };
        if status != NERR_Success && status != ERROR_MORE_DATA.0 {
            return Err(format!(
                "NetUseEnum failed: {}",
                win32_message(WIN32_ERROR(status))
            ));
        }
        if !buffer.is_null() {
            // SAFETY: the API returned `read` contiguous, aligned `USE_INFO_1` entries, whose
            // strings live in the same buffer; we copy them out before freeing it
            let entries =
                unsafe { std::slice::from_raw_parts(buffer.cast::<USE_INFO_1>(), read as usize) };
            for entry in entries {
                // SAFETY: the strings are NUL-terminated or null, inside the API's buffer
                let (local, remote) =
                    unsafe { (read_pwstr(entry.ui1_local), read_pwstr(entry.ui1_remote)) };
                if let Some(remote) = remote {
                    connections.push(Connection { local, remote });
                }
            }
            // SAFETY: allocated by NetUseEnum, not used afterwards
            unsafe { NetApiBufferFree(Some(buffer.cast::<c_void>().cast_const())) };
        }
        if status == NERR_Success {
            return Ok(connections);
        }
    }
}

/// Connected resources of this logon session, as WNet sees them (`WNetOpenEnumW`); only used
/// to cross-check `net_use_enum`
fn wnet_enum() -> Result<Vec<Connection>, String> {
    struct EnumHandle(HANDLE);
    impl Drop for EnumHandle {
        fn drop(&mut self) {
            // SAFETY: a handle from WNetOpenEnumW, closed exactly once
            let result = unsafe { WNetCloseEnum(self.0) };
            if result != NO_ERROR {
                warn!("WNetCloseEnum failed: {}", win32_message(result));
            }
        }
    }

    let mut handle = HANDLE::default();
    // SAFETY: `handle` is a live out-pointer; no NETRESOURCE is needed for RESOURCE_CONNECTED
    // https://learn.microsoft.com/windows/win32/api/winnetwk/nf-winnetwk-wnetopenenumw
    let result = unsafe {
        WNetOpenEnumW(
            RESOURCE_CONNECTED,
            RESOURCETYPE_ANY,
            WNET_OPEN_ENUM_USAGE(0),
            None,
            &raw mut handle,
        )
    };
    if result != NO_ERROR {
        return Err(format!("WNetOpenEnumW failed: {}", win32_message(result)));
    }
    let handle = EnumHandle(handle);

    let mut connections = Vec::new();
    // u64 elements: the buffer must be aligned for NETRESOURCEW
    let mut buffer = vec![0u64; 2048];
    loop {
        let mut count = u32::MAX; // as many entries as fit
        let mut size = u32::try_from(buffer.len() * size_of::<u64>()).unwrap_or(u32::MAX);
        // SAFETY: `buffer` is writable for `size` bytes; count and size are live locals
        // https://learn.microsoft.com/windows/win32/api/winnetwk/nf-winnetwk-wnetenumresourcew
        let result = unsafe {
            WNetEnumResourceW(
                handle.0,
                &raw mut count,
                buffer.as_mut_ptr().cast::<c_void>(),
                &raw mut size,
            )
        };
        if result == ERROR_NO_MORE_ITEMS {
            return Ok(connections);
        }
        if result == ERROR_MORE_DATA {
            // `size` now holds the size needed for one entry
            buffer.resize((size as usize).div_ceil(size_of::<u64>()), 0);
            continue;
        }
        if result != NO_ERROR {
            return Err(format!(
                "WNetEnumResourceW failed: {}",
                win32_message(result)
            ));
        }
        // SAFETY: on success, the buffer starts with `count` NETRESOURCEW entries whose
        // strings live further back in the same buffer; we copy them out before reusing it
        let entries = unsafe {
            std::slice::from_raw_parts(buffer.as_ptr().cast::<NETRESOURCEW>(), count as usize)
        };
        for entry in entries {
            // SAFETY: NUL-terminated or null, inside `buffer`
            let (local, remote) = unsafe {
                (
                    read_pwstr(entry.lpLocalName),
                    read_pwstr(entry.lpRemoteName),
                )
            };
            if let Some(remote) = remote {
                connections.push(Connection { local, remote });
            }
        }
    }
}

fn normalized(connections: &[Connection]) -> BTreeSet<(String, String)> {
    connections
        .iter()
        .map(|c| {
            (
                c.local.as_deref().unwrap_or("").to_uppercase(),
                c.remote.to_lowercase(),
            )
        })
        .collect()
}

/// The real WNet API
#[derive(Default)]
struct NativeWNet {
    /// The last cross-check mismatch we warned about, so it isn't repeated every pass
    last_mismatch: Mutex<Option<String>>,
}

impl NativeWNet {
    /// Warn if `WNetOpenEnumW` disagrees with `NetUseEnum`. It isn't verified yet which of the
    /// two lists deviceless connections completely; `NetUseEnum` is documented to.
    fn cross_check(&self, canonical: &[Connection]) {
        let mismatch = match wnet_enum() {
            Ok(wnet) => {
                debug!("connections: NetUseEnum {canonical:?}, WNetOpenEnumW {wnet:?}");
                let (net_use, wnet) = (normalized(canonical), normalized(&wnet));
                (net_use != wnet).then(|| {
                    format!(
                        "only in NetUseEnum: {:?}; only in WNetOpenEnumW: {:?}",
                        net_use.difference(&wnet).collect::<Vec<_>>(),
                        wnet.difference(&net_use).collect::<Vec<_>>(),
                    )
                })
            }
            Err(e) => Some(e),
        };
        let Ok(mut last) = self.last_mismatch.lock() else {
            return;
        };
        if let Some(mismatch) = &mismatch
            && last.as_ref() != Some(mismatch)
        {
            warn!("connection enumerations disagree: {mismatch}");
        }
        *last = mismatch;
    }
}

impl WNetApi for NativeWNet {
    fn connections(&self) -> Result<Vec<Connection>, String> {
        let connections = net_use_enum()?;
        self.cross_check(&connections);
        Ok(connections)
    }

    fn used_drives(&self) -> u32 {
        // SAFETY: no arguments
        unsafe { GetLogicalDrives() }
    }

    fn add(&self, drive: &str, remote: &str, user: &str, password: &str) -> Result<(), AddError> {
        let wide = |s: &str| {
            U16CString::from_str(s).map_err(|_| AddError::Other("contains a NUL character".into()))
        };
        let (mut local, mut remote_name) = (wide(drive)?, wide(remote)?);
        let (user, password) = (wide(user)?, wide(password)?);
        let resource = NETRESOURCEW {
            dwType: RESOURCETYPE_DISK,
            lpLocalName: PWSTR::from_raw(local.as_mut_ptr()),
            lpRemoteName: PWSTR::from_raw(remote_name.as_mut_ptr()),
            ..Default::default()
        };
        // SAFETY: all strings are NUL-terminated and outlive the call, which doesn't keep them.
        // CONNECT_TEMPORARY: not remembered in the user profile, no saved credentials.
        // https://learn.microsoft.com/windows/win32/api/winnetwk/nf-winnetwk-wnetaddconnection2w
        let result = unsafe {
            WNetAddConnection2W(
                &raw const resource,
                PCWSTR::from_raw(password.as_ptr()),
                PCWSTR::from_raw(user.as_ptr()),
                CONNECT_TEMPORARY,
            )
        };
        match result {
            NO_ERROR => Ok(()),
            ERROR_ALREADY_ASSIGNED | ERROR_DEVICE_ALREADY_REMEMBERED => Err(AddError::LetterTaken),
            ERROR_SESSION_CREDENTIAL_CONFLICT => Err(AddError::CredentialConflict),
            ERROR_LOGON_FAILURE
            | ERROR_INVALID_PASSWORD
            | ERROR_ACCOUNT_RESTRICTION
            | ERROR_INVALID_LOGON_HOURS
            | ERROR_INVALID_WORKSTATION
            | ERROR_PASSWORD_EXPIRED
            | ERROR_ACCOUNT_DISABLED
            | ERROR_ACCOUNT_LOCKED_OUT => Err(AddError::AuthenticationFailed),
            other => Err(AddError::Other(win32_message(other))),
        }
    }

    fn cancel(&self, name: &str, force: bool) -> Result<(), String> {
        let name = U16CString::from_str(name).map_err(|e| e.to_string())?;
        // SAFETY: NUL-terminated, outlives the call. Flags 0: never touch the user profile.
        // Unless forced, open files keep the connection (forcing is the GUI's decision).
        // https://learn.microsoft.com/windows/win32/api/winnetwk/nf-winnetwk-wnetcancelconnection2w
        let result = unsafe {
            WNetCancelConnection2W(PCWSTR::from_raw(name.as_ptr()), NET_CONNECT_FLAGS(0), force)
        };
        if result == NO_ERROR {
            Ok(())
        } else {
            Err(win32_message(result))
        }
    }
}

pub struct CurrentPlatformMountBackend {
    api: Arc<NativeWNet>,
}

impl CurrentPlatformMountBackend {
    pub async fn new() -> Result<Self, EnumerationError> {
        Ok(Self {
            api: Arc::default(),
        })
    }
}

impl MountBackend for CurrentPlatformMountBackend {
    async fn reconcile(
        &self,
        desired_shares: &[Share],
        config: &SessionHelperConfig,
        force: bool,
    ) -> Result<PassResult, EnumerationError> {
        // WNet calls block, possibly for as long as the server takes to answer
        let (api, shares, config) = (
            Arc::clone(&self.api),
            desired_shares.to_vec(),
            config.clone(),
        );
        tokio::task::spawn_blocking(move || reconcile_pass(api.as_ref(), &shares, &config, force))
            .await
            .map_err(|e| EnumerationError(format!("the reconciliation task failed: {e}")))?
    }
}
