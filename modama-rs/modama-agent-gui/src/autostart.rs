//! Per-user autostart of the session helper on Windows: an HKCU Run entry that starts this
//! executable as `session-helper` (it has no console window). Behind the `autostart` feature
//! until there is a proper installer.

use std::{ffi::c_void, path::Path};

use windows::{
    Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_DWORD, REG_SZ, RRF_RT_ANY, RegGetValueW, RegSetKeyValueW,
    },
    core::{PCWSTR, w},
};

const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const RUN_VALUE: PCWSTR = w!("ModamaSessionHelper");
const GUI_KEY: PCWSTR = w!(r"Software\modama\modama-agent-gui");
const DISMISSED_VALUE: PCWSTR = w!("AutostartOfferDismissed");

fn value_exists(key: PCWSTR, value: PCWSTR) -> bool {
    // SAFETY: key and value are NUL-terminated static strings; with no type or data buffers,
    // the call only checks for existence and writes nothing.
    // https://learn.microsoft.com/windows/win32/api/winreg/nf-winreg-reggetvaluew
    unsafe { RegGetValueW(HKEY_CURRENT_USER, key, value, RRF_RT_ANY, None, None, None) }.is_ok()
}

fn set_value(key: PCWSTR, value: PCWSTR, kind: u32, data: &[u8]) -> windows::core::Result<()> {
    let len = u32::try_from(data.len()).map_err(|_| windows::core::Error::empty())?;
    // SAFETY: key and value are NUL-terminated; `data` is live and `len` bytes long for the
    // duration of the call, which copies it. The key is created if it doesn't exist.
    // https://learn.microsoft.com/windows/win32/api/winreg/nf-winreg-regsetkeyvaluew
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            key,
            value,
            kind,
            Some(data.as_ptr().cast::<c_void>()),
            len,
        )
    }
    .ok()
}

/// Is there an autostart entry already (ours, possibly from another install location)?
pub fn entry_exists() -> bool {
    value_exists(RUN_KEY, RUN_VALUE)
}

pub fn dismissed() -> bool {
    value_exists(GUI_KEY, DISMISSED_VALUE)
}

pub fn dismiss() -> windows::core::Result<()> {
    set_value(GUI_KEY, DISMISSED_VALUE, REG_DWORD.0, &1u32.to_le_bytes())
}

pub fn enable(gui_exe: &Path, agent_url: &str) -> windows::core::Result<()> {
    let command = format!(
        "\"{}\" --agent-url \"{agent_url}\" session-helper",
        gui_exe.display()
    );
    // REG_SZ data includes the terminating NUL:
    let bytes: Vec<u8> = command
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    set_value(RUN_KEY, RUN_VALUE, REG_SZ.0, &bytes)
}
