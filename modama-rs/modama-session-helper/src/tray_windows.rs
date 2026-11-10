//! Windows notification-area icon: `Shell_NotifyIconW`, identified by `hWnd` + `uID`. A GUID
//! would bind the icon to the executable's path, so `NIM_ADD` would fail after a move unless both
//! binaries are signed by the same publisher.
//!
//! A dedicated thread owns a hidden top-level window (a message-only window doesn't receive the
//! `TaskbarCreated` broadcast that tells us Explorer restarted), its message loop, the icon and
//! the menu. Other threads only replace the state and post messages to the window.

use std::{
    ffi::c_void,
    sync::{
        Arc, Mutex,
        atomic::{AtomicIsize, Ordering},
    },
    thread::{self, JoinHandle},
};

use log::{info, warn};
use tokio::sync::{mpsc, oneshot};
use windows::{
    Win32::{
        Foundation::{ERROR_CLASS_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, WPARAM},
        UI::{
            Shell::{
                NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
                NIM_SETVERSION, NIN_SELECT, NINF_KEY, NOTIFYICON_VERSION_4, NOTIFYICONDATAW,
                NOTIFYICONDATAW_0, Shell_NotifyIconW,
            },
            WindowsAndMessaging::{
                AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW,
                DefWindowProcW, DestroyIcon, DestroyMenu, DestroyWindow, DispatchMessageW,
                GetMessageW, GetSystemMetrics, HICON, LR_DEFAULTCOLOR, MF_GRAYED, MF_SEPARATOR,
                MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassW,
                RegisterWindowMessageW, SM_CXSMICON, SetForegroundWindow, TPM_NONOTIFY,
                TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WINDOW_EX_STYLE,
                WINDOW_STYLE, WM_APP, WM_CLOSE, WM_CONTEXTMENU, WM_DESTROY, WM_NULL, WNDCLASSW,
            },
        },
    },
    core::{HSTRING, PCWSTR, w},
};

use crate::tray::{TrayAction, TrayState, ico_image};

const ICON: &[u8] = include_bytes!("../assets/tray-icon.ico");

/// The shell's messages about our icon
const TRAY_CALLBACK: u32 = WM_APP + 1;
/// The state changed; update the icon
const REFRESH: u32 = WM_APP + 2;
const ICON_ID: u32 = 1;
const CLASS_NAME: PCWSTR = w!("ModamaSessionHelperTray");
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;

const MENU_OPEN_MODAMA: usize = 1;
const MENU_OPEN_DRIVE: usize = 2;

/// Shared between the tray thread and the rest of the helper
struct Shared {
    state: Mutex<TrayState>,
    actions: mpsc::UnboundedSender<TrayAction>,
    /// The hidden window, 0 while there is none
    window: AtomicIsize,
}

/// Only ever touched on the tray thread
struct ThreadContext {
    shared: Arc<Shared>,
    taskbar_created: u32,
    icon: HICON,
    /// Whether the shell currently has our icon
    installed: bool,
}

// the initializer is const already; the pinned toolchain's clippy reports it anyway
#[allow(clippy::missing_const_for_thread_local)]
mod context {
    use std::cell::RefCell;

    use super::ThreadContext;

    thread_local! {
        pub static CONTEXT: RefCell<Option<ThreadContext>> = const { RefCell::new(None) };
    }
}
use context::CONTEXT;

/// Run `f` on the tray thread's context. Window procedures can be re-entered (the menu runs a
/// modal message loop), so this never holds the borrow across Win32 calls that dispatch
/// messages, and skips instead of panicking if it is already borrowed.
fn with_context<T>(f: impl FnOnce(&mut ThreadContext) -> T) -> Option<T> {
    CONTEXT.with(|context| context.try_borrow_mut().ok()?.as_mut().map(f))
}

pub struct TrayItem {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

fn post(shared: &Shared, message: u32) {
    let window = shared.window.load(Ordering::Acquire);
    if window == 0 {
        return;
    }
    // SAFETY: plain message posting; if the window was destroyed in the meantime, the call
    // fails, which is fine
    // https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-postmessagew
    let _ = unsafe {
        PostMessageW(
            Some(HWND(window as *mut c_void)),
            message,
            WPARAM(0),
            LPARAM(0),
        )
    };
}

/// Create the window and the icon on their own thread. Fails only if the window can't be
/// created; an icon the shell doesn't take (no Explorer yet) is retried on `TaskbarCreated`.
pub async fn start(
    state: TrayState,
    actions: mpsc::UnboundedSender<TrayAction>,
) -> Result<TrayItem, String> {
    let shared = Arc::new(Shared {
        state: Mutex::new(state),
        actions,
        window: AtomicIsize::new(0),
    });
    let (started, started_rx) = oneshot::channel();
    let thread_shared = Arc::clone(&shared);
    let thread = thread::Builder::new()
        .name("tray".to_owned())
        .spawn(move || {
            match initialize(&thread_shared) {
                Ok(()) => {
                    let _ = started.send(Ok(()));
                    message_loop();
                }
                Err(e) => {
                    let _ = started.send(Err(e.to_string()));
                }
            }
            thread_shared.window.store(0, Ordering::Release);
            if let Some(context) = CONTEXT.with(|context| context.replace(None))
                && !context.icon.is_invalid()
            {
                // SAFETY: created by `load_icon` on this thread, no longer shown
                let _ = unsafe { DestroyIcon(context.icon) };
            }
        })
        .map_err(|e| format!("could not start the tray thread: {e}"))?;

    match started_rx.await {
        Ok(Ok(())) => Ok(TrayItem {
            shared,
            thread: Some(thread),
        }),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => {
            let _ = thread.join();
            Err("the tray thread stopped during startup".to_owned())
        }
    }
}

impl TrayItem {
    pub async fn update(&self, state: TrayState) {
        if let Ok(mut current) = self.shared.state.lock() {
            *current = state;
        }
        post(&self.shared, REFRESH);
    }
}

impl Drop for TrayItem {
    fn drop(&mut self) {
        // the window removes the icon when it is destroyed, then the loop ends
        post(&self.shared, WM_CLOSE);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Our icon at the size the shell shows small icons in
fn load_icon() -> windows::core::Result<HICON> {
    // SAFETY: no pointers involved
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) };
    let image = ico_image(ICON, u32::try_from(size).unwrap_or(16))
        .ok_or_else(windows::core::Error::empty)?;
    // SAFETY: `image` is one complete image (DIB or PNG) from our .ico, read during the call
    // only; 0x00030000 is the format version the documentation requires
    // https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-createiconfromresourceex
    unsafe { CreateIconFromResourceEx(image, true, 0x0003_0000, size, size, LR_DEFAULTCOLOR) }
}

fn initialize(shared: &Arc<Shared>) -> windows::core::Result<()> {
    // SAFETY: a static NUL-terminated name
    // https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-registerwindowmessagew
    let taskbar_created = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
    if taskbar_created == 0 {
        return Err(windows::core::Error::from_thread());
    }
    let icon = load_icon().unwrap_or_else(|e| {
        warn!("could not load the tray icon: {e}");
        HICON::default()
    });
    CONTEXT.with(|context| {
        context.replace(Some(ThreadContext {
            shared: Arc::clone(shared),
            taskbar_created,
            icon,
            installed: false,
        }))
    });

    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    // SAFETY: the class struct and its static name are valid during the call
    // https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-registerclassw
    if unsafe { RegisterClassW(&raw const class) } == 0 {
        // SAFETY: read right after the failed call
        if unsafe { GetLastError() } != ERROR_CLASS_ALREADY_EXISTS {
            return Err(windows::core::Error::from_thread());
        }
    }
    // SAFETY: the class is registered; no parent makes it a hidden top-level window, which
    // receives the TaskbarCreated broadcast
    // https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-createwindowexw
    let window = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            CLASS_NAME,
            w!("Modama"),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            None,
            None,
            None,
            None,
        )?
    };
    shared.window.store(window.0 as isize, Ordering::Release);
    add_icon(window);
    Ok(())
}

fn message_loop() {
    let mut message = MSG::default();
    loop {
        // SAFETY: `message` is a valid buffer owned by this thread
        // https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-getmessagew
        let result = unsafe { GetMessageW(&raw mut message, None, 0, 0) };
        match result.0 {
            0 => break,
            -1 => {
                warn!(
                    "the tray's message loop failed: {}",
                    windows::core::Error::from_thread()
                );
                break;
            }
            _ => {
                // SAFETY: a message just filled in by GetMessageW
                unsafe {
                    let _ = TranslateMessage(&raw const message);
                    DispatchMessageW(&raw const message);
                }
            }
        }
    }
}

/// UTF-16 into a fixed, NUL-terminated buffer, cut off if needed
fn copy_wide<const N: usize>(target: &mut [u16; N], value: &str) {
    target.fill(0);
    for (slot, unit) in target
        .iter_mut()
        .take(N.saturating_sub(1))
        .zip(value.encode_utf16())
    {
        *slot = unit;
    }
}

fn notify_data(window: HWND, context: &ThreadContext) -> NOTIFYICONDATAW {
    let label = context
        .shared
        .state
        .lock()
        .map(|state| state.label())
        .unwrap_or_default();
    let mut data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: ICON_ID,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP,
        uCallbackMessage: TRAY_CALLBACK,
        hIcon: context.icon,
        ..Default::default()
    };
    copy_wide(&mut data.szTip, &format!("modama-agent: {label}"));
    data
}

/// Add the icon and switch to the version-4 behavior; on failure, the icon stays absent until
/// the next refresh or `TaskbarCreated`
fn add_icon(window: HWND) {
    let Some(mut data) = with_context(|context| notify_data(window, context)) else {
        return;
    };
    // SAFETY: `data` is fully initialized and read during the call only
    // https://learn.microsoft.com/windows/win32/api/shellapi/nf-shellapi-shell_notifyiconw
    let installed = unsafe { Shell_NotifyIconW(NIM_ADD, &raw const data) }.as_bool() && {
        data.Anonymous = NOTIFYICONDATAW_0 {
            uVersion: NOTIFYICON_VERSION_4,
        };
        // SAFETY: as above, for the icon just added
        let versioned = unsafe { Shell_NotifyIconW(NIM_SETVERSION, &raw const data) }.as_bool();
        if !versioned {
            // SAFETY: as above; an icon with the old behavior would misread our messages
            let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &raw const data) };
        }
        versioned
    };
    if !installed {
        info!("the notification area doesn't take the icon (yet); retrying when it is there");
    }
    with_context(|context| context.installed = installed);
}

fn refresh_icon(window: HWND) {
    match with_context(|context| context.installed.then(|| notify_data(window, context))) {
        Some(Some(data)) => {
            // SAFETY: as in `add_icon`, for our installed icon
            let _ = unsafe { Shell_NotifyIconW(NIM_MODIFY, &raw const data) };
        }
        Some(None) => add_icon(window),
        None => {}
    }
}

fn delete_icon(window: HWND) {
    let installed = with_context(|context| std::mem::replace(&mut context.installed, false));
    if installed != Some(true) {
        return;
    }
    let data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: ICON_ID,
        ..Default::default()
    };
    // SAFETY: as in `add_icon`; hWnd + uID identify our icon
    let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &raw const data) };
}

fn request(action: TrayAction) {
    with_context(|context| {
        // the receiver only goes away with the helper
        let _ = context.shared.actions.send(action);
    });
}

/// The context menu at `anchor` (from the version-4 callback's wParam)
fn show_menu(window: HWND, anchor: WPARAM) {
    let Some((label, has_drive)) = with_context(|context| {
        context
            .shared
            .state
            .lock()
            .map(|state| (state.label(), state.network_drive.is_some()))
            .unwrap_or_default()
    }) else {
        return;
    };
    // SAFETY: no pointers involved; the menu is destroyed below
    // https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-createpopupmenu
    let Ok(menu) = (unsafe { CreatePopupMenu() }) else {
        return;
    };
    let label = HSTRING::from(label);
    let drive_flags = if has_drive {
        MF_STRING
    } else {
        MF_STRING | MF_GRAYED
    };
    // the anchor's coordinates are signed (multi-monitor setups have negative ones)
    let x = i32::from((anchor.0 & 0xffff) as u16 as i16);
    let y = i32::from(((anchor.0 >> 16) & 0xffff) as u16 as i16);
    // SAFETY: the menu and window are ours and valid; the strings outlive the calls, which copy
    // them. TrackPopupMenu runs a modal loop that dispatches messages to our window, so no
    // context borrow is held here.
    // https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-trackpopupmenu
    let selected = unsafe {
        let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, &label);
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, MENU_OPEN_MODAMA, w!("Open modama-agent"));
        let _ = AppendMenuW(menu, drive_flags, MENU_OPEN_DRIVE, w!("Open ingest share"));
        // without this, the menu doesn't close when the user clicks elsewhere
        let _ = SetForegroundWindow(window);
        let selected = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
            x,
            y,
            None,
            window,
            None,
        );
        // the documented companion of SetForegroundWindow above
        let _ = PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        selected
    };
    match usize::try_from(selected.0) {
        Ok(MENU_OPEN_MODAMA) => request(TrayAction::OpenModama),
        Ok(MENU_OPEN_DRIVE) => request(TrayAction::OpenNetworkDrive),
        _ => {}
    }
}

/// # Safety
/// Called by Windows for our window class only, on the tray thread.
unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if with_context(|context| context.taskbar_created) == Some(message) {
        // Explorer (re)started: whatever we had is gone
        with_context(|context| context.installed = false);
        add_icon(window);
        return LRESULT(0);
    }
    match message {
        REFRESH => {
            refresh_icon(window);
            LRESULT(0)
        }
        // version 4: the event is in the low word of lParam
        TRAY_CALLBACK => {
            match (lparam.0 as u32) & 0xffff {
                NIN_SELECT | NIN_KEYSELECT => request(TrayAction::OpenModama),
                WM_CONTEXTMENU => show_menu(window, wparam),
                _ => {}
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            // SAFETY: our own window, on its thread
            let _ = unsafe { DestroyWindow(window) };
            LRESULT(0)
        }
        WM_DESTROY => {
            delete_icon(window);
            // SAFETY: ends this thread's message loop
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        // SAFETY: unhandled messages go to the default procedure unchanged
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}
