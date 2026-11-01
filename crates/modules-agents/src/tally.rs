//! Token and cost totals from an agent's local logs: one record per model
//! reply, summed per day. Shared by the modules that read logs (OpenCode, Pi,
//! Gemini CLI). Days are UTC days, so for a US user "today" rolls over in the
//! afternoon or evening.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use dashboard_contract::{Bar, Status, ValueFormat, Widget};
use dashboard_modules_common::time::rfc3339;

use super::{count, day_label};

/// Days in the bars and the weekly totals.
pub const DAYS: i64 = 7;

/// One model reply.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// Seconds since the epoch.
    pub at: i64,
    pub tokens: u64,
    /// US dollars, when the agent logs a cost.
    pub cost: f64,
    pub model: String,
}

/// Totals over the last `DAYS` days.
#[derive(Debug, Default)]
pub struct Tally {
    /// (`YYYY-MM-DD`, tokens, cost), oldest first, one per day with zeros.
    pub days: Vec<(String, u64, f64)>,
    /// (model, tokens), most first.
    pub models: Vec<(String, u64)>,
    pub replies: usize,
    /// The newest reply read. Readers skip files untouched for `DAYS` days,
    /// so None means none in that time, not none ever.
    pub last: Option<i64>,
}

impl Tally {
    pub fn new(records: &[Record], now: i64) -> Tally {
        let today = now.div_euclid(86_400);
        let mut days: Vec<(String, u64, f64)> = (0..DAYS)
            .rev()
            .map(|ago| (date(today - ago), 0, 0.0))
            .collect();
        let mut models: BTreeMap<&str, u64> = BTreeMap::new();
        let mut replies = 0;
        for r in records {
            let ago = today - r.at.div_euclid(86_400);
            if !(0..DAYS).contains(&ago) {
                continue;
            }
            let day = &mut days[(DAYS - 1 - ago) as usize];
            day.1 += r.tokens;
            day.2 += r.cost;
            *models.entry(r.model.as_str()).or_default() += r.tokens;
            replies += 1;
        }
        let mut models: Vec<(String, u64)> = models
            .into_iter()
            .map(|(m, t)| (m.to_string(), t))
            .collect();
        models.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        Tally {
            days,
            models,
            replies,
            last: records.iter().map(|r| r.at).max(),
        }
    }

    pub fn today(&self) -> u64 {
        self.days.last().map_or(0, |d| d.1)
    }

    pub fn week_tokens(&self) -> u64 {
        self.days.iter().map(|d| d.1).sum()
    }

    pub fn week_cost(&self) -> f64 {
        self.days.iter().map(|d| d.2).sum()
    }

    /// Ok when the agent was used this week, else Unknown: an idle agent is
    /// not a problem.
    pub fn status(&self) -> Status {
        if self.replies > 0 {
            Status::Ok
        } else {
            Status::Unknown
        }
    }

    /// "1.2M today · 8.4M this week · $3.40".
    pub fn summary(&self) -> String {
        let mut parts = vec![
            format!("{} today", count(self.today())),
            format!("{} this week", count(self.week_tokens())),
        ];
        if self.week_cost() > 0.0 {
            parts.push(dollars(self.week_cost()));
        }
        parts.join(" · ")
    }

    /// Stats for today and the week, tokens per day, and the top models.
    pub fn widgets(&self, now: i64) -> Vec<Widget> {
        let mut stats = vec![
            stat("Today", count(self.today())),
            stat("7 days", count(self.week_tokens())),
        ];
        if self.week_cost() > 0.0 {
            stats.push(stat("Cost 7 days", dollars(self.week_cost())));
        }
        let mut out = vec![
            Widget::Row { children: stats },
            Widget::Bars {
                label: Some("Tokens per day".into()),
                unit: None,
                format: ValueFormat::Count,
                max: None,
                bars: self
                    .days
                    .iter()
                    .map(|(date, tokens, _)| Bar {
                        label: day_label(date),
                        value: *tokens as f64,
                        status: None,
                    })
                    .collect(),
            },
        ];
        let mut facts = vec![];
        let total = self.week_tokens().max(1) as f64;
        let top: Vec<String> = self
            .models
            .iter()
            .take(3)
            .map(|(m, t)| format!("{m} {:.0}%", 100.0 * *t as f64 / total))
            .collect();
        if !top.is_empty() {
            facts.push(top.join(", "));
        }
        match self.last {
            Some(t) => facts.push(format!("last used {}", age(now - t))),
            None => facts.push(format!("not used in the last {DAYS} days")),
        }
        out.push(Widget::Text {
            text: facts.join(" · "),
            markdown: false,
        });
        out
    }
}

fn stat(label: &str, value: String) -> Widget {
    Widget::Stat {
        label: label.into(),
        value,
        unit: None,
        delta: None,
        status: None,
    }
}

/// "$3.40".
pub fn dollars(d: f64) -> String {
    format!("${d:.2}")
}

/// `YYYY-MM-DD` of a day number (days since the epoch).
fn date(day: i64) -> String {
    rfc3339(day * 86_400)[..10].to_string()
}

fn age(secs: i64) -> String {
    match secs.max(0) {
        s if s < 60 => "just now".into(),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86_400),
    }
}

/// Made-up replies for a catalog demo: a week of uneven use split between
/// `models` (name, share), with costs when `costs` is set.
pub fn demo_records(now: i64, models: &[(&str, f64)], costs: bool) -> Vec<Record> {
    const PER_DAY: [u64; 7] = [1_420_000, 870_000, 2_350_000, 0, 96_000, 1_180_000, 412_000];
    let mut out = vec![];
    for (ago, tokens) in PER_DAY.iter().enumerate() {
        for (model, share) in models {
            let tokens = (*tokens as f64 * share) as u64;
            if tokens == 0 {
                continue;
            }
            out.push(Record {
                // This time of day on that day, and an hour ago for today.
                at: if ago == 0 {
                    now - 3600
                } else {
                    now - ago as i64 * 86_400
                },
                tokens,
                cost: if costs { tokens as f64 * 2.5e-6 } else { 0.0 },
                model: model.to_string(),
            });
        }
    }
    out
}

/// Files under `dir` (recursively) with one of `exts`, changed in the last
/// `DAYS` days, so a long history is not read on every refresh.
pub fn recent_files(dir: &Path, exts: &[&str], now: i64) -> Vec<PathBuf> {
    let since = SystemTime::UNIX_EPOCH + Duration::from_secs((now - DAYS * 86_400).max(0) as u64);
    let mut out = vec![];
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let path = e.path();
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| exts.contains(&x))
                && meta.modified().is_ok_and(|m| m >= since)
            {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use dashboard_modules_common::time::parse_time;

    #[test]
    fn sums_per_day_and_model() {
        let now = parse_time("2026-10-27T12:00:00Z").unwrap();
        let at = |ts: &str| parse_time(ts).unwrap();
        let records = [
            Record {
                at: at("2026-10-27T01:00:00Z"),
                tokens: 1_000,
                cost: 0.5,
                model: "a".into(),
            },
            Record {
                at: at("2026-10-26T23:00:00Z"),
                tokens: 3_000,
                cost: 0.25,
                model: "b".into(),
            },
            // Older than a week: not counted, but still the "last" candidate.
            Record {
                at: at("2026-10-01T00:00:00Z"),
                tokens: 9_999,
                cost: 9.0,
                model: "c".into(),
            },
        ];
        let t = Tally::new(&records, now);
        assert_eq!(t.days.len(), 7);
        assert_eq!(t.days[6].0, "2026-10-27");
        assert_eq!(t.today(), 1_000);
        assert_eq!(t.week_tokens(), 4_000);
        assert_eq!(t.models[0], ("b".to_string(), 3_000));
        assert_eq!(t.summary(), "1.0k today · 4.0k this week · $0.75");
        assert_eq!(t.status(), Status::Ok);
        assert_eq!(Tally::new(&[], now).status(), Status::Unknown);
    }
}
