//! A classified failure: a `kind` that decides the exit code, a message and
//! a hint. Port of `@osdlc/cli-tool`'s `failure.ts`.
//!
//! Code returns one inside an `anyhow::Error` (or any error chain), and
//! `main` finds it with [`exit_code_of`]. An error without one exits
//! [`exit::ERROR`].

use std::fmt;

use crate::exit;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// A stable name to branch on: `usage`, a kind from [`exit_code_for_kind`]
    /// such as `not-installed`, or one the tool chose.
    pub kind: String,
    pub message: String,
    /// What to do about it, printed after the message.
    pub hint: Option<String>,
    /// The command already printed its `--json` document, which shows what
    /// went wrong, so no error document follows it.
    pub documented: bool,
}

impl Failure {
    pub fn new(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
            hint: None,
            documented: false,
        }
    }

    /// The command cannot run as typed (exit 2).
    pub fn usage(message: impl Into<String>) -> Self {
        Self::new("usage", message)
    }

    /// A program the run needs is not installed (exit 22).
    pub fn not_installed(message: impl Into<String>) -> Self {
        Self::new("not-installed", message)
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Mark that the `--json` document was printed (see [`Self::documented`]).
    pub fn after_document(mut self) -> Self {
        self.documented = true;
        self
    }

    pub fn exit_code(&self) -> u8 {
        exit_code_for_kind(&self.kind)
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Failure {}

/// The exit code for a failure `kind`. The table is `FAILURE_EXIT` in
/// `failure.ts`, so two tools report the same failure with the same code.
pub fn exit_code_for_kind(kind: &str) -> u8 {
    match kind {
        "usage" => exit::USAGE,
        "not-installed" | "not-executable" => exit::MISSING_TOOL,
        "not-authenticated" | "missing-token" | "not-found" | "tls" | "http" | "unreadable"
        | "unsupported" => exit::SERVICE,
        "not-a-repo" | "no-remote" | "bad-remote" | "bad-cwd" => exit::REPOSITORY,
        "network" | "remote-unreachable" | "timeout" | "rate-limited" => exit::UNREACHABLE,
        _ => exit::ERROR,
    }
}

/// The first [`Failure`] in an error chain, if any.
pub fn failure_in<'a>(
    mut err: Option<&'a (dyn std::error::Error + 'static)>,
) -> Option<&'a Failure> {
    while let Some(e) = err {
        if let Some(f) = e.downcast_ref::<Failure>() {
            return Some(f);
        }
        err = e.source();
    }
    None
}

/// The exit code for an error chain: its [`Failure`]'s code, else [`exit::ERROR`].
pub fn exit_code_of(err: &(dyn std::error::Error + 'static)) -> u8 {
    failure_in(Some(err)).map_or(exit::ERROR, Failure::exit_code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Wrapped(Failure);
    impl fmt::Display for Wrapped {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("while checking")
        }
    }
    impl std::error::Error for Wrapped {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.0)
        }
    }

    #[test]
    fn kinds_map_to_the_shared_codes() {
        assert_eq!(exit_code_for_kind("usage"), 2);
        assert_eq!(exit_code_for_kind("not-installed"), 22);
        assert_eq!(exit_code_for_kind("missing-token"), 23);
        assert_eq!(exit_code_for_kind("no-remote"), 24);
        assert_eq!(exit_code_for_kind("rate-limited"), 25);
        assert_eq!(exit_code_for_kind("spawn"), 1);
    }

    #[test]
    fn finds_a_failure_under_context() {
        let err = Wrapped(Failure::not_installed("gh is not installed"));
        assert_eq!(exit_code_of(&err), exit::MISSING_TOOL);
        let plain = std::io::Error::other("boom");
        assert_eq!(exit_code_of(&plain), exit::ERROR);
    }
}
