//! Signing a browser in to a local web view without putting the long-lived
//! token in a URL. Port of `wayfinder/src/serve/auth.ts`.
//!
//! Other local users can reach 127.0.0.1, and a URL ends up in browser
//! history and in the browser opener's argv. So the URL the server prints or
//! opens carries a boot token instead of the session token. The page sends it
//! once, in a JSON body, and gets the session token back. A boot token works
//! once and expires a minute after it is shown.
//!
//! The right boot token sent a second time is refused, and the caller is told
//! (a second tab, or another program that read the link), so the server can
//! warn. Sessions already signed in keep working; [`SignIn::renew`] makes a
//! new link.

use std::time::{Duration, Instant};

/// How long a boot token stays usable once it is shown.
pub const BOOT_TTL: Duration = Duration::from_secs(60);

/// A new random token: 32 lowercase hex characters (128 bits).
pub fn new_token() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("the OS random source");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Compare tokens in time that doesn't depend on where they differ.
pub fn token_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// What [`SignIn::redeem`] decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Redeem {
    /// The first use of the live boot token: hand out the session token.
    Ok,
    /// Wrong, expired or already used.
    Refused,
    /// The right boot token, but already used: refused, and worth a warning.
    Replayed,
}

/// The one-time boot token.
#[derive(Debug)]
pub struct SignIn {
    token: String,
    live: bool,
    redeemed: bool,
    expires_at: Option<Instant>,
}

impl Default for SignIn {
    fn default() -> Self {
        Self::new()
    }
}

impl SignIn {
    /// A new boot token. It doesn't expire until [`arm`](Self::arm) is called.
    pub fn new() -> Self {
        Self {
            token: new_token(),
            live: true,
            redeemed: false,
            expires_at: None,
        }
    }

    /// Put this in the URL the person opens.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Start the expiry timer. Call it when the URL is shown.
    pub fn arm(&mut self, now: Instant) {
        self.expires_at = Some(now + BOOT_TTL);
    }

    /// [`Redeem::Ok`] once, for the first unexpired `candidate` that matches.
    pub fn redeem(&mut self, candidate: &str, now: Instant) -> Redeem {
        let matches = token_eq(candidate, &self.token);
        if self.redeemed && matches {
            return Redeem::Replayed;
        }
        if !self.live {
            return Redeem::Refused;
        }
        if self.expires_at.is_some_and(|t| now > t) {
            self.live = false;
            return Redeem::Refused;
        }
        if !matches {
            return Redeem::Refused;
        }
        self.live = false;
        self.redeemed = true;
        Redeem::Ok
    }

    /// Replace the boot token with a new one, armed, and return it. The old
    /// one stops working; session tokens already handed out keep working.
    pub fn renew(&mut self, now: Instant) -> &str {
        *self = Self::new();
        self.arm(now);
        &self.token
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn works_once_then_reports_a_replay() {
        let now = Instant::now();
        let mut s = SignIn::new();
        s.arm(now);
        let token = s.token().to_string();
        assert_eq!(s.redeem("nope", now), Redeem::Refused);
        assert_eq!(s.redeem(&token, now), Redeem::Ok);
        assert_eq!(s.redeem(&token, now), Redeem::Replayed);
        assert_eq!(s.redeem("nope", now), Redeem::Refused);
    }

    #[test]
    fn expires_a_minute_after_it_is_shown_and_renews() {
        let now = Instant::now();
        let mut s = SignIn::new();
        // Not armed yet: no expiry.
        let token = s.token().to_string();
        s.arm(now);
        let late = now + BOOT_TTL + Duration::from_secs(1);
        assert_eq!(s.redeem(&token, late), Redeem::Refused);
        let fresh = s.renew(late).to_string();
        assert_ne!(fresh, token);
        assert_eq!(s.redeem(&token, late), Redeem::Refused);
        assert_eq!(s.redeem(&fresh, late), Redeem::Ok);
    }

    #[test]
    fn tokens_are_random_hex() {
        let (a, b) = (new_token(), new_token());
        assert_eq!(a.len(), 32);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
        assert!(token_eq(&a, &a.clone()) && !token_eq(&a, &b) && !token_eq(&a, "x"));
    }
}
