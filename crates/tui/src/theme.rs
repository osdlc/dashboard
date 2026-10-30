//! Colors for light and dark terminals.

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
    /// Background behind whatever the mouse is over.
    pub hover: Color,
}

impl Palette {
    /// Asks the terminal whether its background is light. Call once, before
    /// the TUI takes over the terminal; falls back to dark.
    pub fn terminal_is_light() -> bool {
        matches!(theme_mode(QueryOptions::default()), Ok(ThemeMode::Light))
    }

    /// `auto_light` is the result of `terminal_is_light`, used for `Theme::Auto`.
    pub fn for_theme(theme: Theme, auto_light: bool) -> Self {
        let light = match theme {
            Theme::Light => true,
            Theme::Dark => false,
            Theme::Auto => auto_light,
        };
        if light { Self::light() } else { Self::dark() }
    }

    fn dark() -> Self {
        Self {
            heat: [(80, 200, 120), (230, 190, 60), (240, 80, 80)],
            cool: [(60, 210, 200), (90, 140, 245), (170, 110, 240)],
            sunset: [(140, 90, 230), (235, 90, 160), (250, 160, 70)],
            muted: Color::DarkGray,
            ok: Color::Green,
            warn: Color::Yellow,
            error: Color::Red,
            accent: Color::Cyan,
            hover: Color::Rgb(48, 54, 66),
        }
    }

    /// Darker tones: terminal yellow and green are unreadable on white.
    fn light() -> Self {
        Self {
            heat: [(20, 140, 70), (190, 130, 0), (200, 40, 50)],
            cool: [(0, 140, 140), (40, 90, 210), (120, 50, 190)],
            sunset: [(100, 50, 190), (200, 40, 120), (210, 110, 0)],
            muted: Color::Rgb(110, 110, 110),
            ok: Color::Rgb(0, 130, 60),
            warn: Color::Rgb(170, 110, 0),
            error: Color::Rgb(190, 30, 40),
            accent: Color::Rgb(30, 90, 200),
            hover: Color::Rgb(222, 228, 238),
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
        crate::charts::heat_color(stops, t)
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
}
