//! Color themes: one set of named color tokens, used by every renderer.
//!
//! A color theme has a light and a dark variant; `Theme` (auto, light, dark)
//! picks which one shows. Widgets never carry raw colors: they name a token
//! with [`Tone`], and each renderer looks it up in the theme in use.
//! The web view gets the tokens in `UiSettings::color_theme` and turns them
//! into CSS variables (`--accent`, `--series-1`, ...); `web/src/themes.ts`
//! holds the built-in themes for pages that have no server.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Declares the token set once: the struct, its token names and lookup by name.
macro_rules! tokens {
    ($( $(#[doc = $doc:literal])* $field:ident => $name:literal, )*) => {
        /// Every color a renderer uses, as `#rrggbb`.
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
        pub struct Tokens {
            $( $(#[doc = $doc])* #[serde(rename = $name)] pub $field: String, )*
        }

        impl Tokens {
            /// Token names, as written in config files and CSS variables.
            pub const NAMES: &'static [&'static str] = &[$($name),*];

            pub fn get(&self, name: &str) -> Option<&str> {
                match name {
                    $($name => Some(&self.$field),)*
                    _ => None,
                }
            }

            fn slot(&mut self, name: &str) -> Option<&mut String> {
                match name {
                    $($name => Some(&mut self.$field),)*
                    _ => None,
                }
            }
        }
    };
}

/// Writes a `Tokens` with every field; the compiler catches a missing one.
macro_rules! define {
    ($($f:ident: $v:literal),* $(,)?) => {
        Tokens { $($f: $v.to_string()),* }
    };
}

tokens! {
    /// Page background.
    bg => "bg",
    /// Tiles and panels.
    surface => "surface",
    /// Hovered rows and buttons; inline code.
    surface_2 => "surface-2",
    /// Body text.
    text => "text",
    /// Labels, captions and other quiet text.
    muted => "muted",
    /// Tile and table borders.
    border => "border",
    /// The empty part of gauges, meters and bars.
    track => "track",
    accent => "accent",
    /// Background of the item picked with the keyboard.
    accent_soft => "accent-soft",
    /// Text on an `accent` background.
    on_accent => "on-accent",
    ok => "ok",
    warn => "warn",
    error => "error",
    info => "info",
    /// Categorical colors for series, bars and slices, in order.
    series_1 => "series-1",
    series_2 => "series-2",
    series_3 => "series-3",
    series_4 => "series-4",
    series_5 => "series-5",
    series_6 => "series-6",
    /// Stops of the `heat` color scale: low, middle, high.
    heat_low => "heat-low",
    heat_mid => "heat-mid",
    heat_high => "heat-high",
    cool_low => "cool-low",
    cool_mid => "cool-mid",
    cool_high => "cool-high",
    sunset_low => "sunset-low",
    sunset_mid => "sunset-mid",
    sunset_high => "sunset-high",
}

/// A token a widget can color something with, e.g. one line of a chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Tone {
    #[serde(rename = "accent")]
    Accent,
    #[serde(rename = "ok")]
    Ok,
    #[serde(rename = "warn")]
    Warn,
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "muted")]
    Muted,
    #[serde(rename = "series-1")]
    Series1,
    #[serde(rename = "series-2")]
    Series2,
    #[serde(rename = "series-3")]
    Series3,
    #[serde(rename = "series-4")]
    Series4,
    #[serde(rename = "series-5")]
    Series5,
    #[serde(rename = "series-6")]
    Series6,
}

impl Tone {
    /// The token's name, which is also its CSS variable without the `--`.
    pub fn token(self) -> &'static str {
        match self {
            Tone::Accent => "accent",
            Tone::Ok => "ok",
            Tone::Warn => "warn",
            Tone::Error => "error",
            Tone::Info => "info",
            Tone::Muted => "muted",
            Tone::Series1 => "series-1",
            Tone::Series2 => "series-2",
            Tone::Series3 => "series-3",
            Tone::Series4 => "series-4",
            Tone::Series5 => "series-5",
            Tone::Series6 => "series-6",
        }
    }

    /// The `i`th categorical color, wrapping after six.
    pub fn series(i: usize) -> Tone {
        const ALL: [Tone; 6] = [
            Tone::Series1,
            Tone::Series2,
            Tone::Series3,
            Tone::Series4,
            Tone::Series5,
            Tone::Series6,
        ];
        ALL[i % ALL.len()]
    }
}

/// A color theme with both variants, as sent to renderers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ColorTheme {
    pub name: String,
    pub light: Tokens,
    pub dark: Tokens,
}

impl Default for ColorTheme {
    fn default() -> Self {
        builtin(DEFAULT).expect("the default theme is built in")
    }
}

impl ColorTheme {
    pub fn variant(&self, dark: bool) -> &Tokens {
        if dark { &self.dark } else { &self.light }
    }

    /// This theme with some tokens replaced, e.g. from a config file.
    /// Each value must be a hex color (`#rgb` or `#rrggbb`).
    pub fn with_overrides<'a>(
        mut self,
        name: &str,
        light: impl IntoIterator<Item = (&'a str, &'a str)>,
        dark: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<Self, String> {
        self.name = name.to_string();
        for (mode, tokens, changes) in [
            (
                "light",
                &mut self.light,
                light.into_iter().collect::<Vec<_>>(),
            ),
            ("dark", &mut self.dark, dark.into_iter().collect()),
        ] {
            for (token, value) in changes {
                let Some(slot) = tokens.slot(token) else {
                    return Err(format!(
                        "{mode}.{token}: unknown token (tokens: {})",
                        Tokens::NAMES.join(", ")
                    ));
                };
                *slot = normalize_hex(value).ok_or_else(|| {
                    format!("{mode}.{token}: {value:?} is not a color; use #rrggbb or #rgb")
                })?;
            }
        }
        Ok(self)
    }
}

pub const DEFAULT: &str = "default";

/// Built-in theme names, the default first.
pub const BUILTIN: &[&str] = &[DEFAULT, "contrast", "paper", "ember"];

/// Every built-in theme, the default first.
pub fn builtins() -> Vec<ColorTheme> {
    BUILTIN.iter().filter_map(|n| builtin(n)).collect()
}

/// The built-in themes as a TypeScript module, as checked in at
/// `web/src/themes.ts`, so the web view has them before (or without) a server.
pub fn builtins_ts() -> String {
    let json = serde_json::to_string_pretty(&builtins()).expect("themes serialize");
    format!(
        "/* Generated from the Rust contract by pnpm gen:themes. Do not edit. */\n\n\
         import type {{ ColorTheme }} from './contract'\n\n\
         export const BUILTIN_THEMES: ColorTheme[] = {json}\n"
    )
}

/// `#rrggbb` (lowercase) for `#rrggbb` or `#rgb`, else `None`.
pub fn normalize_hex(value: &str) -> Option<String> {
    let hex = value.strip_prefix('#')?;
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let full = match hex.len() {
        6 => hex.to_ascii_lowercase(),
        3 => hex
            .chars()
            .flat_map(|c| [c, c])
            .collect::<String>()
            .to_ascii_lowercase(),
        _ => return None,
    };
    Some(format!("#{full}"))
}

/// The red, green and blue of a `#rrggbb` color.
pub fn rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let full = normalize_hex(hex)?;
    let byte = |i: usize| u8::from_str_radix(&full[i..i + 2], 16).ok();
    Some((byte(1)?, byte(3)?, byte(5)?))
}

/// WCAG contrast ratio between two colors, 1 to 21.
pub fn contrast(a: (u8, u8, u8), b: (u8, u8, u8)) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// WCAG relative luminance, 0 (black) to 1 (white).
pub fn luminance((r, g, b): (u8, u8, u8)) -> f64 {
    let lin = |c: u8| {
        let c = c as f64 / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

/// A built-in theme by name.
pub fn builtin(name: &str) -> Option<ColorTheme> {
    let (light, dark) = match name {
        // The original look. series-1..4 are the colors charts used before
        // there were series tokens: accent, warn, ok, error.
        DEFAULT => (
            define! {
                bg: "#f6f7f9", surface: "#ffffff", surface_2: "#e8ebef",
                text: "#1c1f24", muted: "#6b7280", border: "#dfe2e7", track: "#e8ebef",
                accent: "#2f6fde", accent_soft: "#d5e2f8", on_accent: "#ffffff",
                ok: "#1f9d55", warn: "#c98a00", error: "#d1393e", info: "#2f6fde",
                series_1: "#2f6fde", series_2: "#c98a00", series_3: "#1f9d55",
                series_4: "#d1393e", series_5: "#7832be", series_6: "#008c8c",
                heat_low: "#148c46", heat_mid: "#be8200", heat_high: "#c82832",
                cool_low: "#008c8c", cool_mid: "#285ad2", cool_high: "#7832be",
                sunset_low: "#6432be", sunset_mid: "#c82878", sunset_high: "#d26e00",
            },
            define! {
                bg: "#111317", surface: "#1a1d23", surface_2: "#2a2e35",
                text: "#e6e8eb", muted: "#9097a1", border: "#2c3038", track: "#2a2e35",
                accent: "#6c9cf0", accent_soft: "#2a364c", on_accent: "#ffffff",
                ok: "#3fbf74", warn: "#e0a826", error: "#ef5b5f", info: "#6c9cf0",
                series_1: "#6c9cf0", series_2: "#e0a826", series_3: "#3fbf74",
                series_4: "#ef5b5f", series_5: "#aa6ef0", series_6: "#3cd2c8",
                heat_low: "#50c878", heat_mid: "#e6be3c", heat_high: "#f05050",
                cool_low: "#3cd2c8", cool_mid: "#5a8cf5", cool_high: "#aa6ef0",
                sunset_low: "#8c5ae6", sunset_mid: "#eb5aa0", sunset_high: "#faa046",
            },
        ),
        // Black on white and white on black, strong borders, saturated colors.
        "contrast" => (
            define! {
                bg: "#ffffff", surface: "#ffffff", surface_2: "#e6e6e6",
                text: "#000000", muted: "#3a3a3a", border: "#4d4d4d", track: "#d0d0d0",
                accent: "#0047b3", accent_soft: "#cfe0ff", on_accent: "#ffffff",
                ok: "#00702f", warn: "#8a5300", error: "#b3001b", info: "#005f86",
                series_1: "#0047b3", series_2: "#b34700", series_3: "#00702f",
                series_4: "#b3001b", series_5: "#6a1bb3", series_6: "#006b6b",
                heat_low: "#00702f", heat_mid: "#8a5300", heat_high: "#b3001b",
                cool_low: "#006b6b", cool_mid: "#0047b3", cool_high: "#6a1bb3",
                sunset_low: "#6a1bb3", sunset_mid: "#a8005a", sunset_high: "#b34700",
            },
            define! {
                bg: "#000000", surface: "#0b0b0b", surface_2: "#262626",
                text: "#ffffff", muted: "#cccccc", border: "#9a9a9a", track: "#333333",
                accent: "#66b3ff", accent_soft: "#0d3361", on_accent: "#000000",
                ok: "#4be38a", warn: "#ffd23f", error: "#ff6b6b", info: "#6fdcff",
                series_1: "#66b3ff", series_2: "#ffd23f", series_3: "#4be38a",
                series_4: "#ff6b6b", series_5: "#d199ff", series_6: "#4ee6e6",
                heat_low: "#4be38a", heat_mid: "#ffd23f", heat_high: "#ff6b6b",
                cool_low: "#4ee6e6", cool_mid: "#66b3ff", cool_high: "#d199ff",
                sunset_low: "#d199ff", sunset_mid: "#ff7ac2", sunset_high: "#ffad5c",
            },
        ),
        // Warm off-white and soft charcoal, low-saturation ink colors.
        "paper" => (
            define! {
                bg: "#f3f0e8", surface: "#fbf9f4", surface_2: "#ebe6da",
                text: "#2b2924", muted: "#6b665b", border: "#ddd6c6", track: "#e8e2d4",
                accent: "#3e6a87", accent_soft: "#dbe4e8", on_accent: "#ffffff",
                ok: "#4d7a45", warn: "#9c6b17", error: "#a3423a", info: "#4a6f8a",
                series_1: "#3e6a87", series_2: "#9c6b17", series_3: "#4d7a45",
                series_4: "#a3423a", series_5: "#6d5a8c", series_6: "#4f7f7a",
                heat_low: "#4d7a45", heat_mid: "#9c6b17", heat_high: "#a3423a",
                cool_low: "#4f7f7a", cool_mid: "#3e6a87", cool_high: "#6d5a8c",
                sunset_low: "#6d5a8c", sunset_mid: "#9c4a6e", sunset_high: "#a8652a",
            },
            define! {
                bg: "#1d1c19", surface: "#262521", surface_2: "#34322d",
                text: "#e7e3d8", muted: "#a29c8e", border: "#3a3832", track: "#34322d",
                accent: "#8db2c8", accent_soft: "#2f3a40", on_accent: "#1d1c19",
                ok: "#93bd88", warn: "#d8ad62", error: "#d98b80", info: "#97b4c7",
                series_1: "#8db2c8", series_2: "#d8ad62", series_3: "#93bd88",
                series_4: "#d98b80", series_5: "#b4a2cf", series_6: "#8ac2bb",
                heat_low: "#93bd88", heat_mid: "#d8ad62", heat_high: "#d98b80",
                cool_low: "#8ac2bb", cool_mid: "#8db2c8", cool_high: "#b4a2cf",
                sunset_low: "#b4a2cf", sunset_mid: "#d48aa9", sunset_high: "#dda06a",
            },
        ),
        // Cream and coal with a burnt-orange accent.
        "ember" => (
            define! {
                bg: "#fbf5ef", surface: "#fffcf9", surface_2: "#f3e7dc",
                text: "#2a1c15", muted: "#76635a", border: "#ebdcd0", track: "#f0e3d7",
                accent: "#c2410c", accent_soft: "#f8dccb", on_accent: "#ffffff",
                ok: "#3f7a2e", warn: "#a86a00", error: "#b42318", info: "#2f6b8a",
                series_1: "#c2410c", series_2: "#a86a00", series_3: "#3f7a2e",
                series_4: "#2f6b8a", series_5: "#9b2c5a", series_6: "#7a5230",
                heat_low: "#3f7a2e", heat_mid: "#a86a00", heat_high: "#b42318",
                cool_low: "#2f7a72", cool_mid: "#2f6b8a", cool_high: "#6b3f8a",
                sunset_low: "#9b2c5a", sunset_mid: "#c2410c", sunset_high: "#c98a00",
            },
            define! {
                bg: "#15100d", surface: "#1f1814", surface_2: "#2f251f",
                text: "#f2e6dc", muted: "#b09a8c", border: "#3a2d25", track: "#33281f",
                accent: "#f28a4b", accent_soft: "#4a2a19", on_accent: "#15100d",
                ok: "#8fca6f", warn: "#f0b84a", error: "#f2736b", info: "#78b8d8",
                series_1: "#f28a4b", series_2: "#f0b84a", series_3: "#8fca6f",
                series_4: "#78b8d8", series_5: "#e07aa8", series_6: "#c9a27a",
                heat_low: "#8fca6f", heat_mid: "#f0b84a", heat_high: "#f2736b",
                cool_low: "#6fc9bd", cool_mid: "#78b8d8", cool_high: "#b48ce0",
                sunset_low: "#e07aa8", sunset_mid: "#f28a4b", sunset_high: "#f0c04a",
            },
        ),
        _ => return None,
    };
    Some(ColorTheme {
        name: name.to_string(),
        light,
        dark,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_variants() -> Vec<(String, &'static str, Tokens)> {
        builtins()
            .into_iter()
            .flat_map(|t| [(t.name.clone(), "light", t.light), (t.name, "dark", t.dark)])
            .collect()
    }

    #[test]
    fn every_builtin_has_both_variants_with_every_token_valid() {
        assert_eq!(builtins().len(), BUILTIN.len());
        for (name, mode, tokens) in all_variants() {
            for token in Tokens::NAMES {
                let value = tokens.get(token).unwrap();
                assert_eq!(
                    normalize_hex(value).as_deref(),
                    Some(value),
                    "{name}.{mode}.{token}: {value}"
                );
            }
        }
    }

    #[test]
    fn token_names_match_the_serialized_fields() {
        let json = serde_json::to_value(ColorTheme::default().light).unwrap();
        let keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(|k| k.as_str())
            .collect();
        let mut names = Tokens::NAMES.to_vec();
        names.sort();
        let mut keys = keys;
        keys.sort();
        assert_eq!(names, keys);
    }

    #[test]
    fn tones_are_tokens() {
        let t = ColorTheme::default();
        for i in 0..6 {
            assert!(t.light.get(Tone::series(i).token()).is_some());
        }
        for tone in [
            Tone::Accent,
            Tone::Ok,
            Tone::Warn,
            Tone::Error,
            Tone::Info,
            Tone::Muted,
        ] {
            assert!(t.light.get(tone.token()).is_some());
            let json = serde_json::to_value(tone).unwrap();
            assert_eq!(json, tone.token());
        }
        assert_eq!(
            serde_json::from_str::<Tone>("\"series-3\"").unwrap(),
            Tone::Series3
        );
    }

    /// Text must be easy to read on tiles, and quiet text still readable.
    #[test]
    fn builtins_are_readable() {
        for (name, mode, t) in all_variants() {
            let c = |a: &str, b: &str| contrast(rgb(a).unwrap(), rgb(b).unwrap());
            assert!(c(&t.text, &t.surface) >= 7.0, "{name} {mode}: text");
            assert!(c(&t.text, &t.bg) >= 7.0, "{name} {mode}: text on bg");
            assert!(c(&t.muted, &t.surface) >= 4.5, "{name} {mode}: muted");
            // The default keeps its original look, where a few of these are lower.
            if name != DEFAULT {
                assert!(
                    c(&t.on_accent, &t.accent) >= 3.0,
                    "{name} {mode}: on-accent"
                );
                for token in ["accent", "ok", "warn", "error", "info"] {
                    let v = t.get(token).unwrap();
                    assert!(c(v, &t.surface) >= 3.0, "{name} {mode}: {token}");
                }
            }
        }
    }

    /// The web build reads the built-ins from a generated file; regenerate it
    /// with `pnpm gen:themes` in web/ when a theme changes.
    #[test]
    fn web_themes_file_is_current() {
        let checked_in = include_str!("../../../web/src/themes.ts");
        assert!(
            checked_in == builtins_ts(),
            "web/src/themes.ts is out of date: run `pnpm gen:themes` in web/"
        );
    }

    #[test]
    fn hex_values_are_checked() {
        assert_eq!(normalize_hex("#ABC").as_deref(), Some("#aabbcc"));
        assert_eq!(normalize_hex("#00ff7F").as_deref(), Some("#00ff7f"));
        for bad in ["00ff7f", "#00ff7", "#gg0000", "red", "#00ff7f00", ""] {
            assert_eq!(normalize_hex(bad), None, "{bad}");
        }
        assert_eq!(rgb("#102030"), Some((16, 32, 48)));
    }

    #[test]
    fn overrides_replace_tokens_and_report_mistakes() {
        let t = ColorTheme::default()
            .with_overrides("mine", [("accent", "#F00")], [("series-2", "#00ff00")])
            .unwrap();
        assert_eq!(t.name, "mine");
        assert_eq!(t.light.accent, "#ff0000");
        assert_eq!(t.dark.series_2, "#00ff00");
        assert_eq!(t.dark.accent, ColorTheme::default().dark.accent);

        let err = ColorTheme::default()
            .with_overrides("x", [("accnt", "#fff")], [])
            .unwrap_err();
        assert!(err.starts_with("light.accnt: unknown token"), "{err}");
        let err = ColorTheme::default()
            .with_overrides("x", [], [("ok", "green")])
            .unwrap_err();
        assert_eq!(
            err,
            "dark.ok: \"green\" is not a color; use #rrggbb or #rgb"
        );
    }
}
