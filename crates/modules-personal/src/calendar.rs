//! Today's agenda from an iCalendar (`.ics`) feed or file, with a "next
//! meeting in 12m" stat and each meeting's join link. Google Calendar,
//! Outlook and iCloud all publish a private `.ics` address, so there is no
//! sign-in. That address is a secret: give it through `url_env` or
//! `url_command`, not in the config file.
//!
//! ```yaml
//! modules:
//!   calendar:
//!     config:
//!       url_env: WORK_CALENDAR_ICS     # holds https://calendar.google.com/calendar/ical/.../basic.ics
//!   home-calendar:
//!     builtin: calendar
//!     config: { path: ~/Calendars/home.ics, days: 2 }
//! ```

use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use dashboard_contract::{
    Action, CatalogEntry, ListItem, Manifest, Requirements, Size, Status, View, Widget,
};
use dashboard_host::Module;
use dashboard_modules_common::http::{check, client};
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::ics::{self, Event, Occurrence};
use crate::{day_text, expand_home, manifest};

/// How long `url_command` may run.
const URL_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// The feed's URL (`webcal://` works too). Fine for a public calendar;
    /// for a private address use `url_env` or `url_command`.
    url: Option<String>,
    /// Environment variable holding the feed's URL.
    url_env: Option<String>,
    /// Command that prints the feed's URL, e.g. `["op", "read", "op://me/calendar/url"]`.
    url_command: Option<Vec<String>>,
    /// A local `.ics` file instead of a URL. `~/` is expanded.
    #[schemars(extend("x-path" = true))]
    path: Option<String>,
    /// Days to show, starting today.
    days: u32,
    /// Also list events that have already ended.
    show_past: bool,
    /// The tile turns yellow this many minutes before a meeting.
    warn_minutes: i64,
    /// Download the feed again after this many minutes. The countdown
    /// updates every refresh either way.
    fetch_minutes: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            url: None,
            url_env: None,
            url_command: None,
            path: None,
            days: 1,
            show_past: false,
            warn_minutes: 5,
            fetch_minutes: 10,
        }
    }
}

#[derive(Default)]
pub struct Calendar {
    config: Config,
    events: Vec<Event>,
    fetched: Option<Instant>,
}

#[async_trait]
impl Module for Calendar {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        let c = &self.config;
        if c.url.is_none() && c.url_env.is_none() && c.url_command.is_none() && c.path.is_none() {
            bail!(
                "the calendar module needs `config: {{ url_env: <VAR holding the .ics URL> }}` (or url, url_command, path)"
            );
        }
        // The countdown changes every minute; the feed is downloaded less often.
        Ok(manifest(
            "calendar",
            "Calendar",
            30_000,
            10_000,
            Size::Medium,
            20,
        ))
    }

    async fn render(&mut self) -> Result<View> {
        let tz = TimeZone::system();
        let every = Duration::from_secs(self.config.fetch_minutes.max(1) * 60);
        if self.fetched.is_none_or(|t| t.elapsed() >= every) {
            let text = self.load().await?;
            self.events = ics::parse(&text, &tz)?;
            self.fetched = Some(Instant::now());
        }
        let c = &self.config;
        Ok(agenda_view(
            &self.events,
            Timestamp::now(),
            &tz,
            c.days,
            c.show_past,
            c.warn_minutes,
        ))
    }
}

impl Calendar {
    /// The feed's text. Errors never include the URL: it may be a secret.
    async fn load(&self) -> Result<String> {
        if let Some(path) = &self.config.path {
            let path = expand_home(path);
            return std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()));
        }
        let url = self.url().await?;
        let url = match url.strip_prefix("webcal://") {
            Some(rest) => format!("https://{rest}"),
            None => url,
        };
        let resp = client()?
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow!("downloading the calendar: {}", e.without_url()))?;
        check(resp)
            .await?
            .text()
            .await
            .map_err(|e| anyhow!("downloading the calendar: {}", e.without_url()))
    }

    async fn url(&self) -> Result<String> {
        let c = &self.config;
        if let Some(argv) = &c.url_command {
            let (program, args) = argv.split_first().context("url_command is empty")?;
            let mut cmd = tokio::process::Command::new(program);
            cmd.args(args)
                .stdin(std::process::Stdio::null())
                .kill_on_drop(true);
            // A password manager waiting to be unlocked should not hang the tile.
            let out = tokio::time::timeout(URL_COMMAND_TIMEOUT, cmd.output())
                .await
                .map_err(|_| {
                    anyhow!(
                        "url_command {program:?} did not finish in {}s",
                        URL_COMMAND_TIMEOUT.as_secs()
                    )
                })?
                .with_context(|| format!("running url_command {program:?}"))?;
            if !out.status.success() {
                bail!("url_command {program:?} failed");
            }
            let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if url.is_empty() {
                bail!("url_command {program:?} printed nothing");
            }
            return Ok(url);
        }
        if let Some(var) = &c.url_env {
            return match std::env::var(var) {
                Ok(u) if !u.trim().is_empty() => Ok(u.trim().to_string()),
                _ => bail!("${var} is not set; set it to the calendar's .ics URL"),
            };
        }
        c.url.clone().context("no calendar URL")
    }
}

/// "now", "12m", "1h 5m".
fn until(secs: i64) -> String {
    let mins = (secs + 59) / 60;
    match mins {
        m if m < 1 => "now".into(),
        m if m < 60 => format!("in {m}m"),
        m if m % 60 == 0 => format!("in {}h", m / 60),
        m => format!("in {}h {}m", m / 60, m % 60),
    }
}

/// A short name for a join link's service, shown as a tag.
fn service(url: &str) -> &'static str {
    match url {
        u if u.contains("zoom.us") => "zoom",
        u if u.contains("meet.google.com") => "meet",
        u if u.contains("teams.") => "teams",
        u if u.contains("webex.com") => "webex",
        _ => "link",
    }
}

/// The agenda from `days` days starting today: a "next meeting" stat and the
/// list of events.
fn agenda_view(
    events: &[Event],
    now: Timestamp,
    tz: &TimeZone,
    days: u32,
    show_past: bool,
    warn_minutes: i64,
) -> View {
    let today = now.to_zoned(tz.clone()).date();
    let at = |d: jiff::civil::Date| d.to_zoned(tz.clone()).map(|z| z.timestamp()).ok();
    let from = at(today).unwrap_or(now);
    let to = today
        .checked_add(i64::from(days.max(1)).days())
        .ok()
        .and_then(at)
        .unwrap_or(now);
    let mut list: Vec<Occurrence> = ics::occurrences(events, from, to, tz)
        .into_iter()
        .filter(|o| show_past || o.end > now || o.start >= now)
        .collect();
    let day = |t: Timestamp| t.to_zoned(tz.clone()).date();
    list.sort_by_key(|o| (day(o.start).max(today), !o.all_day, o.start));

    let next = list.iter().find(|o| !o.all_day && o.start >= now);
    let secs = |o: &Occurrence| o.start.as_second() - now.as_second();
    let soon = |o: &Occurrence| !o.all_day && o.start >= now && secs(o) <= warn_minutes * 60;
    let stat = match next {
        Some(o) => {
            let when = if day(o.start) == today {
                until(secs(o))
            } else {
                format!(
                    "{} {}",
                    day_text(day(o.start), today),
                    o.start.to_zoned(tz.clone()).strftime("%H:%M")
                )
            };
            Widget::Stat {
                label: "Next meeting".into(),
                value: when,
                unit: None,
                delta: Some(o.event.summary.clone()),
                status: soon(o).then_some(Status::Warn),
            }
        }
        None => Widget::Stat {
            label: "Next meeting".into(),
            value: "none".into(),
            unit: None,
            delta: Some(if days <= 1 {
                "nothing else today".into()
            } else {
                format!("nothing in the next {days} days")
            }),
            status: None,
        },
    };

    let hm = |t: Timestamp| t.to_zoned(tz.clone()).strftime("%H:%M").to_string();
    let items = list
        .iter()
        .map(|o| {
            let mut sub = vec![];
            let d = day(o.start).max(today);
            let prefix = if d == today {
                String::new()
            } else {
                format!("{} ", day_text(d, today))
            };
            if o.all_day {
                sub.push(format!("{prefix}all day"));
            } else if o.end > o.start {
                sub.push(format!("{prefix}{}–{}", hm(o.start), hm(o.end)));
            } else {
                sub.push(format!("{prefix}{}", hm(o.start)));
            }
            // A location that is only a link is shown as the join tag instead.
            if let Some(loc) = o.event.location.as_deref()
                && !loc.starts_with("http")
            {
                sub.push(loc.to_string());
            }
            let link = o.event.join_link();
            let in_progress = !o.all_day && o.start <= now && now < o.end;
            ListItem {
                tone: None,
                title: o.event.summary.clone(),
                subtitle: Some(sub.join(" · ")),
                status: if in_progress {
                    Some(Status::Ok)
                } else if soon(o) {
                    Some(Status::Warn)
                } else {
                    None
                },
                tags: link.iter().map(|u| service(u).to_string()).collect(),
                action: link.map(|url| Action::OpenUrl { url }),
            }
        })
        .collect();

    let left = list.iter().filter(|o| !o.all_day).count();
    let summary = match next {
        Some(o) if day(o.start) == today => {
            format!("{} {} · {left} events", o.event.summary, until(secs(o)))
        }
        _ => format!("{left} events · no more meetings today"),
    };
    View {
        status: if next.is_some_and(soon) {
            Status::Warn
        } else {
            Status::Ok
        },
        summary,
        root: Widget::Column {
            children: vec![
                stat,
                Widget::List {
                    items,
                    empty_text: Some("no events".into()),
                },
            ],
        },
    }
}

pub fn catalog() -> CatalogEntry {
    // A hand-written feed with times around now, so the demo always reads
    // the same: one meeting under way, the next in 12 minutes.
    let now = Timestamp::now();
    let tz = TimeZone::system();
    let minute = now.as_second() / 60 * 60;
    let utc = |mins: i64| {
        Timestamp::from_second(minute + mins * 60)
            .unwrap_or(now)
            .strftime("%Y%m%dT%H%M%SZ")
            .to_string()
    };
    let today = now
        .to_zoned(tz.clone())
        .date()
        .strftime("%Y%m%d")
        .to_string();
    let feed = format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\n\
         BEGIN:VEVENT\r\nUID:release\r\nSUMMARY:Release day\r\nDTSTART;VALUE=DATE:{today}\r\nEND:VEVENT\r\n\
         BEGIN:VEVENT\r\nUID:review\r\nSUMMARY:Design review\r\nDTSTART:{}\r\nDTEND:{}\r\n\
         LOCATION:Room 4\r\nX-GOOGLE-CONFERENCE:https://meet.google.com/abc-defg-hij\r\nEND:VEVENT\r\n\
         BEGIN:VEVENT\r\nUID:standup\r\nSUMMARY:Standup\r\nDTSTART:{}\r\nDURATION:PT15M\r\n\
         RRULE:FREQ=DAILY\r\nLOCATION:https://acme.zoom.us/j/123456789\r\nEND:VEVENT\r\n\
         BEGIN:VEVENT\r\nUID:sam\r\nSUMMARY:1:1 with Sam\r\nDTSTART:{}\r\nDURATION:PT30M\r\n\
         DESCRIPTION:Join: https://teams.microsoft.com/l/meetup-join/19%3ameeting\r\nEND:VEVENT\r\n\
         BEGIN:VEVENT\r\nUID:focus\r\nSUMMARY:Focus time\r\nDTSTART:{}\r\nDURATION:PT2H\r\nEND:VEVENT\r\n\
         END:VCALENDAR\r\n",
        utc(-20),
        utc(25),
        utc(12 - 7 * 24 * 60),
        utc(150),
        utc(240),
    );
    let events = ics::parse(&feed, &tz).expect("demo feed parses");
    // Two days, so the demo still has meetings late in the evening.
    let demo = agenda_view(
        &events,
        Timestamp::from_second(minute).unwrap_or(now),
        &tz,
        2,
        false,
        5,
    );
    CatalogEntry {
        description: "Today's agenda from an iCalendar (.ics) feed or file, with the next \
                      meeting and its join link. Google, Outlook and iCloud all publish a \
                      private .ics address, so no sign-in is needed. Handles repeating events \
                      and time zones."
            .into(),
        tags: crate::strings(&["calendar", "meetings", "personal"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements {
            binaries: vec![],
            env: vec![],
            hosts: crate::strings(&["<your calendar's .ics host>"]),
        },
        ..crate::entry("calendar", "Calendar", Size::Medium, demo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    fn view_at(h: i8, m: i8) -> View {
        let tz = TimeZone::get("America/New_York").unwrap();
        let events = ics::parse(include_str!("../testdata/calendar/team.ics"), &tz).unwrap();
        let now = date(2026, 10, 27)
            .at(h, m, 0, 0)
            .to_zoned(tz.clone())
            .unwrap()
            .timestamp();
        agenda_view(&events, now, &tz, 1, false, 5)
    }

    fn parts(view: &View) -> (&Widget, &Vec<ListItem>) {
        let Widget::Column { children } = &view.root else {
            panic!()
        };
        let Widget::List { items, .. } = &children[1] else {
            panic!()
        };
        (&children[0], items)
    }

    #[test]
    fn next_meeting_and_join_links() {
        let view = view_at(9, 18);
        let (stat, items) = parts(&view);
        assert_eq!(
            stat,
            &Widget::Stat {
                label: "Next meeting".into(),
                value: "in 12m".into(),
                unit: None,
                delta: Some("Standup".into()),
                status: None,
            }
        );
        assert_eq!(view.summary, "Standup in 12m · 4 events");
        assert_eq!(view.status, Status::Ok);
        let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Release day",
                "Standup",
                "Design review, checkout flow",
                "London sync",
                "1:1 with Sam"
            ]
        );
        assert_eq!(items[0].subtitle.as_deref(), Some("all day"));
        assert_eq!(items[1].subtitle.as_deref(), Some("09:30–09:45"));
        assert_eq!(items[1].tags, ["zoom"]);
        assert_eq!(
            items[1].action,
            Some(Action::OpenUrl {
                url: "https://acme.zoom.us/j/123456789".into()
            })
        );
        assert_eq!(items[2].subtitle.as_deref(), Some("10:00–11:00 · Room 4"));
        assert_eq!(items[3].tags, ["teams"]);
        assert_eq!(items[4].tags, ["link"]);
    }

    #[test]
    fn soon_turns_yellow_and_in_progress_is_marked() {
        let view = view_at(9, 57);
        let (_, items) = parts(&view);
        assert_eq!(view.status, Status::Warn);
        // Standup is over and hidden; the review starts in 3 minutes.
        assert_eq!(items[1].title, "Design review, checkout flow");
        assert_eq!(items[1].status, Some(Status::Warn));

        let view = view_at(10, 30);
        let (_, items) = parts(&view);
        assert_eq!(items[1].status, Some(Status::Ok));
        assert_eq!(view.summary, "London sync in 30m · 3 events");
    }

    #[test]
    fn end_of_day() {
        let view = view_at(18, 0);
        let (stat, items) = parts(&view);
        let Widget::Stat { value, .. } = stat else {
            panic!()
        };
        assert_eq!(value, "none");
        assert_eq!(items.len(), 1); // the all-day event
        assert_eq!(view.summary, "0 events · no more meetings today");
    }

    #[test]
    fn countdown_text() {
        assert_eq!(until(30), "in 1m");
        assert_eq!(until(0), "now");
        assert_eq!(until(3600), "in 1h");
        assert_eq!(until(3900), "in 1h 5m");
    }
}
