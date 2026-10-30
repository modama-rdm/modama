//! Windows shell folders (program files, app data, ...)

use std::{ffi::OsString, os::windows::ffi::OsStringExt as _, path::PathBuf};

use windows::Win32::UI::Shell::{SHGFP_TYPE_CURRENT, SHGetFolderPathW};

/// The folder for a `CSIDL_*` value, such as `CSIDL_LOCAL_APPDATA`
pub fn known_folder(csidl: u32) -> windows::core::Result<PathBuf> {
    let mut path = [0u16; 260];
    // SAFETY: `path` is a live, writable MAX_PATH-sized UTF-16 buffer for the
    // duration of the call. `hwnd` and `hToken` are allowed to be NULL, and the
    // supplied CSIDL values identify real filesystem folders. On success the
    // API writes a NUL-terminated path without retaining the buffer pointer.
    // SHGetFolderPathW is used instead of SHGetKnownFolderPath so the resulting
    // binary does not import CoTaskMemFree from post-Windows-7 combase.dll.
    // https://learn.microsoft.com/windows/win32/api/shlobj_core/nf-shlobj_core-shgetfolderpathw
    unsafe {
        SHGetFolderPathW(
            None,
            csidl as i32,
            None,
            SHGFP_TYPE_CURRENT.0 as u32,
            &mut path,
        )?;
    }
    let length = path
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(path.len());
    Ok(PathBuf::from(OsString::from_wide(&path[..length])))
}
