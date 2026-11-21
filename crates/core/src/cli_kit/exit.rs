//! What a CLI's exit code means. The same in every osdlc tool, so a script
//! never has to learn one tool's number for "you typed it wrong".
//!
//! Port of `@osdlc/cli-tool`'s `exit.ts` and the codes in `failure.ts`.

/// The command did what it was asked.
pub const OK: u8 = 0;
/// A well-formed command failed against the state it found. Look at the state.
pub const ERROR: u8 = 1;
/// The command cannot run as typed: an unknown flag or command, a missing or
/// extra argument, a bad value, two flags that conflict. Fix the command line.
pub const USAGE: u8 = 2;

/// A program the run needs is not installed or cannot be run. Install it.
pub const MISSING_TOOL: u8 = 22;
/// A service refused the caller or gave an unusable answer (not signed in,
/// no token, not found, an HTTP error). Fix the credentials or the name.
pub const SERVICE: u8 = 23;
/// There is no repository or remote to work on. Run it somewhere else.
pub const REPOSITORY: u8 = 24;
/// A service could not be reached in time, or limited the caller. Try again later.
pub const UNREACHABLE: u8 = 25;

/// First code a tool may use for an outcome of its own. A tool's own codes
/// avoid 22-25.
pub const DOMAIN_MIN: u8 = 10;
/// Last such code. Codes above are taken by `sysexits.h` (64-78) and the
/// shell (126, 127, 128+N for a signal).
pub const DOMAIN_MAX: u8 = 63;

/// Whether `code` is in the range a tool may define meanings in.
pub fn is_domain(code: u8) -> bool {
    (DOMAIN_MIN..=DOMAIN_MAX).contains(&code)
}
