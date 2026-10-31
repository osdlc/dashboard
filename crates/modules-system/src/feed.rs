//! What the `http` and `file` modules share: turning what they read into a
//! view. The data can be a view or a widget in the contract's JSON (see
//! `docs/contract.md`), values picked out of any JSON, or plain text.

use anyhow::{Context, Result, bail};
use dashboard_contract::{Status, View, Widget};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

/// Larger bodies and files are refused.
pub const MAX_BYTES: u64 = 1024 * 1024;

/// How to read the data.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    /// A view or a widget when the data is one, else indented JSON, else text.
    #[default]
    Auto,
    /// A view (`{status, summary, root}`) or one widget (`{type, ...}`).
    View,
    /// Any JSON, shown indented.
    Json,
    /// Shown as it is.
    Text,
}

/// One value picked out of JSON, shown as a stat.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub label: String,
    /// JSON Pointer to the value, e.g. `/queue/length` or `/items/0/name`.
    /// A list shows its length.
    pub pointer: String,
    /// Shown after the value, e.g. "ms".
    #[serde(default)]
    pub unit: Option<String>,
}

/// How a module shows what it read.
pub struct Shape<'a> {
    pub format: Format,
    pub fields: &'a [Field],
    pub markdown: bool,
    pub lines: usize,
}

/// Checks the settings both modules share, at init.
pub fn check(format: Format, fields: &[Field], lines: usize) -> Result<()> {
    if lines == 0 {
        bail!("`lines` must be at least 1");
    }
    if !fields.is_empty() && !matches!(format, Format::Auto | Format::Json) {
        bail!("`fields` reads JSON; leave out `format` or set it to json");
    }
    for f in fields {
        if !f.pointer.is_empty() && !f.pointer.starts_with('/') {
            bail!(
                "field {:?}: pointer {:?} must start with `/`, e.g. /data/count",
                f.label,
                f.pointer
            );
        }
    }
    Ok(())
}

/// The tile for `body`. `name` is the source's short name, used in summaries.
pub fn view(body: &str, name: &str, shape: &Shape) -> Result<View> {
    let body = body.strip_prefix('\u{feff}').unwrap_or(body);
    if !shape.fields.is_empty() {
        let json: Value = serde_json::from_str(body).context("`fields` needs JSON")?;
        return Ok(fields_view(&json, shape.fields));
    }
    match shape.format {
        Format::View => {
            let json: Value = serde_json::from_str(body).context("not JSON")?;
            contract_view(json, name)
        }
        Format::Json => {
            let json: Value = serde_json::from_str(body).context("not JSON")?;
            Ok(text_view(&pretty(&json), name, false, shape.lines))
        }
        Format::Text => Ok(text_view(body, name, shape.markdown, shape.lines)),
        Format::Auto => match serde_json::from_str::<Value>(body) {
            Ok(json) if looks_like_view(&json) => {
                // Other JSON can have a `type` key too: show it as JSON
                // rather than fail.
                Ok(contract_view(json.clone(), name)
                    .unwrap_or_else(|_| text_view(&pretty(&json), name, false, shape.lines)))
            }
            Ok(json) => Ok(text_view(&pretty(&json), name, false, shape.lines)),
            Err(_) => Ok(text_view(body, name, shape.markdown, shape.lines)),
        },
    }
}

fn looks_like_view(json: &Value) -> bool {
    json.get("root").is_some() || json.get("type").is_some_and(Value::is_string)
}

/// A view as it is, or a widget in an ok view. Problems the checker finds
/// (an empty summary, a percent over 100) are errors.
fn contract_view(json: Value, name: &str) -> Result<View> {
    let view = if json.get("root").is_some() {
        serde_json::from_value::<View>(json).context("not a view")?
    } else {
        View {
            status: Status::Ok,
            summary: name.to_string(),
            root: serde_json::from_value::<Widget>(json).context("not a widget")?,
        }
    };
    let problems: Vec<String> = dashboard_contract::check::check_view(&view)
        .into_iter()
        .filter(|f| f.problem)
        .map(|f| f.text)
        .collect();
    if !problems.is_empty() {
        bail!("not a valid view: {}", problems.join("; "));
    }
    Ok(view)
}

fn pretty(json: &Value) -> String {
    serde_json::to_string_pretty(json).unwrap_or_default()
}

/// Text as the textfile module shows it: CRLF as LF, trailing blanks
/// dropped, the first `max_lines` lines.
fn text_view(text: &str, name: &str, markdown: bool, max_lines: usize) -> View {
    let text = text.replace("\r\n", "\n");
    let text = text.trim_end_matches([' ', '\t', '\r', '\n']);
    if text.is_empty() {
        return View {
            status: Status::Ok,
            summary: format!("{name} · empty"),
            root: Widget::Text {
                text: "(empty)".into(),
                markdown: false,
            },
        };
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let (shown, summary) = if lines.len() > max_lines {
        (
            format!("{}\n…", lines[..max_lines].join("\n")),
            format!("{name} · first {max_lines} of {} lines", lines.len()),
        )
    } else {
        let count = match lines.len() {
            1 => "1 line".to_string(),
            n => format!("{n} lines"),
        };
        (text.to_string(), format!("{name} · {count}"))
    };
    View {
        status: Status::Ok,
        summary,
        root: Widget::Text {
            text: shown,
            markdown,
        },
    }
}

/// One stat per field, side by side. A missing value shows "—" and makes
/// the tile warn.
fn fields_view(json: &Value, fields: &[Field]) -> View {
    let mut status = Status::Ok;
    let mut summary = vec![];
    let stats: Vec<Widget> = fields
        .iter()
        .map(|f| {
            let value = json.pointer(&f.pointer).and_then(show);
            if value.is_none() {
                status = Status::Warn;
            }
            let value = value.unwrap_or_else(|| "—".into());
            summary.push(match &f.unit {
                Some(unit) => format!("{} {value} {unit}", f.label),
                None => format!("{} {value}", f.label),
            });
            Widget::Stat {
                label: f.label.clone(),
                value,
                unit: f.unit.clone(),
                delta: None,
                status: None,
            }
        })
        .collect();
    let root = match <[Widget; 1]>::try_from(stats) {
        Ok([one]) => one,
        Err(stats) => Widget::Row { children: stats },
    };
    View {
        status,
        summary: summary.join(" · "),
        root,
    }
}

/// A JSON value as a stat's text. None for null.
fn show(v: &Value) -> Option<String> {
    Some(match v {
        Value::Null => return None,
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => match (n.as_i64(), n.as_u64(), n.as_f64()) {
            (Some(i), _, _) => i.to_string(),
            (_, Some(u), _) => u.to_string(),
            (_, _, Some(f)) => {
                let s = format!("{f:.2}");
                s.trim_end_matches('0').trim_end_matches('.').to_string()
            }
            _ => n.to_string(),
        },
        Value::Array(items) => items.len().to_string(),
        Value::Object(_) => "{…}".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(format: Format) -> Shape<'static> {
        Shape {
            format,
            fields: &[],
            markdown: false,
            lines: 50,
        }
    }

    fn text(v: &View) -> &str {
        match &v.root {
            Widget::Text { text, .. } => text,
            other => panic!("text: {other:?}"),
        }
    }

    #[test]
    fn auto_reads_views_widgets_json_and_text() {
        let v = view(
            r#"{"status":"warn","summary":"2 late","root":{"type":"text","text":"hi"}}"#,
            "x",
            &shape(Format::Auto),
        )
        .unwrap();
        assert_eq!(v.status, Status::Warn);
        assert_eq!(v.summary, "2 late");

        let v = view(
            r#"{"type":"gauge","label":"Disk","percent":40}"#,
            "disk.json",
            &shape(Format::Auto),
        )
        .unwrap();
        assert_eq!(v.summary, "disk.json");
        assert!(matches!(v.root, Widget::Gauge { .. }));

        // JSON with a `type` that is not a widget is shown as JSON.
        let v = view(r#"{"type":"PushEvent","id":1}"#, "x", &shape(Format::Auto)).unwrap();
        assert!(text(&v).contains("\"PushEvent\""));

        let v = view("\u{feff}a\r\nb\n\n", "x", &shape(Format::Auto)).unwrap();
        assert_eq!(text(&v), "a\nb");
        assert_eq!(v.summary, "x · 2 lines");
    }

    #[test]
    fn view_format_is_strict() {
        assert!(view("hello", "x", &shape(Format::View)).is_err());
        assert!(view(r#"{"type":"nope"}"#, "x", &shape(Format::View)).is_err());
        let err = view(
            r#"{"status":"ok","summary":"","root":{"type":"text","text":"hi"}}"#,
            "x",
            &shape(Format::View),
        )
        .unwrap_err();
        assert!(err.to_string().contains("not a valid view"), "{err}");
    }

    #[test]
    fn text_and_json_formats() {
        let v = view(r#"{"a":1}"#, "x", &shape(Format::Text)).unwrap();
        assert_eq!(text(&v), r#"{"a":1}"#);
        let v = view(r#"{"a":1}"#, "x", &shape(Format::Json)).unwrap();
        assert_eq!(text(&v), "{\n  \"a\": 1\n}");
        assert!(view("nope", "x", &shape(Format::Json)).is_err());
        let v = view(
            "1\n2\n3",
            "x",
            &Shape {
                lines: 2,
                ..shape(Format::Text)
            },
        )
        .unwrap();
        assert_eq!(text(&v), "1\n2\n…");
        assert_eq!(v.summary, "x · first 2 of 3 lines");
    }

    #[test]
    fn fields_pick_values() {
        let fields = vec![
            Field {
                label: "Queue".into(),
                pointer: "/queue".into(),
                unit: None,
            },
            Field {
                label: "p99".into(),
                pointer: "/latency/p99".into(),
                unit: Some("ms".into()),
            },
            Field {
                label: "Jobs".into(),
                pointer: "/jobs".into(),
                unit: None,
            },
            Field {
                label: "Gone".into(),
                pointer: "/missing".into(),
                unit: None,
            },
        ];
        let body = r#"{"queue":12,"latency":{"p99":107.456},"jobs":["a","b"]}"#;
        let v = view(
            body,
            "x",
            &Shape {
                fields: &fields,
                ..shape(Format::Auto)
            },
        )
        .unwrap();
        assert_eq!(v.status, Status::Warn);
        assert_eq!(v.summary, "Queue 12 · p99 107.46 ms · Jobs 2 · Gone —");
        let Widget::Row { children } = &v.root else {
            panic!("{:?}", v.root)
        };
        assert_eq!(children.len(), 4);

        let one = &fields[..1];
        let v = view(
            body,
            "x",
            &Shape {
                fields: one,
                ..shape(Format::Auto)
            },
        )
        .unwrap();
        assert_eq!(v.status, Status::Ok);
        assert!(matches!(v.root, Widget::Stat { .. }));
        assert!(
            view(
                "text",
                "x",
                &Shape {
                    fields: one,
                    ..shape(Format::Auto)
                }
            )
            .is_err()
        );
    }

    #[test]
    fn checks_settings() {
        let bad = [Field {
            label: "a".into(),
            pointer: "a".into(),
            unit: None,
        }];
        assert!(check(Format::Auto, &bad, 10).is_err());
        assert!(check(Format::Auto, &[], 0).is_err());
        let ok = [Field {
            label: "a".into(),
            pointer: "/a".into(),
            unit: None,
        }];
        assert!(check(Format::Text, &ok, 10).is_err());
        assert!(check(Format::Json, &ok, 10).is_ok());
    }
}
