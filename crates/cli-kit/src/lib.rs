//! Command-line conventions from the osdlc CLI standard
//! (`standards/cli-interface.md` in osdlc/osdlc), ported from
//! `@osdlc/cli-tool`:
//!
//! - [`exit`]: what an exit code means, as named constants.
//! - [`Failure`]: an error that carries its exit code and a hint.
//! - [`safe`]: text from outside made safe to print on a terminal.
//! - [`output`]: the error line, warnings, `--json` documents and clap errors.
//! - [`boot`]: the one-time boot token a local web view signs in with.
//!
//! Nothing here knows about the dashboard, so it can move into a shared
//! `cli-kit` crate once a second Rust tool needs it.

pub mod boot;
pub mod exit;
mod failure;
pub mod output;
pub mod safe;

pub use failure::{Failure, exit_code_for_kind, exit_code_of, failure_in};
