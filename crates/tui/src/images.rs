//! The image widget. The terminal's graphics support is detected once at
//! start: Kitty, iTerm2 or Sixel when it answers for one, colored half blocks
//! otherwise. `DASHBOARD_IMAGES=halfblocks` skips the query. Each picture is
//! decoded once and encoded again only when its file or its area changes.

use std::collections::VecDeque;
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::SystemTime;

use image::DynamicImage;
use image::imageops::FilterType;
use ratatui::layout::{Rect, Size};
use ratatui::style::Style;
use ratatui::widgets::{Paragraph, Wrap};
use ratatui_image::picker::Picker;
use ratatui_image::protocol::Protocol;
use ratatui_image::{Image, Resize};

use crate::canvas::Canvas;
use crate::fit;
use crate::theme::Palette;

static PICKER: OnceLock<Picker> = OnceLock::new();

/// Pictures kept decoded. A screensaver shows one at a time, and a screen
/// rarely has more than a few.
const KEEP: usize = 6;

struct Entry {
    path: String,
    modified: Option<SystemTime>,
    image: Result<DynamicImage, String>,
    /// The last encoding, for this area size.
    encoded: Option<(Size, Result<Protocol, String>)>,
}

static CACHE: LazyLock<Mutex<VecDeque<Entry>>> = LazyLock::new(Default::default);

/// Asks the terminal which graphics it supports. Call after entering raw mode
/// and the alternate screen, before reading events.
pub fn detect() {
    let picker = match std::env::var("DASHBOARD_IMAGES").as_deref() {
        Ok("halfblocks") => Picker::halfblocks(),
        _ => Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks()),
    };
    let _ = PICKER.set(picker);
}

/// Half blocks until `detect` runs, so tests and captures need no terminal.
fn picker() -> &'static Picker {
    PICKER.get_or_init(Picker::halfblocks)
}

pub fn draw(f: &mut Canvas, p: &Palette, area: Rect, path: &str, caption: Option<&str>) {
    let (picture, line) = match caption {
        Some(c) if area.height > 2 => (
            Rect {
                height: area.height - 1,
                ..area
            },
            Some((c, Rect::new(area.x, area.bottom() - 1, area.width, 1))),
        ),
        _ => (area, None),
    };
    if let Some((text, rect)) = line {
        let text = fit::truncate(text, rect.width as usize);
        f.render_widget(
            Paragraph::new(text)
                .style(Style::new().fg(p.muted))
                .centered(),
            rect,
        );
    }
    if picture.width == 0 || picture.height == 0 {
        return;
    }

    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let entry = entry(&mut cache, path);
    let size = Size::new(picture.width, picture.height);
    if entry.encoded.as_ref().is_none_or(|(s, _)| *s != size) {
        let encoded = match &entry.image {
            Ok(image) => picker()
                .new_protocol(
                    image.clone(),
                    size,
                    Resize::Scale(Some(FilterType::Triangle)),
                )
                .map_err(|e| e.to_string()),
            Err(e) => Err(e.clone()),
        };
        entry.encoded = Some((size, encoded));
    }
    match &entry.encoded {
        Some((_, Ok(protocol))) => {
            // Centered: the picture keeps its shape, so one side has room left.
            let got = protocol.size();
            let rect = Rect::new(
                picture.x + picture.width.saturating_sub(got.width) / 2,
                picture.y + picture.height.saturating_sub(got.height) / 2,
                got.width.min(picture.width),
                got.height.min(picture.height),
            );
            f.render_widget(Image::new(protocol), rect);
        }
        Some((_, Err(e))) => {
            f.render_widget(
                Paragraph::new(format!("can't show {path}: {e}"))
                    .style(Style::new().fg(p.warn))
                    .wrap(Wrap { trim: true }),
                picture,
            );
        }
        None => {}
    }
}

/// The cache entry for `path`, decoding the file when it is new or changed.
/// The most recently used entry is at the back.
fn entry<'a>(cache: &'a mut VecDeque<Entry>, path: &str) -> &'a mut Entry {
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    if let Some(i) = cache.iter().position(|e| e.path == path) {
        let e = cache.remove(i).expect("index from position");
        if e.modified == modified && modified.is_some() {
            cache.push_back(e);
            return cache.back_mut().expect("just pushed");
        }
    }
    if cache.len() >= KEEP {
        cache.pop_front();
    }
    cache.push_back(Entry {
        path: path.to_string(),
        modified,
        image: decode(path),
        encoded: None,
    });
    cache.back_mut().expect("just pushed")
}

fn decode(path: &str) -> Result<DynamicImage, String> {
    image::ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;

    fn sample() -> String {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../modules-system/testdata/screensaver/sunset.png"
        )
        .into()
    }

    fn render(path: &str, w: u16, h: u16) -> Buffer {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        let p = Palette::for_theme(dashboard_contract::Theme::Dark, false);
        draw(&mut Canvas(&mut buf), &p, area, path, Some("sunset.png"));
        buf
    }

    #[test]
    fn half_blocks_fill_the_width_and_keep_the_shape() {
        let buf = render(&sample(), 40, 20);
        let block = |x: u16, y: u16| matches!(buf[(x, y)].symbol(), "▀" | "▄");
        // 480x300 is wider than the 40x19 cell area, so it fills the width
        // and leaves equal room above and below.
        let rows: Vec<u16> = (0..19).filter(|&y| block(20, y)).collect();
        assert!(!rows.is_empty(), "nothing drawn");
        let (top, bottom) = (rows[0], 18 - rows.last().unwrap());
        assert!(top > 0 && top.abs_diff(bottom) <= 1, "rows {rows:?}");
        let middle = rows[rows.len() / 2];
        assert!((0..40).all(|x| block(x, middle)));
        let caption: String = (0..40).map(|x| buf[(x, 19)].symbol()).collect();
        assert_eq!(caption.trim(), "sunset.png");
    }

    /// One sample per format the widget documents: each decodes and draws.
    #[test]
    fn every_sample_format_draws() {
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../modules-system/testdata/screensaver"
        );
        for name in [
            "ball.gif",
            "hills.jpg",
            "night.webp",
            "peaks.bmp",
            "sunset.png",
        ] {
            let buf = render(&format!("{dir}/{name}"), 40, 20);
            let blocks = (0..40)
                .flat_map(|x| (0..19).map(move |y| (x, y)))
                .filter(|&(x, y)| matches!(buf[(x, y)].symbol(), "▀" | "▄"))
                .count();
            assert!(blocks > 200, "{name}: {blocks} cells drawn");
        }
    }

    #[test]
    fn a_missing_file_says_so() {
        let buf = render("/no/such/picture.png", 40, 6);
        let text: String = (0..40).map(|x| buf[(x, 0)].symbol()).collect();
        assert!(text.starts_with("can't show"), "{text}");
    }
}
