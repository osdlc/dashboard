# Desktop app, wallpaper and screensaver

`sitroom-desktop` (in `apps/desktop`) is sitroom as a desktop app. It is a separate
binary from the `sitroom` CLI, so the CLI stays small. It is built with Tauri 2 and uses the
system webview: WKWebView on macOS, WebView2 on Windows.

It runs the same modules as `sitroom serve` (crate `sitroom-modules`) and the same web server
(`sitroom-web`), on 127.0.0.1 and any free port, and loads that page in its windows. The page
talks to the server over the usual WebSocket, not through Tauri, so the app needs no IPC
permissions.

## Run

```sh
cargo run -p sitroom-desktop -- --config examples/demo.yaml                # a window, and a tray / menu-bar icon
cargo run -p sitroom-desktop -- --config examples/demo.yaml --ambient      # the window in ambient mode
cargo run -p sitroom-desktop -- --config examples/demo.yaml --wallpaper    # on the desktop, behind the icons
cargo run -p sitroom-desktop -- --config examples/demo.yaml --screensaver  # full screen until a key or mouse move
```

Other flags: `--port N` (default: any free port) and `--no-tray`. Without `--config` it uses the
same config file as `sitroom`. `--wallpaper --no-tray` is refused: wallpaper windows ignore
clicks, so the tray's *Quit* is the only way to end the app.

The native GPUI app (experimental) takes the same flags: `cargo run -p sitroom-native -- --ambient`.

Plain `cargo build` and `cargo test` leave both apps out (they are slow to build, and on Linux the
Tauri app needs WebKitGTK). `cargo build --workspace` or `-p sitroom-desktop` / `-p sitroom-native`
include them. Neither app is in the release archives or Homebrew yet; those have `sitroom` and `sit`.

Build the macOS app bundle (not signed or notarized):

```sh
cd apps/desktop && cargo tauri build --bundles app -- --profile small
# target/small/bundle/macos/Sitroom.app
```

## Modes

- **Window:** a normal window with the full web view. The tray / menu-bar icon has *Open window*,
  *Open ambient window*, *Wallpaper* (on or off) and *Quit*. With the icon, closing the window
  keeps the app and its modules running; *Quit* ends it. On macOS a click on the Dock icon opens
  the window again.
- **Ambient mode** is part of the web view, so the browser gets it too: add `mode=ambient` to the
  page URL (`?mode=ambient`). No header or editor, large type, nothing to click, and every screen rotates (the
  config's `rotate:` if set, otherwise every screen for 30 seconds each).
- **Wallpaper:** one borderless ambient window per display that ignores the mouse.
  - macOS: the window sits at the desktop window level (the level of the desktop picture), on
    every Space, and stays put when Spaces change. The app has no Dock icon in this mode.
  - Windows: the window is made a child of the desktop's `WorkerW` window, behind the icons. This
    is the trick wallpaper apps use: sending Progman the message `0x052C` makes Explorer create
    that window. On Windows 11 24H2 and later the `WorkerW` is a child of Progman; both layouts are
    handled. If none is found, the window stays a normal bottom-most window.
- **Screensaver:** one full-screen ambient window per display, above everything (on macOS at the
  screensaver window level, above the menu bar and Dock). Any key, click or mouse move quits.

The wallpaper and screensaver windows are made from the displays connected when the mode starts.
A display added later gets no window, and one removed leaves its window behind; turn wallpaper
off and on again from the tray to match the displays.

Links open in the system browser, not in the app: the windows only load sitroom's own
page, and a link that asks for a new window (`target="_blank"` or `window.open`)
goes to the browser too. Only `http` and `https` links are opened.

## Frame rate and animations

The native GPUI app (`apps/native`) has its own settings, like `tui:` for the terminal view:

```yaml
native:
  fps: 30            # 1 to 240; default 30
  animations: false  # default true
```

`fps` is the most frames a second the app draws. Animation and new data both wait for the next
frame, so a lower number uses less CPU and moves less smoothly; the display's refresh rate is the
limit above that. Input redraws at once, and an update for a tile on another screen doesn't
redraw at all.

With `animations: true`, numbers roll to new values, screens fade in, and charts scroll as new
points arrive. With `false`, new values show at once, and the app draws only when data arrives
or you press a key, at most `fps` times a second.

The web view and the Tauri app ignore both, because the browser paces their animations. Like
`theme:`, the last config file that sets `native:` wins.

In the native app's screensaver, Esc quits and a click shows a tile fullscreen (Esc closes it
first). Moving the mouse does nothing, so you can reach a tile. Cmd-Q also quits.

## Windows screensaver (.scr)

Copy `sitroom-desktop.exe` to `Sitroom.scr`. Windows runs a screensaver with:

| Argument | What the app does |
|---|---|
| `/s` | the screensaver |
| `/p <hwnd>` | the screensaver inside the little preview box in the settings dialog; quits when the box goes away |
| `/c`, or no arguments | a message box that names the config file (there are no other settings) |

`/p:1234` and `/c:1234` forms are accepted too. Right-click the `.scr` and choose *Install*, or
copy it to `C:\Windows\System32`.

This has only been cross-compiled (`cargo xwin build`), not run: running it needs a Windows
machine.

## macOS .saver experiment

`apps/desktop/saver` is a small screensaver bundle written in Objective-C. Its view is a
WKWebView that loads the ambient page. `apps/desktop/saver/build.sh` builds
`target/saver/Sitroom.saver` (92 KB, not signed).

It does not start sitroom itself; it shows a server that is already running. It takes the
URL and the session token from, in order: `$SITROOM_SAVER_URL` and `$SITROOM_SAVER_TOKEN` (for
testing outside the screensaver host), the `url` and `token` screensaver defaults
(`defaults -currentHost write dev.sitroom.saver url 'http://127.0.0.1:7878/'`, and the same for
`token`), then the record that `sitroom serve --detach` writes,
`~/.local/share/run/sitroom/serve.json`. The token is put in the page's sessionStorage before the page runs, on the server's
origin only, so it never goes in the URL (see "Signing in to the web view" in
[cli.md](cli.md)). The app's own windows sign in the same way.

Can it reach localhost? macOS runs third-party screensavers inside `legacyScreenSaver.appex`,
which is sandboxed. Its entitlements (`codesign -d --entitlements - <appex>`) include
`com.apple.security.network.client` and `network.server`, and read-only access to every file
(`temporary-exception.files.absolute-path.read-only: /`). So, going by the entitlements, the view
may open `http://127.0.0.1` and read the serve record.

What was checked:

- Loaded outside the screensaver host (a test program that loads the bundle into a window), the
  view reached a local `sitroom serve` and loaded the page.
- Not checked: running it inside the real screensaver host. That needs installing it into
  `~/Library/Screen Savers`, which the build doesn't do. To try it: run
  `sitroom serve --detach`, build the saver, double-click `Sitroom.saver`, and pick it in
  System Settings. If the page doesn't load, the saver says why on screen and in Console
  (search for `SitroomSaver`).

Known limits: the token changes each time the server starts, so the saver needs the serve record
(or the default updated). The saver looks for the record only at the fixed path above, which is
the default location; if `SITROOM_*` or `XDG_*` variables move the CLI's run folder, set the
`url` and `token` defaults instead. A fixed token, or a token file, would make this simpler.

## Size and memory

Measured on macOS 26 (Apple Silicon), `small` profile (LTO, `opt-level = "s"`, stripped), with
every built-in module.

| | Size |
|---|---|
| `Sitroom.app` (macOS, arm64) | 9.0 MB; 4.3 MB zipped |
| `sitroom-desktop` binary, release profile (no LTO, not stripped) | 28.5 MB |
| `sitroom-desktop.exe` (Windows x64, cargo-xwin) | 10.3 MB, no extra DLLs (WebView2 comes with Windows) |
| `sitroom` CLI, for comparison | 5.7 MB (macOS), 6.2 MB (Windows) |

Memory at idle (RSS after 40 s, `examples/demo.yaml` with rotation on, one display):

| Mode | App process | WebKit processes (WebContent, Networking, GPU) | Total |
|---|---|---|---|
| Window | 79 MB | 116 MB | about 195 MB |
| Wallpaper | 83 MB | 172 MB | about 255 MB |

The demo config is busy (about 50 animated tiles); a small config uses less. The wallpaper
window is full-screen, so the page draws more.

Memory on Windows was not measured (no Windows machine).
