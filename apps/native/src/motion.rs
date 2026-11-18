//! Animation: values that move to their new targets over a moment, as the web
//! view's CSS transitions and `AnimatedNumber.vue` do. Drawing asks for a
//! value by key; while any value is moving, the view asks for another frame.
//! With animations off, every value is at its target at once.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::{Duration, Instant};

const NUMBER: Duration = Duration::from_millis(500);
const FADE: Duration = Duration::from_millis(250);

struct Number {
    from: f64,
    to: f64,
    start: Instant,
}

struct Slide {
    t0: i64,
    t1: i64,
    /// When the data last moved.
    at: Instant,
    /// How far to shift, as a share of the chart's width, at `start`.
    shift: f64,
    start: Instant,
    duration: Duration,
}

#[derive(Default)]
pub struct Motion {
    numbers: RefCell<HashMap<String, Number>>,
    slides: RefCell<HashMap<String, Slide>>,
    fades: RefCell<HashMap<String, Instant>>,
    moving: Cell<bool>,
    still: Cell<bool>,
}

impl Motion {
    /// Call before drawing a frame. `animate` false: nothing moves this frame.
    pub fn begin(&self, animate: bool) {
        self.moving.set(false);
        self.still.set(!animate);
    }

    /// True when something drawn this frame is still moving.
    pub fn moving(&self) -> bool {
        self.moving.get()
    }

    /// Forget everything, e.g. on another screen: nothing animates from there.
    pub fn clear(&self) {
        self.numbers.borrow_mut().clear();
        self.slides.borrow_mut().clear();
    }

    /// `target`, reached over half a second from the value shown before it.
    pub fn number(&self, key: &str, target: f64) -> f64 {
        let now = Instant::now();
        let mut numbers = self.numbers.borrow_mut();
        let n = numbers.entry(key.to_string()).or_insert(Number {
            from: target,
            to: target,
            // Already there: a number just shown isn't moving.
            start: now.checked_sub(NUMBER).unwrap_or(now),
        });
        if n.to != target {
            n.from = eased(n, now);
            n.to = target;
            n.start = now;
        }
        if self.still.get() {
            n.from = target;
            return target;
        }
        let value = eased(n, now);
        if now.duration_since(n.start) < NUMBER {
            self.moving.set(true);
        }
        value
    }

    /// How far to shift a chart right, as a share of its width, when its time
    /// range moved: it scrolls instead of jumping (`SeriesChart.vue`).
    pub fn slide(&self, key: &str, t0: i64, t1: i64, moved: impl FnOnce(i64) -> f64) -> f64 {
        let now = Instant::now();
        let mut slides = self.slides.borrow_mut();
        let s = slides.entry(key.to_string()).or_insert(Slide {
            t0,
            t1,
            at: now,
            shift: 0.0,
            start: now,
            duration: Duration::ZERO,
        });
        if s.t0 != t0 || s.t1 != t1 {
            if t0 > s.t0 && t1 > s.t1 {
                s.shift = moved(s.t1);
                s.duration = now.duration_since(s.at).min(Duration::from_secs(2));
                s.start = now;
            }
            s.t0 = t0;
            s.t1 = t1;
            s.at = now;
        }
        let spent = now.duration_since(s.start);
        if self.still.get() || spent >= s.duration {
            return 0.0;
        }
        self.moving.set(true);
        s.shift * (1.0 - spent.as_secs_f64() / s.duration.as_secs_f64())
    }

    /// 0 to 1 over a moment, from the first time `key` is asked for.
    pub fn fade_in(&self, key: &str) -> f32 {
        let now = Instant::now();
        let start = *self
            .fades
            .borrow_mut()
            .entry(key.to_string())
            .or_insert(now);
        if self.still.get() {
            return 1.0;
        }
        let t = now.duration_since(start).as_secs_f32() / FADE.as_secs_f32();
        if t < 1.0 {
            self.moving.set(true);
        }
        t.min(1.0)
    }

    pub fn forget_fade(&self, key: &str) {
        self.fades.borrow_mut().remove(key);
    }
}

fn eased(n: &Number, now: Instant) -> f64 {
    let t = (now.duration_since(n.start).as_secs_f64() / NUMBER.as_secs_f64()).min(1.0);
    n.from + (n.to - n.from) * (1.0 - (1.0 - t).powi(3))
}

/// The number in a string like "1,204.5", and how many decimals it shows.
pub fn parse_number(s: &str) -> Option<(f64, usize)> {
    let s = s.trim();
    let digits = s.strip_prefix('-').unwrap_or(s);
    if digits.is_empty()
        || !digits
            .chars()
            .all(|c| c.is_ascii_digit() || c == ',' || c == '.')
        || digits.matches('.').count() > 1
        || digits.ends_with('.')
    {
        return None;
    }
    let plain = s.replace(',', "");
    let decimals = plain.split_once('.').map_or(0, |(_, d)| d.len());
    Some((plain.parse().ok()?, decimals))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_animations_off_values_are_at_their_targets() {
        let motion = Motion::default();
        motion.begin(false);
        assert_eq!(motion.number("a", 10.0), 10.0);
        assert_eq!(motion.number("a", 20.0), 20.0);
        assert_eq!(motion.fade_in("screen"), 1.0);
        assert!(!motion.moving());
    }

    #[test]
    fn reads_plain_and_formatted_numbers_only() {
        assert_eq!(parse_number("1,204.5"), Some((1204.5, 1)));
        assert_eq!(parse_number("-3"), Some((-3.0, 0)));
        assert_eq!(parse_number("1.2k"), None);
        assert_eq!(parse_number("14:05"), None);
        assert_eq!(parse_number(""), None);
    }

    #[test]
    fn a_new_number_starts_where_it_is() {
        let motion = Motion::default();
        assert_eq!(motion.number("a", 10.0), 10.0);
        assert!(!motion.moving());
        let v = motion.number("a", 20.0);
        assert!((10.0..20.0).contains(&v));
        assert!(motion.moving());
    }
}
