//! macOS window levels: a wallpaper window goes at the desktop window level,
//! the level of the desktop picture, so it sits behind the desktop icons and
//! every app; a screensaver window goes at the screensaver level, above the
//! menu bar and the Dock.

use objc2::msg_send;
use objc2::runtime::AnyObject;
use tauri::WebviewWindow;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGWindowLevelForKey(key: i32) -> i32;
}

/// `kCGDesktopWindowLevelKey`.
const DESKTOP_WINDOW_LEVEL_KEY: i32 = 2;
/// `kCGScreenSaverWindowLevelKey`.
const SCREENSAVER_WINDOW_LEVEL_KEY: i32 = 13;

// NSWindowCollectionBehavior: on every Space, stays put when Spaces change
// (no Mission Control slide), and left out of Cmd-` window cycling.
const CAN_JOIN_ALL_SPACES: usize = 1 << 0;
const STATIONARY: usize = 1 << 4;
const IGNORES_CYCLE: usize = 1 << 6;

/// Call on the main thread (Tauri's setup and menu handlers run there).
pub fn pin_to_desktop(window: &WebviewWindow) -> tauri::Result<()> {
    set_level(window, DESKTOP_WINDOW_LEVEL_KEY)
}

/// Call on the main thread.
pub fn raise_to_screensaver(window: &WebviewWindow) -> tauri::Result<()> {
    set_level(window, SCREENSAVER_WINDOW_LEVEL_KEY)
}

fn set_level(window: &WebviewWindow, key: i32) -> tauri::Result<()> {
    let ns_window = window.ns_window()? as *mut AnyObject;
    // SAFETY: ns_window is the live NSWindow behind `window`, used on the main
    // thread; setLevel: and setCollectionBehavior: take NSInteger / NSUInteger.
    unsafe {
        let ns_window = &*ns_window;
        let level = CGWindowLevelForKey(key) as isize;
        let _: () = msg_send![ns_window, setLevel: level];
        let behavior = CAN_JOIN_ALL_SPACES | STATIONARY | IGNORES_CYCLE;
        let _: () = msg_send![ns_window, setCollectionBehavior: behavior];
    }
    Ok(())
}
