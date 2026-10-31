//! What a command prints: `--json` documents on stdout, and the error line
//! and warnings on stderr.
//!
//! - A `--json` document is one object that starts with `schemaVersion` and
//!   `kind`, never a bare array, so it can gain a field without breaking a
//!   caller.
//! - An error is one line, `<tool>: <message> <hint>`, with terminal controls
//!   removed. Under `--json` an error document goes to stdout as well.
//! - A warning is `<tool>: warning: <message>`.
//!
//! Writers are passed in, so tests can read what was printed.

use std::error::Error;
use std::io::{self, Write};

use serde::Serialize;

use crate::failure::failure_in;
use crate::safe::safe;
use crate::{Failure, exit};

/// The version of every document this crate writes. It rises when a
/// document changes incompatibly; adding a field does not raise it.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize)]
struct Document<'a, T> {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    kind: &'a str,
    #[serde(flatten)]
    body: &'a T,
}

/// `body` (a struct or map) as a document of this `kind`, pretty-printed.
pub fn document<T: Serialize>(kind: &str, body: &T) -> serde_json::Result<String> {
    serde_json::to_string_pretty(&Document {
        schema_version: SCHEMA_VERSION,
        kind,
        body,
    })
}

/// Write `body` as a document of this `kind`, with a newline.
pub fn write_json<T: Serialize>(out: &mut dyn Write, kind: &str, body: &T) -> io::Result<()> {
    let text = document(kind, body).map_err(io::Error::other)?;
    writeln!(out, "{text}")
}

/// The error document: `{ schemaVersion, kind: "error", code, message, exit, hint? }`.
#[derive(Debug, Serialize, PartialEq)]
pub struct ErrorDocument {
    /// `usage` for exit 2, a failure kind for 22-25, else `error` or a name the
    /// tool chose.
    pub code: String,
    pub message: String,
    pub exit: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// Prints errors and warnings for one tool.
#[derive(Debug, Clone, Copy)]
pub struct Reporter {
    /// The tool's name, the prefix of every line.
    pub name: &'static str,
    /// `--json` was given: errors also go to stdout as an error document.
    pub json: bool,
}

impl Reporter {
    /// `<name>: <message> <hint>` and a newline, made safe.
    pub fn error_line(&self, message: &str, hint: Option<&str>) -> String {
        let text = match hint {
            Some(hint) => format!("{}: {message} {hint}\n", self.name),
            None => format!("{}: {message}\n", self.name),
        };
        safe(&text)
    }

    /// `<name>: warning: <message>` and a newline, made safe.
    pub fn warning_line(&self, message: &str) -> String {
        safe(&format!("{}: warning: {message}\n", self.name))
    }

    pub fn warn(&self, message: &str) {
        eprint!("{}", self.warning_line(message));
    }

    /// Report a failed run and return its exit code: the error line on stderr,
    /// and under `--json` the error document on stdout. The message is the
    /// error and its causes joined with `: `, as `anyhow` shows `{:#}`.
    pub fn fail(
        &self,
        err: &(dyn Error + 'static),
        stdout: &mut dyn Write,
        stderr: &mut dyn Write,
    ) -> u8 {
        let failure = failure_in(Some(err));
        let mut message = err.to_string();
        let mut source = err.source();
        while let Some(e) = source {
            message.push_str(": ");
            message.push_str(&e.to_string());
            source = e.source();
        }
        let doc = ErrorDocument {
            code: failure.map_or_else(|| "error".into(), |f| f.kind.clone()),
            message,
            exit: failure.map_or(exit::ERROR, Failure::exit_code),
            hint: failure.and_then(|f| f.hint.clone()),
        };
        if failure.is_some_and(|f| f.documented) {
            let _ = stderr.write_all(
                self.error_line(&doc.message, doc.hint.as_deref())
                    .as_bytes(),
            );
            return doc.exit;
        }
        self.report(&doc, stdout, stderr)
    }

    /// Print `doc` as the error line (and document under `--json`); return its exit code.
    pub fn report(
        &self,
        doc: &ErrorDocument,
        stdout: &mut dyn Write,
        stderr: &mut dyn Write,
    ) -> u8 {
        if self.json {
            let _ = write_json(stdout, "error", doc);
        }
        let _ = stderr.write_all(
            self.error_line(&doc.message, doc.hint.as_deref())
                .as_bytes(),
        );
        doc.exit
    }

    /// Handle a clap parse error: help and `--version` print as clap prints
    /// them and exit 0; anything else is a usage error (exit 2) shown as the
    /// error line, then clap's usage line and where the help is.
    pub fn clap_error(
        &self,
        err: &clap::Error,
        stdout: &mut dyn Write,
        stderr: &mut dyn Write,
    ) -> u8 {
        use clap::error::ErrorKind;
        if matches!(
            err.kind(),
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
        ) {
            let _ = err.print();
            return exit::OK;
        }
        let rendered = err.render().to_string();
        let rendered = rendered.strip_prefix("error: ").unwrap_or(&rendered);
        let (first, rest) = rendered.split_once('\n').unwrap_or((rendered, ""));
        let doc = ErrorDocument {
            code: "usage".into(),
            message: first.trim().to_string(),
            exit: exit::USAGE,
            hint: Some(format!("See \"{} --help\".", self.name)),
        };
        if self.json {
            let _ = write_json(stdout, "error", &doc);
        }
        let _ = stderr.write_all(self.error_line(&doc.message, None).as_bytes());
        let rest = rest.trim();
        if !rest.is_empty() {
            let _ = writeln!(stderr, "\n{}", safe(rest));
        }
        doc.exit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: Reporter = Reporter {
        name: "tooly",
        json: true,
    };

    #[test]
    fn documents_start_with_version_and_kind() {
        #[derive(Serialize)]
        struct Body {
            running: bool,
        }
        let text = document("status", &Body { running: false }).unwrap();
        assert_eq!(
            text,
            "{\n  \"schemaVersion\": 1,\n  \"kind\": \"status\",\n  \"running\": false\n}"
        );
    }

    #[test]
    fn failures_print_the_line_and_the_error_document() {
        let err = Failure::not_installed("gh is not installed.\x1b[2J").hint("Install it.");
        let (mut out, mut errs) = (vec![], vec![]);
        assert_eq!(R.fail(&err, &mut out, &mut errs), exit::MISSING_TOOL);
        assert_eq!(
            String::from_utf8(errs).unwrap(),
            "tooly: gh is not installed. Install it.\n"
        );
        let doc: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(doc["kind"], "error");
        assert_eq!(doc["code"], "not-installed");
        assert_eq!(doc["exit"], 22);
        assert_eq!(doc["hint"], "Install it.");

        let plain = std::io::Error::other("boom");
        let (mut out, mut errs) = (vec![], vec![]);
        assert_eq!(R.fail(&plain, &mut out, &mut errs), exit::ERROR);
        let doc: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(doc["code"], "error");
        assert!(doc.get("hint").is_none());

        // The command printed its own document: only the line.
        let err = Failure::new("problems", "2 problem(s)").after_document();
        let (mut out, mut errs) = (vec![], vec![]);
        assert_eq!(R.fail(&err, &mut out, &mut errs), exit::ERROR);
        assert!(out.is_empty());
        assert_eq!(String::from_utf8(errs).unwrap(), "tooly: 2 problem(s)\n");
    }

    #[test]
    fn clap_errors_are_usage_errors() {
        use clap::Parser;
        #[derive(Parser, Debug)]
        #[command(name = "tooly")]
        struct Cli {
            #[arg(long)]
            port: u16,
        }
        let err = Cli::try_parse_from(["tooly", "--port", "x"]).unwrap_err();
        let (mut out, mut errs) = (vec![], vec![]);
        assert_eq!(R.clap_error(&err, &mut out, &mut errs), exit::USAGE);
        let errs = String::from_utf8(errs).unwrap();
        assert!(errs.starts_with("tooly: invalid value 'x'"), "{errs}");
        assert!(errs.contains("--help"), "{errs}");
        let doc: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(doc["code"], "usage");
        assert_eq!(doc["exit"], 2);
    }
}
