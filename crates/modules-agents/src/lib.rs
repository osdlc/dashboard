//! The agents bundle: usage of AI coding agents other than Claude Code (that
//! one is the cswap plugin), meaning how much of each limit is used and when it
//! resets. Each module sits behind a cargo feature named after its id.

use dashboard_contract::{Status, Thresholds, Widget};
use dashboard_modules_common::Builtin;
#[allow(unused_imports)]
pub(crate) use dashboard_modules_common::{entry, schema, strings};

#[cfg(feature = "codex")]
pub mod codex;
#[cfg(feature = "copilot")]
pub mod copilot;

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
