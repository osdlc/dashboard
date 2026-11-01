//! Windows: the wallpaper's WorkerW parenting, the screensaver preview box,
//! and the `/c` message box.
//!
//! Wallpaper: Explorer draws the desktop picture in a `WorkerW` window behind
//! the one that holds the desktop icons (`SHELLDLL_DefView`). Sending Progman
//! the undocumented message 0x052C makes Explorer create that WorkerW; a window
//! made its child shows on the desktop, behind the icons. This is what
//! wallpaper apps (Lively, Wallpaper Engine) do. On Windows 11 24H2 and later
//! the WorkerW is a child of Progman instead of a top-level sibling; both are
//! looked for.

use std::ffi::c_void;

use tauri::{AppHandle, Monitor, WebviewWindow, WebviewWindowBuilder, Wry};
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, GetClientRect, GetSystemMetrics, IsWindow,
    MB_ICONINFORMATION, MB_OK, MessageBoxW, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SMTO_NORMAL,
    SWP_NOACTIVATE, SWP_NOZORDER, SendMessageTimeoutW, SetParent, SetWindowPos,
};
use windows::core::{BOOL, HSTRING, PCWSTR, w};

/// Parent `window` to the desktop's WorkerW and place it over `monitor`.
pub fn pin_to_desktop(window: &WebviewWindow, monitor: &Monitor) -> tauri::Result<()> {
    let Some(worker) = desktop_worker() else {
        tracing::warn!("no desktop WorkerW found; the wallpaper stays a normal bottom window");
        return Ok(());
    };
    let hwnd = window.hwnd()?;
    // SAFETY: both handles are live windows; the calls only change parent and position.
    unsafe {
        let _ = SetParent(hwnd, Some(worker));
        // WorkerW covers the whole virtual screen, whose origin can be negative.
        let x = monitor.position().x - GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = monitor.position().y - GetSystemMetrics(SM_YVIRTUALSCREEN);
        let size = monitor.size();
        let _ = SetWindowPos(
            hwnd,
            None,
            x,
            y,
            size.width as i32,
            size.height as i32,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    Ok(())
}

fn desktop_worker() -> Option<HWND> {
    // SAFETY: plain Win32 window lookups; `found` outlives EnumWindows.
    unsafe {
        let progman = FindWindowW(w!("Progman"), PCWSTR::null()).ok()?;
        let mut result = 0usize;
        let _ = SendMessageTimeoutW(
            progman,
            0x052C,
            WPARAM(0xD),
            LPARAM(0x1),
            SMTO_NORMAL,
            1000,
            Some(&mut result),
        );
        let mut found = HWND::default();
        let _ = EnumWindows(Some(find_worker), LPARAM(&mut found as *mut HWND as isize));
        if !found.is_invalid() {
            return Some(found);
        }
        // Windows 11 24H2+: the WorkerW is a child of Progman.
        FindWindowExW(Some(progman), None, w!("WorkerW"), PCWSTR::null()).ok()
    }
}

/// For the top-level window that holds the icons, the WorkerW after it is the
/// one behind them.
unsafe extern "system" fn find_worker(top: HWND, out: LPARAM) -> BOOL {
    // SAFETY: called by EnumWindows with the pointer passed in desktop_worker.
    unsafe {
        if FindWindowExW(Some(top), None, w!("SHELLDLL_DefView"), PCWSTR::null()).is_ok()
            && let Ok(worker) = FindWindowExW(None, Some(top), w!("WorkerW"), PCWSTR::null())
        {
            *(out.0 as *mut HWND) = worker;
            return false.into();
        }
    }
    true.into()
}

/// Show the screensaver inside the preview box `parent`, and quit when the
/// box goes away (the user picked another screensaver or closed the dialog).
pub fn preview(
    app: &AppHandle,
    builder: WebviewWindowBuilder<'_, Wry, AppHandle>,
    parent: isize,
) -> tauri::Result<()> {
    let parent = HWND(parent as *mut c_void);
    let mut rect = RECT::default();
    // SAFETY: GetClientRect only reads the window's size.
    unsafe {
        let _ = GetClientRect(parent, &mut rect);
    }
    let window = builder.parent_raw(parent).build()?;
    window.set_position(tauri::PhysicalPosition::new(0, 0))?;
    window.set_size(tauri::PhysicalSize::new(
        (rect.right - rect.left) as u32,
        (rect.bottom - rect.top) as u32,
    ))?;
    let app = app.clone();
    let parent = parent.0 as isize;
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));
            // SAFETY: IsWindow accepts any handle value.
            if !unsafe { IsWindow(Some(HWND(parent as *mut c_void))) }.as_bool() {
                app.exit(0);
                return;
            }
        }
    });
    Ok(())
}

pub fn message_box(title: &str, text: &str) {
    // SAFETY: both strings live until the call returns.
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(text),
            &HSTRING::from(title),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}
