# 0001: GPUI for the native app

Status: decided, November 2026.

## Decision

The native app (`apps/native`) stays on GPUI. The Slint and SwiftUI versions
built to compare with it are removed. The cheap settings for the native app are
a low `native.fps` and, if motion isn't wanted, `native.animations: false`.

## Why we looked

The GPUI app redraws the whole window on every frame: one view, a full layout
of every tile, about 10 ms of CPU per frame. We wanted to know whether a
retained framework, which redraws only what changed, would cost less to run
all day as a window, wallpaper or screensaver.

## What was built

Two experiments drawing the same screens, tiles and widgets as the GPUI app,
from the same config:

- **Slint 1.18.1** (`apps/slint`, about 3,400 lines of Rust and `.slint`). Ran
  the modules in-process like the GPUI app. Rust did the layout and produced
  positioned pieces per tile; Slint drew them, and its property tracking
  redrew only the rows whose values changed. Numbers and fills animated with
  Slint's `animate`.
- **SwiftUI** (`apps/swiftui`, about 3,200 lines of Swift, macOS 14+). Swift
  can't link the Rust host, so it started `dashboard serve` as a child process
  and read tiles over the web view's WebSocket. One `@Observable` model per
  tile, so an update to one tile redrew only that tile. Bar fills used Core
  Animation; numbers rolled with a SwiftUI push transition.

Both followed the frame limit and the animations switch: at the time the
top-level keys `frame_rate:` and `animations:`, now `native.fps` and
`native.animations`. The GPUI app is about 6,700 lines.

## Results

The showcase screen of `examples/demo.yaml`, a 1280x830 window on screen but
not focused, on macOS. CPU is percent of one core, summed over the app and
every child process (server, plugins), from 10 s to 40 s after start.

| Setting | GPUI | Slint | SwiftUI |
|---|---|---|---|
| Defaults (30 fps, animations on) | 9.9% | 47.8% | 79.9% |
| `frame_rate: 1` | 2.4% | 3.2% | 33.4% |
| `frame_rate: 1`, number roll off | | | 11.0% |
| `frame_rate: 2`, animations on | 2.9% | 46.5% | 56.0% |
| `frame_rate: 2`, `animations: false` | 2.8% | 4.1% | 5.9% |
| Memory | 92 MB | 141 MB | 133–145 MB |

SwiftUI's 5.9% is 3.7% for the app and 2.2% for the server it needs. The
default and 1 fps rows were measured at a load average of 3.5–5.7, the 2 fps
rows at 2.3–3.8.

## What the numbers say

- **Animation is the cost, not the framework.** With animations off, all three
  are within a few points of each other.
- **GPUI is cheapest with animations on** because the app steps its own
  animations, so the frame limit caps them too. Slint and SwiftUI run their
  animations at the display's rate (up to 120 Hz) whatever the frame limit says.
  Capping them would mean dropping the frameworks' animation systems and
  stepping values by hand, which removes most of what they offer.
- **SwiftUI's text animations run on the main thread.** Number rolls are laid
  out again on every frame. Only Core Animation layers (the bar fills) run off
  it.
- **Redrawing only what changed didn't win.** Slint and SwiftUI redraw per
  tile, and GPUI redraws everything, yet GPUI was no more expensive at equal
  frame rates. At 1–2 fps the full redraw costs very little.

## Costs of switching that we'd also pay

- SwiftUI is macOS only and needs the server as a second process, plus a Swift
  copy of the contract types kept in step by hand.
- Slint and SwiftUI each need their own copy of the layout and fitting rules,
  kept in step with the GPUI and web views.

## Left open

- GPUI could cache each tile's drawing (a view per tile with `.cached()`), so
  that a frame lays out only the tiles that changed. Likely 3–5x less CPU per
  frame at 30 fps; small in absolute terms at 1–2 fps. Not done.
- Whether `native.animations: false` should be the default for wallpaper and
  screensaver modes.

The two experiments were never merged; this document is what's kept of them.
