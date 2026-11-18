//! Windows for each mode, and the tray / menu-bar icon: what
//! `apps/desktop/src/app.rs` does with Tauri. GPUI has no window levels or
//! click-through, so wallpaper and screensaver windows set them natively.

use gpui::{
    App, AppContext, Bounds, DisplayId, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowHandle, WindowOptions, px, size,
};

use crate::app::{Dashboard, Mode, Shared};

/// For test runs: the window doesn't take focus, floats above the others and
/// lets clicks through.
const NO_FOCUS: &str = "DASH_NO_FOCUS";

/// Show the app window, or bring it to the front if it is open.
pub fn open_window(shared: &Shared, ambient: bool, cx: &mut App) {
    let no_focus = std::env::var_os(NO_FOCUS).is_some();
    let mode = if ambient { Mode::Ambient } else { Mode::Window };
    for handle in cx.windows() {
        if let Some(handle) = handle.downcast::<Dashboard>()
            && handle.read_with(cx, |d, _| d.mode == mode).unwrap_or(false)
        {
            let _ = handle.update(cx, |_, window, _| window.activate_window());
            cx.activate(true);
            return;
        }
    }
    let bounds = Bounds::centered(None, size(px(1280.0), px(800.0)), cx);
    let shared = shared.clone();
    let opened = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(480.0), px(320.0))),
            titlebar: Some(TitlebarOptions {
                title: Some(
                    if ambient {
                        "Dashboard (ambient)"
                    } else {
                        "Dashboard"
                    }
                    .into(),
                ),
                ..Default::default()
            }),
            focus: !no_focus,
            ..Default::default()
        },
        move |window, cx| cx.new(|cx| Dashboard::new(shared, mode, window, cx)),
    );
    match opened {
        Ok(handle) if no_focus => {
            let _ = handle.update(cx, |_, window, _| native::float(window));
        }
        Ok(_) => cx.activate(true),
        Err(err) => eprintln!("couldn't open the window: {err:#}"),
    }
}

/// A borderless ambient window that covers one display.
fn covering(
    shared: &Shared,
    mode: Mode,
    bounds: Bounds<gpui::Pixels>,
    display: DisplayId,
    focus: bool,
    cx: &mut App,
) -> Option<WindowHandle<Dashboard>> {
    let shared = shared.clone();
    let opened = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            focus,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            display_id: Some(display),
            window_background: WindowBackgroundAppearance::Opaque,
            ..Default::default()
        },
        move |window, cx| cx.new(|cx| Dashboard::new(shared, mode, window, cx)),
    );
    match opened {
        Ok(handle) => Some(handle),
        Err(err) => {
            eprintln!("couldn't open a window: {err:#}");
            None
        }
    }
}

/// One window per display, on the desktop. Returns them, to close later.
pub fn wallpaper(shared: &Shared, cx: &mut App) -> Vec<WindowHandle<Dashboard>> {
    let displays = cx.displays();
    let mut windows = vec![];
    for display in displays {
        let bounds = display.bounds();
        if let Some(handle) = covering(shared, Mode::Wallpaper, bounds, display.id(), false, cx) {
            let _ = handle.update(cx, |_, window, _| native::pin_to_desktop(window, bounds));
            windows.push(handle);
        }
    }
    windows
}

/// One window per display, above everything, until Esc.
pub fn screensaver(shared: &Shared, cx: &mut App) {
    let displays = cx.displays();
    for (i, display) in displays.into_iter().enumerate() {
        let bounds = display.bounds();
        if let Some(handle) = covering(shared, Mode::Screensaver, bounds, display.id(), i == 0, cx)
        {
            let _ = handle.update(cx, |_, window, _| native::raise_to_screensaver(window));
        }
    }
    cx.activate(true);
}

/// The screensaver inside the Control Panel's preview box (Windows `/p`).
/// Only Windows has one; elsewhere just show the screensaver.
pub fn preview(shared: &Shared, parent: isize, cx: &mut App) {
    #[cfg(windows)]
    {
        let display = cx.primary_display();
        if let Some(display) = display {
            let bounds = display.bounds();
            if let Some(handle) = covering(shared, Mode::Ambient, bounds, display.id(), false, cx) {
                let _ = handle.update(cx, |_, window, _| native::preview(window, parent));
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = parent;
        screensaver(shared, cx);
    }
}

#[cfg(any(target_os = "macos", windows))]
pub mod tray {
    use std::time::Duration;

    use gpui::{App, WindowHandle};
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, TrayIconBuilder};

    use crate::app::{Dashboard, Shared};

    fn icon() -> Option<Icon> {
        let image =
            image::load_from_memory(include_bytes!("../../desktop/icons/32x32.png")).ok()?;
        let rgba = image.into_rgba8();
        let (width, height) = rgba.dimensions();
        Icon::from_rgba(rgba.into_raw(), width, height).ok()
    }

    /// The menu-bar icon: open a window, turn the wallpaper on or off, quit.
    pub fn add(shared: Shared, wallpaper_on: Vec<WindowHandle<Dashboard>>, cx: &mut App) {
        let open = MenuItem::new("Open window", true, None);
        let ambient = MenuItem::new("Open ambient window", true, None);
        let wallpaper = CheckMenuItem::new("Wallpaper", true, !wallpaper_on.is_empty(), None);
        let quit = MenuItem::new("Quit", true, None);
        let menu = Menu::new();
        let built = menu.append_items(&[
            &open,
            &ambient,
            &wallpaper,
            &PredefinedMenuItem::separator(),
            &quit,
        ]);
        if let Err(err) = built {
            eprintln!("couldn't build the tray menu: {err}");
            return;
        }
        let mut builder = TrayIconBuilder::new()
            .with_tooltip("Dashboard")
            .with_menu(Box::new(menu));
        if let Some(icon) = icon() {
            builder = builder.with_icon(icon);
        }
        let tray = match builder.build() {
            Ok(tray) => tray,
            Err(err) => {
                eprintln!("couldn't add the tray icon: {err}");
                return;
            }
        };
        cx.spawn(async move |cx| {
            // Kept here: dropping it removes the icon.
            let _tray = tray;
            let mut windows = wallpaper_on;
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                while let Ok(event) = MenuEvent::receiver().try_recv() {
                    let id = event.id();
                    cx.update(|cx| {
                        if id == open.id() {
                            super::open_window(&shared, false, cx);
                        } else if id == ambient.id() {
                            super::open_window(&shared, true, cx);
                        } else if id == wallpaper.id() {
                            if wallpaper.is_checked() {
                                windows = super::wallpaper(&shared, cx);
                            } else {
                                for handle in windows.drain(..) {
                                    let _ =
                                        handle.update(cx, |_, window, _| window.remove_window());
                                }
                            }
                        } else if id == quit.id() {
                            cx.quit();
                        }
                    });
                }
            }
        })
        .detach();
    }
}

#[cfg(target_os = "macos")]
mod native {
    use gpui::{Bounds, Pixels, Window};
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGWindowLevelForKey(key: i32) -> i32;
    }

    /// `kCGDesktopWindowLevelKey`.
    const DESKTOP_WINDOW_LEVEL_KEY: i32 = 2;
    /// `kCGScreenSaverWindowLevelKey`.
    const SCREENSAVER_WINDOW_LEVEL_KEY: i32 = 13;
    /// `kCGFloatingWindowLevelKey`.
    const FLOATING_WINDOW_LEVEL_KEY: i32 = 5;

    // NSWindowCollectionBehavior: on every Space, stays put when Spaces change,
    // and left out of Cmd-` window cycling.
    const CAN_JOIN_ALL_SPACES: usize = 1 << 0;
    const STATIONARY: usize = 1 << 4;
    const IGNORES_CYCLE: usize = 1 << 6;

    fn ns_window(window: &Window) -> Option<*mut AnyObject> {
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return None;
        };
        let view = handle.ns_view.as_ptr() as *mut AnyObject;
        // SAFETY: the handle holds the live NSView behind `window`, on the main thread.
        let ns_window: *mut AnyObject = unsafe { msg_send![&*view, window] };
        (!ns_window.is_null()).then_some(ns_window)
    }

    fn set_level(window: &Window, key: i32, ignore_mouse: bool) {
        let Some(ns_window) = ns_window(window) else {
            eprintln!("no native window to set the level of");
            return;
        };
        // SAFETY: ns_window is live and this runs on the main thread; these
        // methods take NSInteger, NSUInteger and BOOL.
        unsafe {
            let ns_window = &*ns_window;
            let level = CGWindowLevelForKey(key) as isize;
            let _: () = msg_send![ns_window, setLevel: level];
            let behavior = CAN_JOIN_ALL_SPACES | STATIONARY | IGNORES_CYCLE;
            let _: () = msg_send![ns_window, setCollectionBehavior: behavior];
            let _: () = msg_send![ns_window, setIgnoresMouseEvents: ignore_mouse];
        }
    }

    pub fn pin_to_desktop(window: &Window, _bounds: Bounds<Pixels>) {
        set_level(window, DESKTOP_WINDOW_LEVEL_KEY, true);
    }

    pub fn raise_to_screensaver(window: &Window) {
        set_level(window, SCREENSAVER_WINDOW_LEVEL_KEY, false);
    }

    pub fn float(window: &Window) {
        set_level(window, FLOATING_WINDOW_LEVEL_KEY, true);
    }
}

#[cfg(windows)]
mod native {
    //! Not compiled or run yet: this build has only been made on macOS.
    //! See `apps/desktop/src/win.rs` for how the WorkerW parenting works.

    use std::ffi::c_void;

    use gpui::{Bounds, Pixels, Window};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, FindWindowExW, FindWindowW, GWL_EXSTYLE, GetClientRect, GetSystemMetrics,
        GetWindowLongPtrW, IsWindow, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SMTO_NORMAL,
        SWP_NOACTIVATE, SWP_NOZORDER, SendMessageTimeoutW, SetParent, SetWindowLongPtrW,
        SetWindowPos, WS_EX_LAYERED, WS_EX_TRANSPARENT,
    };
    use windows::core::{BOOL, PCWSTR, w};

    fn hwnd(window: &Window) -> Option<HWND> {
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return None;
        };
        Some(HWND(handle.hwnd.get() as *mut c_void))
    }

    pub fn pin_to_desktop(window: &Window, bounds: Bounds<Pixels>) {
        let Some(hwnd) = hwnd(window) else {
            return;
        };
        let Some(worker) = desktop_worker() else {
            eprintln!("no desktop WorkerW found; the wallpaper stays a normal window");
            return;
        };
        let scale = window.scale_factor();
        // SAFETY: both handles are live windows; the calls only change parent,
        // style and position.
        unsafe {
            let _ = SetParent(hwnd, Some(worker));
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(
                hwnd,
                GWL_EXSTYLE,
                style | WS_EX_LAYERED.0 as isize | WS_EX_TRANSPARENT.0 as isize,
            );
            // WorkerW covers the whole virtual screen, whose origin can be negative.
            let x =
                (f32::from(bounds.origin.x) * scale) as i32 - GetSystemMetrics(SM_XVIRTUALSCREEN);
            let y =
                (f32::from(bounds.origin.y) * scale) as i32 - GetSystemMetrics(SM_YVIRTUALSCREEN);
            let _ = SetWindowPos(
                hwnd,
                None,
                x,
                y,
                (f32::from(bounds.size.width) * scale) as i32,
                (f32::from(bounds.size.height) * scale) as i32,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    pub fn raise_to_screensaver(_window: &Window) {}

    pub fn float(_window: &Window) {}

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

    /// For the top-level window that holds the icons, the WorkerW after it is
    /// the one behind them.
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

    /// Show inside the preview box `parent`, and quit when the box goes away.
    pub fn preview(window: &Window, parent: isize) {
        let Some(hwnd) = hwnd(window) else {
            return;
        };
        let parent_hwnd = HWND(parent as *mut c_void);
        let mut rect = RECT::default();
        // SAFETY: the handles are live; the calls read a size, set the parent and move.
        unsafe {
            let _ = GetClientRect(parent_hwnd, &mut rect);
            let _ = SetParent(hwnd, Some(parent_hwnd));
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                // SAFETY: IsWindow accepts any handle value.
                if !unsafe { IsWindow(Some(HWND(parent as *mut c_void))) }.as_bool() {
                    std::process::exit(0);
                }
            }
        });
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod native {
    //! Linux: no window levels here yet. Wallpaper mode could use GPUI's
    //! Wayland layer-shell windows.

    use gpui::{Bounds, Pixels, Window};

    pub fn pin_to_desktop(_window: &Window, _bounds: Bounds<Pixels>) {}

    pub fn raise_to_screensaver(_window: &Window) {}

    pub fn float(_window: &Window) {}
}
