//! The agents bundle: usage of AI coding agents other than Claude Code (that
//! one is the cswap plugin), meaning how much of each limit is used and when it
//! resets, or for agents without limits, tokens and cost per day from their
//! local logs (see `tally`). Each module sits behind a cargo feature named
//! after its id.
//!
//! These are built in rather than plugins: each reads the agent's own local
//! state and needs no token in the config (rule 1 in
//! docs/modules.md). Cursor and Copilot also call a web
//! API, with the login their own app or CLI already keeps.

use dashboard_contract::{Status, Thresholds, Widget};
use dashboard_modules_common::Builtin;
#[allow(unused_imports)]
pub(crate) use dashboard_modules_common::{entry, schema, strings};

#[cfg(feature = "antigravity")]
pub mod antigravity;
#[cfg(feature = "codex")]
pub mod codex;
#[cfg(feature = "copilot")]
pub mod copilot;
#[cfg(feature = "cursor")]
pub mod cursor;
#[cfg(feature = "gemini")]
pub mod gemini;
#[cfg(feature = "opencode")]
pub mod opencode;
#[cfg(feature = "pi")]
pub mod pi;
pub mod tally;

/// The modules this build includes, in display order.
pub const MODULES: &[Builtin] = &[
    #[cfg(feature = "codex")]
    Builtin {
        id: "codex",
        core: false,
        new: || Box::new(codex::Codex::default()),
        catalog: codex::catalog,
    },
    #[cfg(feature = "copilot")]
    Builtin {
        id: "copilot",
        core: false,
        new: || Box::new(copilot::Copilot),
        catalog: copilot::catalog,
    },
    #[cfg(feature = "cursor")]
    Builtin {
        id: "cursor",
        core: false,
        new: || Box::new(cursor::Cursor::default()),
        catalog: cursor::catalog,
    },
    #[cfg(feature = "antigravity")]
    Builtin {
        id: "antigravity",
        core: false,
        new: || Box::new(antigravity::Antigravity),
        catalog: antigravity::catalog,
    },
    #[cfg(feature = "gemini")]
    Builtin {
        id: "gemini",
        core: false,
        new: || Box::new(gemini::Gemini::default()),
        catalog: gemini::catalog,
    },
    #[cfg(feature = "opencode")]
    Builtin {
        id: "opencode",
        core: false,
        new: || Box::new(opencode::OpenCode::default()),
        catalog: opencode::catalog,
    },
    #[cfg(feature = "pi")]
    Builtin {
        id: "pi",
        core: false,
        new: || Box::new(pi::Pi::default()),
        catalog: pi::catalog,
    },
];

/// Same levels as the cswap tile.
pub const THRESHOLDS: Thresholds = Thresholds {
    warn: 75.0,
    error: 90.0,
};

/// A limit window: share used, and when it resets.
pub struct Window {
    pub label: String,
    pub used_pct: f64,
    /// Seconds since the epoch.
    pub resets_at: Option<i64>,
    /// Window length in seconds, for the pace marker.
    pub length: Option<i64>,
    /// Shown before the reset time, e.g. "184 of 1500 left".
    pub note: Option<String>,
}

impl Window {
    /// A gauge whose marker shows where usage would be at an even pace.
    pub fn gauge(&self, now: i64) -> Widget {
        let left = self.resets_at.map(|r| r - now);
        let marker = match (left, self.length) {
            (Some(left), Some(len)) if len > 0 => {
                Some((100.0 * (1.0 - left as f64 / len as f64)).clamp(0.0, 100.0))
            }
            _ => None,
        };
        let mut caption: Vec<String> = self.note.iter().cloned().collect();
        if let Some(left) = left.filter(|l| *l > 0) {
            caption.push(format!("resets in {}", until(left)));
        }
        Widget::Gauge {
            label: self.label.clone(),
            percent: self.used_pct,
            marker,
            thresholds: Some(THRESHOLDS),
            caption: (!caption.is_empty()).then(|| caption.join(" · ")),
        }
    }

    pub fn status(&self) -> Status {
        if self.used_pct >= THRESHOLDS.error {
            Status::Error
        } else if self.used_pct >= THRESHOLDS.warn {
            Status::Warn
        } else {
            Status::Ok
        }
    }
}

/// The worst status of the windows, or Unknown when there are none.
pub fn worst(windows: &[Window]) -> Status {
    let rank = |s: Status| match s {
        Status::Error => 3,
        Status::Warn => 2,
        Status::Ok => 1,
        Status::Unknown => 0,
    };
    windows
        .iter()
        .map(Window::status)
        .max_by_key(|s| rank(*s))
        .unwrap_or(Status::Unknown)
}

/// "3d 4h", "4h 12m" or "12m".
pub fn until(secs: i64) -> String {
    let (d, h, m) = (secs / 86_400, secs % 86_400 / 3600, secs % 3600 / 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{}m", m.max(1))
    }
}

pub fn now_secs() -> i64 {
    dashboard_host::now_ms() / 1000
}

/// A one-row list that opens `url`: the way from a tile to the agent's own
/// usage page or docs.
pub fn link(title: &str, subtitle: &str, url: &str) -> Widget {
    Widget::List {
        items: vec![dashboard_contract::ListItem {
            title: title.into(),
            subtitle: Some(subtitle.into()),
            status: None,
            tags: vec![],
            action: Some(dashboard_contract::Action::OpenUrl { url: url.into() }),
        }],
        empty_text: None,
    }
}

/// Token counts as "4.2M".
pub fn count(n: u64) -> String {
    dashboard_contract::format_value(n as f64, dashboard_contract::ValueFormat::Count, None)
}

/// `2026-10-24` -> `Sat 24`.
pub fn day_label(date: &str) -> String {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    match dashboard_modules_common::time::parse_time(&format!("{date}T00:00:00Z")) {
        Some(t) => format!(
            "{} {}",
            DAYS[(t / 86_400).rem_euclid(7) as usize],
            date.get(8..10).unwrap_or("")
        ),
        None => date.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pace_marker_and_caption() {
        let w = Window {
            label: "Weekly".into(),
            used_pct: 40.0,
            resets_at: Some(1_000 + 2 * 86_400),
            length: Some(7 * 86_400),
            note: None,
        };
        let Widget::Gauge {
            marker, caption, ..
        } = w.gauge(1_000)
        else {
            panic!()
        };
        assert_eq!(marker.map(|m| m.round()), Some(71.0));
        assert_eq!(caption.as_deref(), Some("resets in 2d 0h"));
        assert_eq!(until(3 * 3600 + 5 * 60), "3h 5m");
    }
}
