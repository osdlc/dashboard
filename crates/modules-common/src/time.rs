//! Times as the tracker APIs send them, and ages like "5m ago".

/// "5m ago" style age from an RFC 3339 timestamp.
pub fn ago(ts: &str) -> String {
    let Some(then) = parse_time(ts) else {
        return ts.to_string();
    };
    let secs = (dashboard_host::now_ms() / 1000 - then).max(0);
    match secs {
        s if s < 60 => "just now".into(),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86_400),
    }
}

/// Seconds since the epoch from `YYYY-MM-DDTHH:MM:SS`, with optional
/// fractional seconds and a `Z`, `+hh:mm` or `+hhmm` offset (no offset = UTC).
pub fn parse_time(ts: &str) -> Option<i64> {
    let n = |r: std::ops::Range<usize>| ts.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (h, mi, s) = (n(11..13)?, n(14..16)?, n(17..19)?);
    let rest = ts.get(19..)?;
    let rest = rest.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    let offset = match rest.as_bytes().first() {
        None | Some(b'Z') => 0,
        Some(&sign @ (b'+' | b'-')) => {
            let digits: String = rest[1..].chars().filter(char::is_ascii_digit).collect();
            let oh: i64 = digits.get(0..2)?.parse().ok()?;
            let om: i64 = digits.get(2..4).and_then(|m| m.parse().ok()).unwrap_or(0);
            let secs = oh * 3600 + om * 60;
            if sign == b'+' { secs } else { -secs }
        }
        _ => return None,
    };
    // Days from civil date (Howard Hinnant's algorithm).
    let y = if mo <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (mo + if mo > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3600 + mi * 60 + s - offset)
}

/// RFC 3339 time `secs_ago` seconds before now, for demo data whose ages
/// should read the same whenever it is shown.
pub fn demo_time(secs_ago: i64) -> String {
    rfc3339(dashboard_host::now_ms() / 1000 - secs_ago)
}

/// RFC 3339 time (UTC) from seconds since the epoch.
pub fn rfc3339(t: i64) -> String {
    let (days, secs) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    // Civil date from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tracker_timestamps() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_time("2026-10-27T02:09:57Z"), Some(1_793_066_997));
        // Linear: milliseconds. Jira: milliseconds and +hhmm. Forgejo: +hh:mm.
        assert_eq!(parse_time("2026-10-27T02:09:57.123Z"), Some(1_793_066_997));
        assert_eq!(
            parse_time("2026-10-27T04:09:57.000+0200"),
            Some(1_793_066_997)
        );
        assert_eq!(parse_time("2026-10-26T21:09:57-05:00"), Some(1_793_066_997));
        assert_eq!(parse_time("garbage"), None);
    }

    #[test]
    fn demo_times_parse_back() {
        let now = dashboard_host::now_ms() / 1000;
        for ago in [0, 59, 3_600, 86_400 * 40, 86_400 * 400] {
            // demo_time reads the clock again, so a second may have passed.
            let back = parse_time(&demo_time(ago)).unwrap();
            assert!((0..=1).contains(&(back - (now - ago))), "{ago}");
        }
    }
}
