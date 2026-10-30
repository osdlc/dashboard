//! Automatic screen rotation. Time is passed in so the logic is testable.

use std::time::{Duration, Instant};

use dashboard_contract::{RotateSettings, ScreenLayout};

pub struct Rotation {
    /// Indexes into the screen list, in rotation order.
    order: Vec<usize>,
    interval: Duration,
    resume_after: Duration,
    pub enabled: bool,
    last_input: Option<Instant>,
    last_switch: Instant,
}

#[derive(Debug, PartialEq, Eq)]
pub enum State {
    On,
    /// Waiting for the user to be idle.
    Paused,
    Off,
}

impl Rotation {
    pub fn new(settings: &RotateSettings, screens: &[ScreenLayout], now: Instant) -> Option<Self> {
        let order: Vec<usize> = settings
            .screens
            .iter()
            .filter_map(|name| screens.iter().position(|s| &s.name == name))
            .collect();
        (order.len() >= 2).then(|| Self {
            order,
            interval: Duration::from_millis(settings.interval_ms),
            resume_after: Duration::from_millis(settings.resume_after_ms),
            enabled: true,
            last_input: None,
            last_switch: now,
        })
    }

    /// Any key press or manual screen change.
    pub fn input(&mut self, now: Instant) {
        self.last_input = Some(now);
        self.last_switch = now;
    }

    pub fn toggle(&mut self, now: Instant) {
        self.enabled = !self.enabled;
        self.last_input = None;
        self.last_switch = now;
    }

    pub fn state(&self, now: Instant) -> State {
        match (self.enabled, self.last_input) {
            (false, _) => State::Off,
            (true, Some(t)) if now.duration_since(t) < self.resume_after => State::Paused,
            _ => State::On,
        }
    }

    /// The screen to show now, if it's time to move on from `current`.
    pub fn tick(&mut self, current: usize, now: Instant) -> Option<usize> {
        if self.state(now) != State::On || now.duration_since(self.last_switch) < self.interval {
            return None;
        }
        self.last_switch = now;
        let next = match self.order.iter().position(|&i| i == current) {
            Some(pos) => self.order[(pos + 1) % self.order.len()],
            None => self.order[0],
        };
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screens(names: &[&str]) -> Vec<ScreenLayout> {
        names
            .iter()
            .map(|n| ScreenLayout {
                name: n.to_string(),
                tiles: vec![],
            })
            .collect()
    }

    fn rotation(now: Instant) -> Rotation {
        let settings = RotateSettings {
            interval_ms: 10_000,
            screens: vec!["a".into(), "c".into()],
            resume_after_ms: 30_000,
        };
        Rotation::new(&settings, &screens(&["a", "b", "c"]), now).unwrap()
    }

    #[test]
    fn rotates_through_listed_screens() {
        let t0 = Instant::now();
        let s = Duration::from_secs;
        let mut r = rotation(t0);
        assert_eq!(r.tick(0, t0 + s(5)), None);
        assert_eq!(r.tick(0, t0 + s(10)), Some(2));
        assert_eq!(r.tick(2, t0 + s(20)), Some(0));
        // From a screen not in the list, go to the first listed one.
        assert_eq!(r.tick(1, t0 + s(30)), Some(0));
    }

    #[test]
    fn input_pauses_until_idle() {
        let t0 = Instant::now();
        let s = Duration::from_secs;
        let mut r = rotation(t0);
        r.input(t0 + s(1));
        assert_eq!(r.state(t0 + s(20)), State::Paused);
        assert_eq!(r.tick(0, t0 + s(20)), None);
        assert_eq!(r.state(t0 + s(31)), State::On);
        assert_eq!(r.tick(0, t0 + s(31)), Some(2));
    }

    #[test]
    fn toggle_turns_off_and_on() {
        let t0 = Instant::now();
        let s = Duration::from_secs;
        let mut r = rotation(t0);
        r.toggle(t0);
        assert_eq!(r.state(t0 + s(60)), State::Off);
        assert_eq!(r.tick(0, t0 + s(60)), None);
        r.toggle(t0 + s(60));
        assert_eq!(r.tick(0, t0 + s(70)), Some(2));
    }

    #[test]
    fn needs_two_known_screens() {
        let settings = RotateSettings {
            interval_ms: 1,
            screens: vec!["a".into(), "zzz".into()],
            resume_after_ms: 1,
        };
        assert!(Rotation::new(&settings, &screens(&["a", "b"]), Instant::now()).is_none());
    }
}
