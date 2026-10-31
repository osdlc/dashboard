//! A small iCalendar (RFC 5545) reader: the events of an `.ics` feed, their
//! time zones (IANA names, and the Windows names Outlook uses), and the
//! common repeat rules, expanded over a window of time.
//!
//! Repeat rules cover FREQ (daily, weekly, monthly, yearly), INTERVAL, COUNT,
//! UNTIL, BYDAY (with `1MO` / `-1FR` in monthly and yearly rules), BYMONTHDAY,
//! BYMONTH, BYSETPOS and WKST, plus EXDATE and moved or cancelled single
//! occurrences (RECURRENCE-ID). A rule with any other part (BYWEEKNO,
//! BYYEARDAY, ...) is not expanded: the event shows once, at its start,
//! rather than on wrong days. RDATE is left out.

use std::collections::{HashMap, HashSet};

use anyhow::{Result, bail};
use jiff::civil::{Date, DateTime, Time, Weekday};
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};

/// A start time as the feed gives it.
#[derive(Debug, Clone, PartialEq)]
pub enum Start {
    /// An all-day event's date.
    Date(Date),
    /// A wall-clock time in a zone (UTC for `...Z`, the system zone for
    /// floating times).
    At(DateTime, TimeZone),
}

impl Start {
    /// The instant it starts. All-day events start at midnight in `local`.
    fn instant(&self, local: &TimeZone) -> Option<Timestamp> {
        let zoned = match self {
            Start::Date(d) => d.to_datetime(Time::midnight()).to_zoned(local.clone()),
            Start::At(dt, tz) => dt.to_zoned(tz.clone()),
        };
        zoned.ok().map(|z| z.timestamp())
    }

    /// Compares an EXDATE or RECURRENCE-ID with an occurrence: the instant,
    /// or the date for all-day events.
    fn key(&self) -> i64 {
        match self {
            Start::Date(d) => i64::from((*d - Date::constant(1970, 1, 1)).get_days()),
            Start::At(..) => self
                .instant(&TimeZone::UTC)
                .map_or(i64::MIN, |t| t.as_second()),
        }
    }

    fn date(&self) -> Date {
        match self {
            Start::Date(d) => *d,
            Start::At(dt, _) => dt.date(),
        }
    }

    /// The same kind of start on another day.
    fn on(&self, date: Date) -> Start {
        match self {
            Start::Date(_) => Start::Date(date),
            Start::At(dt, tz) => Start::At(date.to_datetime(dt.time()), tz.clone()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Event {
    pub uid: String,
    pub summary: String,
    pub location: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
    /// A video call link from a calendar-specific property.
    pub conference: Option<String>,
    pub start: Start,
    /// Seconds for timed events, days for all-day ones.
    pub length: i64,
    pub rrule: Option<RRule>,
    pub exdates: Vec<Start>,
    /// Set on an edited single occurrence of a repeating event.
    pub recurrence_id: Option<Start>,
    pub cancelled: bool,
}

impl Event {
    /// The video call link: a calendar's own conference property, else the
    /// first meeting link in the location or description, else the event URL.
    pub fn join_link(&self) -> Option<String> {
        const HOSTS: [&str; 10] = [
            "zoom.us",
            "meet.google.com",
            "teams.microsoft.com",
            "teams.live.com",
            "webex.com",
            "whereby.com",
            "meet.jit.si",
            "chime.aws",
            "gotomeeting.com",
            "around.co",
        ];
        let meeting = |text: &Option<String>| {
            urls(text.as_deref()?)
                .into_iter()
                .find(|u| HOSTS.iter().any(|h| u.contains(h)))
        };
        self.conference
            .clone()
            .or_else(|| meeting(&self.location))
            .or_else(|| meeting(&self.description))
            .or_else(|| self.url.clone())
    }
}

/// The http(s) links in some text.
fn urls(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || "<>\"'()[]".contains(c))
        .filter(|w| w.starts_with("https://") || w.starts_with("http://"))
        .map(|w| w.trim_end_matches(['.', ',', ';']).to_string())
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RRule {
    pub freq: Freq,
    pub interval: i64,
    pub count: Option<u32>,
    pub until: Option<Start>,
    /// Weekdays, with an optional position in the month or year (`-1FR`).
    pub by_day: Vec<(i8, Weekday)>,
    pub by_month_day: Vec<i8>,
    pub by_month: Vec<i8>,
    /// Picks from each period's dates: 1 is the first, -1 the last.
    pub by_set_pos: Vec<i32>,
    pub week_start: Weekday,
}

/// One occurrence in a window.
#[derive(Debug, Clone)]
pub struct Occurrence<'a> {
    pub event: &'a Event,
    pub start: Timestamp,
    pub end: Timestamp,
    pub all_day: bool,
}

/// Parses every VEVENT. `local` is the zone for floating times when the
/// feed names none (`X-WR-TIMEZONE`).
pub fn parse(text: &str, local: &TimeZone) -> Result<Vec<Event>> {
    // Outlook exports often start with a byte order mark.
    if !text
        .trim_start_matches('\u{feff}')
        .trim_start()
        .starts_with("BEGIN:VCALENDAR")
    {
        bail!("not an iCalendar file (no BEGIN:VCALENDAR)");
    }
    let mut floating = local.clone();
    let mut events = vec![];
    let mut props: Option<Vec<Prop>> = None;
    let mut depth = 0;
    for line in unfold(text) {
        let Some(prop) = Prop::parse(&line) else {
            continue;
        };
        match (prop.name.as_str(), prop.value.as_str()) {
            ("BEGIN", "VEVENT") => props = Some(vec![]),
            ("END", "VEVENT") => {
                if let Some(p) = props.take()
                    && let Some(e) = event(&p, &floating)
                {
                    events.push(e);
                }
            }
            // Alarms inside an event have their own SUMMARY and DESCRIPTION.
            ("BEGIN", _) if props.is_some() => depth += 1,
            ("END", _) if props.is_some() => depth -= 1,
            ("X-WR-TIMEZONE", name) if props.is_none() => {
                if let Some(tz) = zone(name) {
                    floating = tz;
                }
            }
            _ => {
                if let Some(p) = props.as_mut()
                    && depth == 0
                {
                    p.push(prop);
                }
            }
        }
    }
    Ok(events)
}

/// Lines with RFC 5545 folding undone (a continuation starts with a space or tab).
fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = vec![];
    for raw in text.lines() {
        let raw = raw.trim_end_matches('\r');
        match (raw.strip_prefix([' ', '\t']), lines.last_mut()) {
            (Some(rest), Some(last)) => last.push_str(rest),
            _ => lines.push(raw.to_string()),
        }
    }
    lines
}

struct Prop {
    name: String,
    params: Vec<(String, String)>,
    value: String,
}

impl Prop {
    /// `NAME;PARAM=x;PARAM="y:z":value`.
    fn parse(line: &str) -> Option<Prop> {
        let mut quoted = false;
        let colon = line.char_indices().find_map(|(i, c)| match c {
            '"' => {
                quoted = !quoted;
                None
            }
            ':' if !quoted => Some(i),
            _ => None,
        })?;
        let (head, value) = (&line[..colon], &line[colon + 1..]);
        let mut parts = head.split(';');
        let name = parts.next()?.trim().to_ascii_uppercase();
        let params = parts
            .filter_map(|p| p.split_once('='))
            .map(|(k, v)| (k.to_ascii_uppercase(), v.trim_matches('"').to_string()))
            .collect();
        Some(Prop {
            name,
            params,
            value: value.to_string(),
        })
    }

    fn param(&self, key: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// The value as text, with `\n`, `\,` and `\;` undone.
    fn text(&self) -> String {
        let mut out = String::new();
        let mut chars = self.value.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n' | 'N') => out.push('\n'),
                    Some(other) => out.push(other),
                    None => {}
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    fn start(&self, floating: &TimeZone) -> Option<Start> {
        let tz = match self.param("TZID") {
            Some(name) => zone(name).unwrap_or_else(|| floating.clone()),
            None => floating.clone(),
        };
        start(self.value.trim(), tz)
    }
}

/// `20261027` or `20261027T093000` (in `tz`) or `20261027T093000Z` (UTC).
fn start(value: &str, tz: TimeZone) -> Option<Start> {
    let n = |r: std::ops::Range<usize>| value.get(r)?.parse::<i16>().ok();
    let date = Date::new(n(0..4)?, n(4..6)? as i8, n(6..8)? as i8).ok()?;
    if value.len() == 8 {
        return Some(Start::Date(date));
    }
    if value.as_bytes().get(8) != Some(&b'T') {
        return None;
    }
    let time = Time::new(n(9..11)? as i8, n(11..13)? as i8, n(13..15)? as i8, 0).ok()?;
    let tz = if value.ends_with('Z') {
        TimeZone::UTC
    } else {
        tz
    };
    Some(Start::At(date.to_datetime(time), tz))
}

/// A time zone by IANA name (also inside a longer TZID such as
/// `/mozilla.org/20050126_1/America/New_York`) or Windows name.
fn zone(name: &str) -> Option<TimeZone> {
    let name = name.trim();
    if let Ok(tz) = TimeZone::get(name) {
        return Some(tz);
    }
    // Try the path's tails: "a/b/Europe/London" -> "b/Europe/London" -> "Europe/London".
    let mut rest = name;
    while let Some((_, tail)) = rest.split_once('/') {
        if let Ok(tz) = TimeZone::get(tail) {
            return Some(tz);
        }
        rest = tail;
    }
    let iana = match name {
        "UTC" | "Coordinated Universal Time" => "UTC",
        "Eastern Standard Time" => "America/New_York",
        "Central Standard Time" => "America/Chicago",
        "Mountain Standard Time" => "America/Denver",
        "US Mountain Standard Time" => "America/Phoenix",
        "Pacific Standard Time" => "America/Los_Angeles",
        "Alaskan Standard Time" => "America/Anchorage",
        "Hawaiian Standard Time" => "Pacific/Honolulu",
        "Atlantic Standard Time" => "America/Halifax",
        "E. South America Standard Time" => "America/Sao_Paulo",
        "GMT Standard Time" => "Europe/London",
        "W. Europe Standard Time" => "Europe/Berlin",
        "Romance Standard Time" => "Europe/Paris",
        "Central Europe Standard Time" => "Europe/Budapest",
        "Central European Standard Time" => "Europe/Warsaw",
        "FLE Standard Time" => "Europe/Helsinki",
        "GTB Standard Time" => "Europe/Bucharest",
        "Israel Standard Time" => "Asia/Jerusalem",
        "India Standard Time" => "Asia/Kolkata",
        "China Standard Time" => "Asia/Shanghai",
        "Singapore Standard Time" => "Asia/Singapore",
        "Tokyo Standard Time" => "Asia/Tokyo",
        "AUS Eastern Standard Time" => "Australia/Sydney",
        "New Zealand Standard Time" => "Pacific/Auckland",
        _ => return None,
    };
    TimeZone::get(iana).ok()
}

/// `PT1H30M`, `P1D`, `P2W` in seconds.
fn duration(value: &str) -> Option<i64> {
    let rest = value
        .trim()
        .trim_start_matches(['+', '-'])
        .strip_prefix('P')?;
    let mut secs = 0;
    let mut num = String::new();
    for c in rest.chars() {
        match c {
            '0'..='9' => num.push(c),
            'T' => {}
            unit => {
                let n: i64 = num.parse().ok()?;
                num.clear();
                secs += n * match unit {
                    'W' => 7 * 86_400,
                    'D' => 86_400,
                    'H' => 3600,
                    'M' => 60,
                    'S' => 1,
                    _ => return None,
                };
            }
        }
    }
    Some(secs)
}

fn event(props: &[Prop], floating: &TimeZone) -> Option<Event> {
    let get = |name: &str| props.iter().find(|p| p.name == name);
    let text = |name: &str| get(name).map(Prop::text).filter(|t| !t.trim().is_empty());
    let start = get("DTSTART")?.start(floating)?;
    let length = match (&start, get("DTEND").and_then(|p| p.start(floating))) {
        (Start::Date(s), Some(Start::Date(e))) => i64::from((e - *s).get_days()).max(1),
        (Start::At(..), Some(end @ Start::At(..))) => {
            let (s, e) = (start.instant(floating)?, end.instant(floating)?);
            (e.as_second() - s.as_second()).max(0)
        }
        _ => match (&start, get("DURATION").and_then(|p| duration(&p.value))) {
            (Start::Date(_), Some(secs)) => (secs / 86_400).max(1),
            (Start::Date(_), None) => 1,
            (Start::At(..), secs) => secs.unwrap_or(0),
        },
    };
    let exdates = props
        .iter()
        .filter(|p| p.name == "EXDATE")
        .flat_map(|p| {
            p.value.split(',').filter_map(|v| {
                Prop {
                    name: String::new(),
                    params: p.params.clone(),
                    value: v.to_string(),
                }
                .start(floating)
            })
        })
        .collect();
    Some(Event {
        uid: text("UID").unwrap_or_default(),
        summary: text("SUMMARY").unwrap_or_else(|| "(no title)".into()),
        location: text("LOCATION"),
        description: text("DESCRIPTION"),
        url: text("URL").filter(|u| u.starts_with("http")),
        conference: text("X-GOOGLE-CONFERENCE")
            .or_else(|| text("X-MICROSOFT-SKYPETEAMSMEETINGURL"))
            .filter(|u| u.starts_with("http")),
        rrule: get("RRULE").and_then(|p| rrule(&p.value, floating)),
        exdates,
        recurrence_id: get("RECURRENCE-ID").and_then(|p| p.start(floating)),
        cancelled: get("STATUS").is_some_and(|p| p.value.eq_ignore_ascii_case("CANCELLED")),
        start,
        length,
    })
}

fn weekday(code: &str) -> Option<Weekday> {
    Some(match code {
        "MO" => Weekday::Monday,
        "TU" => Weekday::Tuesday,
        "WE" => Weekday::Wednesday,
        "TH" => Weekday::Thursday,
        "FR" => Weekday::Friday,
        "SA" => Weekday::Saturday,
        "SU" => Weekday::Sunday,
        _ => return None,
    })
}

/// `FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE;UNTIL=20261231T000000Z`.
fn rrule(value: &str, floating: &TimeZone) -> Option<RRule> {
    let mut rule = RRule {
        freq: Freq::Daily,
        interval: 1,
        count: None,
        until: None,
        by_day: vec![],
        by_month_day: vec![],
        by_month: vec![],
        by_set_pos: vec![],
        week_start: Weekday::Monday,
    };
    let mut freq = None;
    let list = |v: &str| -> Vec<i8> { v.split(',').filter_map(|n| n.parse().ok()).collect() };
    for part in value.split(';') {
        let (k, v) = part.split_once('=')?;
        match k.to_ascii_uppercase().as_str() {
            "FREQ" => {
                freq = Some(match v {
                    "DAILY" => Freq::Daily,
                    "WEEKLY" => Freq::Weekly,
                    "MONTHLY" => Freq::Monthly,
                    "YEARLY" => Freq::Yearly,
                    // Hourly and finer don't fit an agenda.
                    _ => return None,
                })
            }
            "INTERVAL" => rule.interval = v.parse::<i64>().ok()?.max(1),
            "COUNT" => rule.count = v.parse().ok(),
            "UNTIL" => rule.until = start(v, floating.clone()),
            "BYDAY" => {
                rule.by_day = v
                    .split(',')
                    .filter_map(|d| {
                        let (pos, code) = d.split_at(d.len().checked_sub(2)?);
                        Some((pos.parse().unwrap_or(0), weekday(code)?))
                    })
                    .collect()
            }
            "BYMONTHDAY" => rule.by_month_day = list(v),
            "BYMONTH" => rule.by_month = list(v),
            "BYSETPOS" => rule.by_set_pos = v.split(',').filter_map(|n| n.parse().ok()).collect(),
            "WKST" => rule.week_start = weekday(v).unwrap_or(Weekday::Monday),
            // Expanding without a part would put the event on wrong days.
            _ => return None,
        }
    }
    rule.freq = freq?;
    // In a yearly rule without BYMONTH, BYDAY and BYMONTHDAY count over the
    // whole year (`20MO`, every 15th), which month_days does not do.
    if rule.freq == Freq::Yearly
        && rule.by_month.is_empty()
        && !(rule.by_day.is_empty() && rule.by_month_day.is_empty())
    {
        return None;
    }
    Some(rule)
}

/// How many periods (days, weeks, ...) a rule is followed before giving up:
/// a daily event can go back decades.
const MAX_PERIODS: i64 = 40_000;

/// The days a rule picks in one month. With both BYMONTHDAY and BYDAY, a
/// day must match both (Friday the 13th).
fn month_days(first: Date, rule: &RRule, day: i8) -> Vec<Date> {
    let dim = first.days_in_month();
    let on = |d: i8| first.with().day(d).build().ok();
    let by_month_day: Vec<Date> = rule
        .by_month_day
        .iter()
        .filter_map(|&d| on(if d < 0 { dim + d + 1 } else { d }))
        .collect();
    let mut by_day = vec![];
    for &(pos, wd) in &rule.by_day {
        if pos == 0 {
            by_day.extend((1..=dim).filter_map(on).filter(|d| d.weekday() == wd));
        } else if let Ok(d) = first.nth_weekday_of_month(pos, wd) {
            by_day.push(d);
        }
    }
    match (rule.by_month_day.is_empty(), rule.by_day.is_empty()) {
        (true, true) => on(day).into_iter().collect(),
        (true, false) => by_day,
        (false, true) => by_month_day,
        (false, false) => by_month_day
            .into_iter()
            .filter(|d| by_day.contains(d))
            .collect(),
    }
}

/// Start dates a rule makes in period `n` (0 is the period of the first start).
fn period_dates(first: Date, rule: &RRule, n: i64) -> Vec<Date> {
    let step = n * rule.interval;
    let weekdays: Vec<Weekday> = rule.by_day.iter().map(|(_, w)| *w).collect();
    let mut dates = match rule.freq {
        Freq::Daily => first
            .checked_add(step.days())
            .ok()
            .filter(|d| weekdays.is_empty() || weekdays.contains(&d.weekday()))
            .filter(|d| {
                let dim = d.days_in_month();
                rule.by_month_day.is_empty()
                    || rule
                        .by_month_day
                        .iter()
                        .any(|&m| d.day() == if m < 0 { dim + m + 1 } else { m })
            })
            .into_iter()
            .collect(),
        Freq::Weekly => {
            let back = (first.weekday().to_monday_zero_offset()
                - rule.week_start.to_monday_zero_offset())
            .rem_euclid(7);
            let Ok(week) = first.checked_add((step * 7 - i64::from(back)).days()) else {
                return vec![];
            };
            let wanted = if weekdays.is_empty() {
                vec![first.weekday()]
            } else {
                weekdays
            };
            (0..7)
                .filter_map(|i| week.checked_add(i.days()).ok())
                .filter(|d| wanted.contains(&d.weekday()))
                .collect()
        }
        Freq::Monthly => match first.first_of_month().checked_add(step.months()) {
            Ok(month) => month_days(month, rule, first.day()),
            Err(_) => vec![],
        },
        Freq::Yearly => {
            let year = i64::from(first.year()) + step;
            let months = if rule.by_month.is_empty() {
                vec![first.month()]
            } else {
                rule.by_month.clone()
            };
            months
                .into_iter()
                .filter_map(|m| Date::new(i16::try_from(year).ok()?, m, 1).ok())
                .flat_map(|month| month_days(month, rule, first.day()))
                .collect()
        }
    };
    if !rule.by_month.is_empty() && rule.freq != Freq::Yearly {
        dates.retain(|d| rule.by_month.contains(&d.month()));
    }
    dates.sort();
    dates.dedup();
    if !rule.by_set_pos.is_empty() {
        let len = dates.len() as i64;
        let mut picked: Vec<Date> = rule
            .by_set_pos
            .iter()
            .filter_map(|&pos| {
                let i = if pos > 0 {
                    i64::from(pos) - 1
                } else {
                    len + i64::from(pos)
                };
                usize::try_from(i).ok().and_then(|i| dates.get(i)).copied()
            })
            .collect();
        picked.sort();
        picked.dedup();
        return picked;
    }
    dates
}

/// The period a date falls in (0 is the period of `first`), rounded down.
/// Used to skip ahead to a window instead of walking from the first start.
fn period_of(first: Date, rule: &RRule, date: Date) -> i64 {
    let periods = match rule.freq {
        Freq::Daily => i64::from((date - first).get_days()),
        Freq::Weekly => i64::from((date - first).get_days()) / 7,
        Freq::Monthly => {
            (i64::from(date.year()) - i64::from(first.year())) * 12 + i64::from(date.month())
                - i64::from(first.month())
        }
        Freq::Yearly => i64::from(date.year()) - i64::from(first.year()),
    };
    periods.div_euclid(rule.interval)
}

/// The starts of a repeating event, in order, up to `to`. Without COUNT,
/// starts well before `from` are skipped (the result may still hold some).
fn repeat(
    event: &Event,
    rule: &RRule,
    from: Timestamp,
    to: Timestamp,
    local: &TimeZone,
) -> Vec<Start> {
    let first = event.start.date();
    let until = match &rule.until {
        // A date-only UNTIL on a timed event covers that whole day.
        Some(Start::Date(d)) if matches!(event.start, Start::At(..)) => d
            .tomorrow()
            .ok()
            .and_then(|d| Start::Date(d).instant(local))
            .and_then(|t| t.checked_sub(1.second()).ok()),
        u => u.as_ref().and_then(|u| u.instant(local)),
    };
    // COUNT needs every start from the first; otherwise jump to the period
    // a little before `from`, less the event's length, so a long event that
    // began earlier still overlaps.
    let skip = if rule.count.is_some() {
        0
    } else {
        let back = match event.start {
            Start::Date(_) => event.length + 1,
            Start::At(..) => event.length / 86_400 + 2,
        };
        from.to_zoned(local.clone())
            .date()
            .checked_sub(back.days())
            .map_or(0, |d| (period_of(first, rule, d) - 1).max(0))
    };
    let mut out = vec![];
    let mut count = 0;
    for n in skip..skip + MAX_PERIODS {
        for date in period_dates(first, rule, n) {
            if date < first {
                continue;
            }
            let start = event.start.on(date);
            let Some(at) = start.instant(local) else {
                continue;
            };
            if until.is_some_and(|u| at > u) || at >= to {
                return out;
            }
            count += 1;
            out.push(start);
            if rule.count.is_some_and(|c| count >= c) {
                return out;
            }
        }
    }
    out
}

/// Every occurrence that overlaps `[from, to)`, by start time. All-day events
/// are placed in `local`.
pub fn occurrences<'a>(
    events: &'a [Event],
    from: Timestamp,
    to: Timestamp,
    local: &TimeZone,
) -> Vec<Occurrence<'a>> {
    // Single occurrences that were moved or cancelled, by event.
    let mut moved: HashMap<&str, HashSet<i64>> = HashMap::new();
    for e in events {
        if let Some(id) = &e.recurrence_id {
            moved.entry(&e.uid).or_default().insert(id.key());
        }
    }
    let mut out = vec![];
    for e in events {
        if e.cancelled {
            continue;
        }
        let starts = match (&e.rrule, &e.recurrence_id) {
            (Some(rule), None) => {
                let skip: HashSet<i64> = e
                    .exdates
                    .iter()
                    .map(Start::key)
                    .chain(moved.get(e.uid.as_str()).into_iter().flatten().copied())
                    .collect();
                repeat(e, rule, from, to, local)
                    .into_iter()
                    .filter(|s| !skip.contains(&s.key()))
                    .collect()
            }
            _ => vec![e.start.clone()],
        };
        for s in starts {
            let Some(start) = s.instant(local) else {
                continue;
            };
            let all_day = matches!(s, Start::Date(_));
            let end = if let Start::Date(d) = s {
                d.checked_add(e.length.days())
                    .ok()
                    .and_then(|end| Start::Date(end).instant(local))
            } else {
                start.checked_add(e.length.seconds()).ok()
            };
            let end = end.unwrap_or(start);
            // A zero-length event still shows at its start.
            if start < to && (end > from || (end == start && start >= from)) {
                out.push(Occurrence {
                    event: e,
                    start,
                    end,
                    all_day,
                });
            }
        }
    }
    out.sort_by_key(|o| (!o.all_day, o.start, o.end));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    fn ny() -> TimeZone {
        TimeZone::get("America/New_York").unwrap()
    }

    /// Occurrences on one New York day, as "HH:MM summary".
    fn day(events: &[Event], d: Date) -> Vec<String> {
        let tz = ny();
        let from = d.to_zoned(tz.clone()).unwrap();
        let to = from.tomorrow().unwrap();
        occurrences(events, from.timestamp(), to.timestamp(), &tz)
            .into_iter()
            .map(|o| {
                if o.all_day {
                    format!("all-day {}", o.event.summary)
                } else {
                    let t = o.start.to_zoned(tz.clone());
                    format!("{} {}", t.strftime("%H:%M"), o.event.summary)
                }
            })
            .collect()
    }

    fn sample() -> Vec<Event> {
        parse(include_str!("../testdata/calendar/team.ics"), &ny()).unwrap()
    }

    #[test]
    fn reads_the_events() {
        let events = sample();
        let standup = events.iter().find(|e| e.summary == "Standup").unwrap();
        assert_eq!(standup.length, 15 * 60);
        assert_eq!(
            standup.join_link().as_deref(),
            Some("https://acme.zoom.us/j/123456789")
        );
        // Folded lines and escaped commas.
        let review = events.iter().find(|e| e.uid == "review@acme").unwrap();
        assert_eq!(review.summary, "Design review, checkout flow");
        assert_eq!(
            review.join_link().as_deref(),
            Some("https://meet.google.com/abc-defg-hij")
        );
        // The alarm's DESCRIPTION is not the event's.
        assert_eq!(review.description.as_deref(), Some("Agenda:\nflows"));
    }

    #[test]
    fn expands_repeats_with_zones_exdates_and_moves() {
        let events = sample();
        // Tuesday 2026-10-27. Standup daily on weekdays at 09:30 New York;
        // the London sync at 15:00 London is 11:00 in New York; the Outlook
        // (Windows zone) 1:1 at 13:00 Pacific is 16:00 here.
        assert_eq!(
            day(&events, date(2026, 10, 27)),
            [
                "all-day Release day",
                "09:30 Standup",
                "10:00 Design review, checkout flow",
                "11:00 London sync",
                "16:00 1:1 with Sam",
            ]
        );
        // Wednesday: standup excluded by EXDATE; no review.
        assert_eq!(day(&events, date(2026, 10, 28)), ["11:00 London sync"]);
        // Thursday: standup moved to 10:15.
        assert_eq!(
            day(&events, date(2026, 10, 29)),
            ["10:15 Standup", "11:00 London sync"]
        );
        // Saturday: nothing on weekdays only.
        assert!(day(&events, date(2026, 10, 31)).is_empty());
        // After DST ends in London (Oct 25) but before New York (Nov 1),
        // 15:00 London is 11:00 here; after both, it is 10:00.
        assert_eq!(day(&events, date(2026, 11, 3))[1], "10:00 London sync");
        // Monthly on the last Friday: Oct 30, until a COUNT of 3 runs out.
        assert_eq!(day(&events, date(2026, 10, 30))[0], "09:30 Standup");
        assert!(day(&events, date(2026, 10, 30)).contains(&"14:00 Retro".to_string()));
        assert!(!day(&events, date(2027, 1, 29)).contains(&"14:00 Retro".to_string()));
        // The last weekday of the month (BYSETPOS=-1): Friday Oct 30 (the
        // 31st is a Saturday) and Monday Nov 30, not every weekday.
        let close = "16:00 Month close".to_string();
        assert!(day(&events, date(2026, 10, 30)).contains(&close));
        assert!(!day(&events, date(2026, 10, 29)).contains(&close));
        assert!(day(&events, date(2026, 11, 30)).contains(&close));
        assert!(!day(&events, date(2026, 11, 27)).contains(&close));
        // The cancelled meeting never shows.
        assert!(
            !events
                .iter()
                .filter(|e| e.cancelled)
                .any(|e| day(&events, e.start.date()).contains(&e.summary))
        );
    }

    #[test]
    fn rules_and_durations() {
        assert_eq!(duration("PT1H30M"), Some(5400));
        assert_eq!(duration("P1D"), Some(86_400));
        assert_eq!(duration("P2W"), Some(14 * 86_400));
        let r = rrule("FREQ=MONTHLY;BYDAY=-1FR;COUNT=3", &TimeZone::UTC).unwrap();
        assert_eq!(r.by_day, [(-1, Weekday::Friday)]);
        assert_eq!(r.count, Some(3));
        assert!(rrule("FREQ=HOURLY", &TimeZone::UTC).is_none());
        // Parts it cannot expand: the event shows once instead.
        assert!(rrule("FREQ=YEARLY;BYWEEKNO=20;BYDAY=MO", &TimeZone::UTC).is_none());
        assert!(rrule("FREQ=YEARLY;BYDAY=20MO", &TimeZone::UTC).is_none());
        // Friday the 13th: BYDAY and BYMONTHDAY together.
        let r = rrule("FREQ=MONTHLY;BYDAY=FR;BYMONTHDAY=13", &TimeZone::UTC).unwrap();
        let first = date(2026, 1, 1);
        let fridays: Vec<Date> = (0..12).flat_map(|n| period_dates(first, &r, n)).collect();
        assert_eq!(
            fridays,
            [date(2026, 2, 13), date(2026, 3, 13), date(2026, 11, 13)]
        );
        assert_eq!(
            zone("Pacific Standard Time"),
            TimeZone::get("America/Los_Angeles").ok()
        );
        assert_eq!(
            zone("/mozilla.org/20050126_1/Europe/London"),
            TimeZone::get("Europe/London").ok()
        );
    }

    /// Starts of a one-line event and rule in a New York window.
    fn starts(dtstart: &str, rule: &str, from: Date, to: Date) -> Vec<String> {
        let text = format!(
            "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:x\nSUMMARY:x\n\
             DTSTART;TZID=America/New_York:{dtstart}\nRRULE:{rule}\nEND:VEVENT\nEND:VCALENDAR\n"
        );
        let tz = ny();
        let events = parse(&text, &tz).unwrap();
        let at = |d: Date| d.to_zoned(tz.clone()).unwrap().timestamp();
        occurrences(&events, at(from), at(to), &tz)
            .into_iter()
            .map(|o| {
                o.start
                    .to_zoned(tz.clone())
                    .strftime("%F %H:%M")
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn until_dates_and_old_starts() {
        // A date-only UNTIL keeps that day's occurrence.
        assert_eq!(
            starts(
                "20261026T093000",
                "FREQ=DAILY;UNTIL=20261028",
                date(2026, 10, 26),
                date(2026, 11, 1)
            ),
            ["2026-10-26 09:30", "2026-10-27 09:30", "2026-10-28 09:30"]
        );
        // A daily event from decades ago still shows today.
        assert_eq!(
            starts(
                "19900101T080000",
                "FREQ=DAILY;INTERVAL=3",
                date(2026, 10, 27),
                date(2026, 10, 30)
            ),
            ["2026-10-28 08:00"]
        );
        // A multi-day all-day repeat that began before the window overlaps it.
        let text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:y\nSUMMARY:Trip\n\
                    DTSTART;VALUE=DATE:20200101\nDTEND;VALUE=DATE:20200111\n\
                    RRULE:FREQ=MONTHLY\nEND:VEVENT\nEND:VCALENDAR\n";
        assert_eq!(
            day(&parse(text, &ny()).unwrap(), date(2026, 10, 5)),
            ["all-day Trip"]
        );
    }

    #[test]
    fn reads_a_file_with_a_byte_order_mark() {
        assert!(parse("\u{feff}BEGIN:VCALENDAR\nEND:VCALENDAR\n", &TimeZone::UTC).is_ok());
    }

    #[test]
    fn rejects_other_files() {
        assert!(parse("<html>", &TimeZone::UTC).is_err());
    }
}
