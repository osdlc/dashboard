//! Colors for light and dark terminals, from the color theme's tokens.
//!
//! The TUI keeps the terminal's own background and text color; everything
//! else comes from the theme (see `dashboard_contract::theme`). Colors that
//! are drawn as text are darkened (light) or lightened (dark) until they read
//! well on the theme's background: terminal glyphs are thin, so a color that
//! works as a web bar can be too faint as a character.

use dashboard_contract::theme::{self, ColorTheme, Tokens, Tone};
use dashboard_contract::{ColorScale, Status, Theme};
use ratatui::style::Color;
use terminal_colorsaurus::{QueryOptions, ThemeMode, theme_mode};

pub struct Palette {
    /// Low, middle and high stops for heat-colored graphs and meters.
    pub heat: [(u8, u8, u8); 3],
    pub cool: [(u8, u8, u8); 3],
    pub sunset: [(u8, u8, u8); 3],
    pub muted: Color,
    pub ok: Color,
    pub warn: Color,
    pub error: Color,
    pub accent: Color,
    pub info: Color,
    /// Categorical colors for series and bars, in order.
    pub series: [Color; 6],
    /// Background behind whatever the mouse is over.
    pub hover: Color,
    /// Background behind the item picked with the keyboard.
    pub selected: Color,
    depth: Depth,
}

/// How many colors the terminal can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    /// 24-bit color.
    True,
    /// The xterm 256-color palette.
    Ansi256,
    /// The 16 named colors, which the user's terminal theme defines.
    Ansi16,
}

impl Depth {
    /// From `COLORTERM` and `TERM`.
    pub fn detect() -> Self {
        Self::from_env(
            std::env::var("COLORTERM").ok().as_deref(),
            std::env::var("TERM").ok().as_deref(),
        )
    }

    pub fn from_env(colorterm: Option<&str>, term: Option<&str>) -> Self {
        if matches!(colorterm, Some("truecolor" | "24bit")) {
            Depth::True
        } else if term.is_some_and(|t| t.contains("256color") || t.contains("direct")) {
            Depth::Ansi256
        } else {
            Depth::Ansi16
        }
    }

    /// `rgb` as this terminal can show it: the nearest color it has.
    pub fn color(self, rgb: (u8, u8, u8)) -> Color {
        match self {
            Depth::True => Color::Rgb(rgb.0, rgb.1, rgb.2),
            Depth::Ansi256 => Color::Indexed(nearest_256(rgb)),
            Depth::Ansi16 => nearest_16(rgb),
        }
    }
}

/// Text colors need this contrast with the background (WCAG AA for text).
const TEXT_CONTRAST: f64 = 4.5;
/// Graph colors (lines, bars, meters) need less.
const GRAPH_CONTRAST: f64 = 3.0;

impl Palette {
    /// Asks the terminal whether its background is light. Call once, before
    /// the TUI takes over the terminal; falls back to dark.
    pub fn terminal_is_light() -> bool {
        matches!(theme_mode(QueryOptions::default()), Ok(ThemeMode::Light))
    }

    /// The default color theme. `auto_light` is the result of
    /// `terminal_is_light`, used for `Theme::Auto`.
    pub fn for_theme(theme: Theme, auto_light: bool) -> Self {
        Self::new(theme, &ColorTheme::default(), auto_light, Depth::detect())
    }

    pub fn new(theme: Theme, colors: &ColorTheme, auto_light: bool, depth: Depth) -> Self {
        let light = match theme {
            Theme::Light => true,
            Theme::Dark => false,
            Theme::Auto => auto_light,
        };
        Self::from_tokens(colors.variant(!light), depth)
    }

    pub fn from_tokens(t: &Tokens, depth: Depth) -> Self {
        let bg = rgb(&t.bg);
        let text = |hex: &str| depth.color(readable(rgb(hex), bg, TEXT_CONTRAST));
        let graph = |hex: &str| readable(rgb(hex), bg, GRAPH_CONTRAST);
        let stops = |a: &str, b: &str, c: &str| [graph(a), graph(b), graph(c)];
        Self {
            heat: stops(&t.heat_low, &t.heat_mid, &t.heat_high),
            cool: stops(&t.cool_low, &t.cool_mid, &t.cool_high),
            sunset: stops(&t.sunset_low, &t.sunset_mid, &t.sunset_high),
            muted: text(&t.muted),
            ok: text(&t.ok),
            warn: text(&t.warn),
            error: text(&t.error),
            accent: text(&t.accent),
            info: text(&t.info),
            series: [
                &t.series_1,
                &t.series_2,
                &t.series_3,
                &t.series_4,
                &t.series_5,
                &t.series_6,
            ]
            .map(|hex| depth.color(graph(hex))),
            hover: depth.color(rgb(&t.surface_2)),
            selected: depth.color(rgb(&t.accent_soft)),
            depth,
        }
    }

    /// Color for `t` in 0..=1 on a color scale. `Accent` ignores `t`.
    pub fn scale(&self, scale: ColorScale, t: f64) -> Color {
        let stops = match scale {
            ColorScale::Accent => return self.accent,
            ColorScale::Heat => &self.heat,
            ColorScale::Cool => &self.cool,
            ColorScale::Sunset => &self.sunset,
        };
        match crate::charts::heat_color(stops, t) {
            Color::Rgb(r, g, b) => self.depth.color((r, g, b)),
            other => other,
        }
    }

    pub fn status(&self, status: Status) -> Color {
        match status {
            Status::Ok => self.ok,
            Status::Warn => self.warn,
            Status::Error => self.error,
            Status::Unknown => self.muted,
        }
    }

    pub fn opt_status(&self, status: Option<Status>) -> Color {
        status.map(|s| self.status(s)).unwrap_or(Color::Reset)
    }

    /// The color a widget names with a `tone`.
    pub fn tone(&self, tone: Tone) -> Color {
        match tone {
            Tone::Accent => self.accent,
            Tone::Ok => self.ok,
            Tone::Warn => self.warn,
            Tone::Error => self.error,
            Tone::Info => self.info,
            Tone::Muted => self.muted,
            Tone::Series1 => self.series[0],
            Tone::Series2 => self.series[1],
            Tone::Series3 => self.series[2],
            Tone::Series4 => self.series[3],
            Tone::Series5 => self.series[4],
            Tone::Series6 => self.series[5],
        }
    }
}

/// Tokens are checked hex colors, so this only falls back on a bug.
fn rgb(hex: &str) -> (u8, u8, u8) {
    theme::rgb(hex).unwrap_or((128, 128, 128))
}

/// `c`, moved toward black (light background) or white (dark) in small
/// steps until it has `min` contrast with `bg`.
fn readable(c: (u8, u8, u8), bg: (u8, u8, u8), min: f64) -> (u8, u8, u8) {
    let target = if theme::luminance(bg) > 0.5 {
        0.0
    } else {
        255.0
    };
    let mut out = c;
    for step in 1..=20 {
        if theme::contrast(out, bg) >= min {
            break;
        }
        let k = step as f64 / 20.0;
        let mix = |x: u8| (x as f64 + (target - x as f64) * k).round() as u8;
        out = (mix(c.0), mix(c.1), mix(c.2));
    }
    out
}

/// The nearest xterm 256-palette color: the 6x6x6 cube or the gray ramp.
fn nearest_256((r, g, b): (u8, u8, u8)) -> u8 {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let level = |v: u8| {
        (0..6u8)
            .min_by_key(|&i| (LEVELS[i as usize] as i32 - v as i32).abs())
            .unwrap_or(0)
    };
    let (ri, gi, bi) = (level(r), level(g), level(b));
    let cube = (
        LEVELS[ri as usize],
        LEVELS[gi as usize],
        LEVELS[bi as usize],
    );
    // The gray ramp, 232 to 255, runs 8, 18, ..., 238.
    let avg = (r as u32 + g as u32 + b as u32) / 3;
    let step = ((avg.saturating_sub(8) + 5) / 10).min(23) as u8;
    let gray = 8 + step * 10;
    let dist = |c: (u8, u8, u8)| {
        let d = |x: u8, y: u8| (x as i32 - y as i32).pow(2);
        d(c.0, r) + d(c.1, g) + d(c.2, b)
    };
    if dist((gray, gray, gray)) < dist(cube) {
        232 + step
    } else {
        16 + 36 * ri + 6 * gi + bi
    }
}

/// The nearest of the 16 named colors, by hue: their exact values depend on
/// the terminal's theme, so matching by distance would mislead.
fn nearest_16((r, g, b): (u8, u8, u8)) -> Color {
    let (rf, gf, bf) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let lightness = (max + min) / 2.0;
    let chroma = max - min;
    if chroma < 0.15 {
        return match lightness {
            l if l < 0.2 => Color::Black,
            l if l < 0.55 => Color::DarkGray,
            l if l < 0.85 => Color::Gray,
            _ => Color::White,
        };
    }
    let hue = if max == rf {
        60.0 * ((gf - bf) / chroma).rem_euclid(6.0)
    } else if max == gf {
        60.0 * ((bf - rf) / chroma + 2.0)
    } else {
        60.0 * ((rf - gf) / chroma + 4.0)
    };
    let bright = lightness > 0.6;
    let pick = |light: Color, normal: Color| if bright { light } else { normal };
    match hue {
        h if !(20.0..330.0).contains(&h) => pick(Color::LightRed, Color::Red),
        h if h < 75.0 => pick(Color::LightYellow, Color::Yellow),
        h if h < 160.0 => pick(Color::LightGreen, Color::Green),
        h if h < 200.0 => pick(Color::LightCyan, Color::Cyan),
        h if h < 260.0 => pick(Color::LightBlue, Color::Blue),
        _ => pick(Color::LightMagenta, Color::Magenta),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contrast_of(c: Color, bg: &str) -> f64 {
        let Color::Rgb(r, g, b) = c else {
            panic!("not truecolor: {c:?}")
        };
        theme::contrast((r, g, b), rgb(bg))
    }

    #[test]
    fn built_from_tokens_in_truecolor() {
        let t = ColorTheme::default();
        let p = Palette::new(Theme::Dark, &t, false, Depth::True);
        assert_eq!(p.accent, Color::Rgb(0x6c, 0x9c, 0xf0));
        assert_eq!(p.hover, Color::Rgb(0x2a, 0x2e, 0x35));
        assert_eq!(p.tone(Tone::Series2), Color::Rgb(0xe0, 0xa8, 0x26));
        assert_eq!(p.heat[2], (0xf0, 0x50, 0x50));
        // Auto follows the terminal.
        let light = Palette::new(Theme::Auto, &t, true, Depth::True);
        assert_eq!(light.selected, Color::Rgb(0xd5, 0xe2, 0xf8));
    }

    /// Light terminals: yellow and green text gets darker until it reads on white.
    #[test]
    fn text_colors_are_readable_on_the_background() {
        for theme in theme::builtins() {
            for (mode, tokens) in [(Theme::Light, &theme.light), (Theme::Dark, &theme.dark)] {
                let p = Palette::new(mode, &theme, false, Depth::True);
                for c in [p.muted, p.ok, p.warn, p.error, p.accent, p.info] {
                    let ratio = contrast_of(c, &tokens.bg);
                    assert!(
                        ratio >= TEXT_CONTRAST,
                        "{} {mode:?}: {c:?} {ratio}",
                        theme.name
                    );
                }
                for c in p.series {
                    assert!(contrast_of(c, &tokens.bg) >= GRAPH_CONTRAST);
                }
            }
        }
        let p = Palette::new(Theme::Light, &ColorTheme::default(), false, Depth::True);
        assert_ne!(
            p.warn,
            Color::Rgb(0xc9, 0x8a, 0x00),
            "light warn is darkened"
        );
    }

    #[test]
    fn depth_from_environment() {
        assert_eq!(Depth::from_env(Some("truecolor"), None), Depth::True);
        assert_eq!(Depth::from_env(Some("24bit"), Some("xterm")), Depth::True);
        assert_eq!(
            Depth::from_env(None, Some("xterm-256color")),
            Depth::Ansi256
        );
        assert_eq!(Depth::from_env(None, Some("xterm")), Depth::Ansi16);
        assert_eq!(Depth::from_env(None, None), Depth::Ansi16);
    }

    #[test]
    fn falls_back_to_the_nearest_palette_color() {
        assert_eq!(nearest_256((255, 0, 0)), 196);
        assert_eq!(nearest_256((0, 0, 0)), 16);
        assert_eq!(nearest_256((128, 128, 128)), 244);
        assert_eq!(nearest_256((0x6c, 0x9c, 0xf0)), 16 + 36 + 6 * 3 + 5);

        let dark = ColorTheme::default();
        let p = Palette::new(Theme::Dark, &dark, false, Depth::Ansi16);
        assert_eq!(p.ok, Color::Green);
        assert_eq!(p.warn, Color::Yellow);
        assert_eq!(p.error, Color::LightRed);
        assert_eq!(p.accent, Color::LightBlue);
        assert_eq!(p.muted, Color::Gray);
        assert!(matches!(
            p.scale(ColorScale::Heat, 1.0),
            Color::Red | Color::LightRed
        ));
        let p = Palette::new(Theme::Dark, &dark, false, Depth::Ansi256);
        assert!(matches!(p.accent, Color::Indexed(_)));
        assert!(matches!(p.scale(ColorScale::Cool, 0.3), Color::Indexed(_)));
    }
}
