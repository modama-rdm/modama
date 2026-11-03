//! Per-monitor DPI awareness for the session helper process.
//!
//! Unaware, the helper is told sizes at 96 DPI and its output is scaled up: the tray gets the
//! 16 px icon stretched (blurry), and the physical anchor the shell passes for the menu is scaled
//! once more, so the menu opens far from the pointer. The GUI gets awareness from winit; the
//! helper has no winit window. `SetProcessDpiAwarenessContext` is Windows 10 1703 and later, so
//! it is looked up at run time: an import would keep the binary from starting on Windows 7, which
//! stays unaware.

use log::{info, warn};
use windows::{
    Win32::{
        System::LibraryLoader::{GetModuleHandleW, GetProcAddress},
        UI::HiDpi::{DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2},
    },
    core::{BOOL, s, w},
};

/// Make the process per-monitor DPI aware; call before creating any window.
pub(crate) fn enable_per_monitor_dpi_awareness() {
    type SetProcessDpiAwarenessContext = unsafe extern "system" fn(DPI_AWARENESS_CONTEXT) -> BOOL;

    // SAFETY: reads only the static NUL-terminated name. The returned handle is not
    // reference-counted and becomes invalid if the module is freed
    // (https://learn.microsoft.com/en-us/windows/win32/api/libloaderapi/nf-libloaderapi-getmodulehandlew).
    // user32.dll can't be: it is in this executable's import table (window, menu and tray calls),
    // so the loader maps it before `main` and holds that reference until the process ends.
    let user32 = unsafe { GetModuleHandleW(w!("user32.dll")) };
    let address = user32.ok().and_then(|user32| {
        // SAFETY: a module handle of this process (above) and a static NUL-terminated narrow
        // name, the LPCSTR that GetProcAddress takes; it returns NULL (None, handled below) if
        // the function is missing
        // (https://learn.microsoft.com/en-us/windows/win32/api/libloaderapi/nf-libloaderapi-getprocaddress).
        unsafe { GetProcAddress(user32, s!("SetProcessDpiAwarenessContext")) }
    });
    let Some(address) = address else {
        info!("this Windows has no per-monitor DPI awareness; the tray icon may be scaled");
        return;
    };
    // SAFETY: transmutes one function pointer into another of the same size. The address is
    // user32's SetProcessDpiAwarenessContext, documented as
    // `BOOL WINAPI SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT value)`
    // (https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setprocessdpiawarenesscontext).
    // WINAPI is `extern "system"`; `DPI_AWARENESS_CONTEXT` (a handle) and `BOOL` (i32) are
    // repr(transparent) over those C types. So the alias is the function's real signature.
    let set_context = unsafe {
        std::mem::transmute::<unsafe extern "system" fn() -> isize, SetProcessDpiAwarenessContext>(
            address,
        )
    };
    // SAFETY: the argument is a predefined awareness context
    // (https://learn.microsoft.com/en-us/windows/win32/hidpi/dpi-awareness-context; V2 exists
    // wherever this function does, both since Windows 10 1703). It carries no pointer the
    // function could dereference. There are no other preconditions: an invalid argument, a second
    // call or a call after UI exists fails with FALSE (ERROR_INVALID_PARAMETER /
    // ERROR_ACCESS_DENIED, handled below), not with undefined behaviour. For correct scaling,
    // though not for safety, it must come before any UI: we run before the tokio runtime and the
    // tray window, and popups are separate processes. Logging, the elevation check and the
    // singleton mutex create no UI.
    if !unsafe { set_context(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }.as_bool() {
        warn!(
            "could not make the session helper DPI aware: {}",
            windows::core::Error::from_thread()
        );
    }
}
