//! Colors and spacing. Colors come from the theme's named tokens, as in the
//! web view (`web/src/colors.ts`). Color scales mix their stops in oklab, like
//! `color-mix(in oklab, ...)` in `web/src/scale.ts`.

use std::rc::Rc;

use dashboard_contract::{ColorScale, Status, Tokens, Tone};
use gpui::{Hsla, Rgba};

#[derive(Clone)]
pub struct Palette(Rc<Tokens>);

impl Palette {
    pub fn new(tokens: Tokens) -> Self {
        Self(Rc::new(tokens))
    }

    pub fn get(&self, name: &str) -> Hsla {
        rgba(self.rgb(name)).into()
    }

    fn rgb(&self, name: &str) -> (u8, u8, u8) {
        self.0
            .get(name)
            .and_then(dashboard_contract::theme::rgb)
            .unwrap_or((128, 128, 128))
    }

    pub fn status(&self, status: Status) -> Hsla {
        match status {
            Status::Ok => self.get("ok"),
            Status::Warn => self.get("warn"),
            Status::Error => self.get("error"),
            Status::Unknown => self.get("muted"),
        }
    }

    pub fn tone(&self, tone: Tone) -> Hsla {
        self.get(tone.token())
    }

    /// The color for `t` in 0..1 on a color scale.
    pub fn scale(&self, scale: ColorScale, t: f64) -> Hsla {
        let Some(name) = scale_name(scale) else {
            return self.get("accent");
        };
        let t = t.clamp(0.0, 1.0) as f32;
        let stop = |s: &str| self.rgb(&format!("{name}-{s}"));
        let (a, b, share) = if t < 0.5 {
            (stop("low"), stop("mid"), t * 2.0)
        } else {
            (stop("mid"), stop("high"), (t - 0.5) * 2.0)
        };
        mix(a, b, share)
    }

    /// A scale's bottom and top colors, or `None` for `accent`.
    pub fn scale_ends(&self, scale: ColorScale) -> Option<(Hsla, Hsla)> {
        scale_name(scale)?;
        Some((self.scale(scale, 0.0), self.scale(scale, 1.0)))
    }
}

fn scale_name(scale: ColorScale) -> Option<&'static str> {
    match scale {
        ColorScale::Accent => None,
        ColorScale::Heat => Some("heat"),
        ColorScale::Cool => Some("cool"),
        ColorScale::Sunset => Some("sunset"),
    }
}

fn rgba((r, g, b): (u8, u8, u8)) -> Rgba {
    Rgba {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: 1.0,
    }
}

fn mix(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> Hsla {
    let (a, b) = (oklab(a), oklab(b));
    let m = |i: usize| a[i] + (b[i] - a[i]) * t;
    from_oklab([m(0), m(1), m(2)]).into()
}

fn oklab((r, g, b): (u8, u8, u8)) -> [f32; 3] {
    let lin = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (lin(r), lin(g), lin(b));
    let l = (0.41222147 * r + 0.53633254 * g + 0.05144599 * b).cbrt();
    let m = (0.2119035 * r + 0.6806995 * g + 0.10739696 * b).cbrt();
    let s = (0.08830246 * r + 0.28171884 * g + 0.6299787 * b).cbrt();
    [
        0.21045426 * l + 0.7936178 * m - 0.004_072_047 * s,
        1.9779985 * l - 2.4285922 * m + 0.4505937 * s,
        0.025904037 * l + 0.78277177 * m - 0.80867577 * s,
    ]
}

fn from_oklab([l, a, b]: [f32; 3]) -> Rgba {
    let x = (l + 0.39633778 * a + 0.21580376 * b).powi(3);
    let y = (l - 0.105561346 * a - 0.06385417 * b).powi(3);
    let z = (l - 0.08948418 * a - 1.2914855 * b).powi(3);
    let gamma = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        if c <= 0.0031308 {
            c * 12.92
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        }
    };
    Rgba {
        r: gamma(4.0767417 * x - 3.3077116 * y + 0.23096994 * z),
        g: gamma(-1.268438 * x + 2.6097574 * y - 0.34131938 * z),
        b: gamma(-0.0041960864 * x - 0.7034186 * y + 1.7076147 * z),
        a: 1.0,
    }
}

/// Spacing in pixels, from `web/src/style.css`: comfortable, compact (`d`) or
/// ambient, which scales with the window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    /// The size of `1rem`.
    pub rem: f32,
    pub gap: f32,
    pub row: f32,
    pub page: f32,
    pub max_width: f32,
    pub compact: bool,
    /// Compact tiles and rows have less padding, and wrappers smaller gaps.
    pub row_gap: f32,
    pub column_gap: f32,
}

impl Metrics {
    pub fn comfortable() -> Self {
        Self {
            rem: 16.0,
            gap: 16.0,
            row: 120.0,
            page: 16.0,
            max_width: 1400.0,
            compact: false,
            row_gap: 16.0,
            column_gap: 12.0,
        }
    }

    pub fn compact() -> Self {
        Self {
            rem: 16.0,
            gap: 4.0,
            row: 84.0,
            page: 8.0,
            max_width: f32::INFINITY,
            compact: true,
            row_gap: 10.0,
            column_gap: 6.0,
        }
    }

    pub fn ambient(window_width: f32) -> Self {
        let rem = (window_width * 0.0125).clamp(16.0, 36.0);
        Self {
            rem,
            gap: window_width * 0.015,
            row: rem * 8.0,
            page: window_width * 0.02,
            max_width: f32::INFINITY,
            compact: false,
            row_gap: 16.0,
            column_gap: 12.0,
        }
    }

    /// The height of one line of text `size` rem tall.
    pub fn line(&self, size: f32) -> f32 {
        (size * self.rem * 1.25).ceil()
    }

    pub fn font(&self, size: f32) -> f32 {
        size * self.rem
    }

    /// Roughly how wide `text` is at `size` rem. Used to divide up table
    /// columns and to guess at wrapping, not to place the text.
    pub fn text_width(&self, text: &str, size: f32) -> f32 {
        text.chars().count() as f32 * size * self.rem * 0.52
    }
}
