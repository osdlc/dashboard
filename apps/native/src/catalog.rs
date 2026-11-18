//! Settings forms from a module's JSON Schema, as in `web/src/catalog.ts`.

use std::collections::{HashMap, HashSet};

use dashboard_contract::CatalogEntry;
use serde_json::{Map, Value, json};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FieldKind {
    Text,
    Number,
    Integer,
    Boolean,
    Choice,
    Lines,
    Json,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub key: String,
    pub kind: FieldKind,
    pub required: bool,
    pub description: String,
    /// The default, as shown in an empty input.
    pub placeholder: String,
    /// For `Choice`: the allowed values and their labels.
    pub choices: Vec<(String, String)>,
}

/// False for a marketplace entry that isn't installed yet.
pub fn installed(entry: &CatalogEntry) -> bool {
    entry.source != "marketplace"
}

/// The form for a module's `config:` block.
pub fn fields(schema: Option<&Value>) -> Vec<Field> {
    let Some(schema) = schema else {
        return vec![];
    };
    let Some(props) = schema.get("properties").and_then(Value::as_object) else {
        return vec![];
    };
    let required: HashSet<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    props
        .iter()
        .map(|(key, raw)| {
            let prop = resolve(schema, raw);
            let choices = choices_of(schema, &prop);
            let kind = if !choices.is_empty() {
                FieldKind::Choice
            } else {
                match main_type(&prop) {
                    Some("string") => FieldKind::Text,
                    Some("integer") => FieldKind::Integer,
                    Some("number") => FieldKind::Number,
                    Some("boolean") => FieldKind::Boolean,
                    Some("array") => {
                        let items = prop.get("items").cloned().unwrap_or(json!({}));
                        if main_type(&resolve(schema, &items)) == Some("string") {
                            FieldKind::Lines
                        } else {
                            FieldKind::Json
                        }
                    }
                    _ => FieldKind::Json,
                }
            };
            let text =
                |s: &Value, name: &str| s.get(name).and_then(Value::as_str).map(str::to_string);
            let description = text(raw, "description")
                .or_else(|| text(&prop, "description"))
                .unwrap_or_default();
            let default = raw.get("default").or_else(|| prop.get("default"));
            Field {
                key: key.clone(),
                kind,
                required: required.contains(key.as_str()),
                description,
                placeholder: placeholder(default, kind),
                choices,
            }
        })
        .collect()
}

/// Follow a local `$ref`, and unwrap `anyOf: [X, null]` (an optional setting).
fn resolve(root: &Value, schema: &Value) -> Value {
    if let Some(name) = schema
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|r| r.strip_prefix("#/$defs/"))
    {
        let def = root.get("$defs").and_then(|d| d.get(name)).cloned();
        return resolve(root, &def.unwrap_or(json!({})));
    }
    if let Some(any) = schema.get("anyOf").and_then(Value::as_array) {
        let rest: Vec<&Value> = any
            .iter()
            .filter(|x| x.get("type").and_then(Value::as_str) != Some("null"))
            .collect();
        if let [only] = rest.as_slice() {
            let mut out = resolve(root, only);
            if let (Some(obj), Some(desc)) = (out.as_object_mut(), schema.get("description")) {
                obj.insert("description".into(), desc.clone());
            }
            return out;
        }
    }
    schema.clone()
}

/// The type that isn't `null`, e.g. `string` for `["string", "null"]`.
fn main_type(schema: &Value) -> Option<&str> {
    match schema.get("type")? {
        Value::String(t) => Some(t),
        Value::Array(types) => types
            .iter()
            .filter_map(Value::as_str)
            .find(|t| *t != "null"),
        _ => None,
    }
}

fn plain(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Allowed values from `enum`, or `oneOf` items that are each a `const` or an `enum`.
fn choices_of(root: &Value, schema: &Value) -> Vec<(String, String)> {
    let from_enum = |values: &Vec<Value>| -> Vec<(String, String)> {
        values
            .iter()
            .filter(|v| !v.is_null())
            .map(|v| (plain(v), plain(v)))
            .collect()
    };
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        return from_enum(values);
    }
    let Some(one) = schema.get("oneOf").and_then(Value::as_array) else {
        return vec![];
    };
    let mut out = vec![];
    for raw in one {
        let option = resolve(root, raw);
        if let Some(value) = option.get("const") {
            let why = option
                .get("description")
                .and_then(Value::as_str)
                .map_or(String::new(), |d| format!(" — {d}"));
            out.push((plain(value), format!("{}{why}", plain(value))));
        } else if let Some(values) = option.get("enum").and_then(Value::as_array) {
            out.extend(from_enum(values));
        } else {
            return vec![];
        }
    }
    out
}

fn placeholder(default: Option<&Value>, kind: FieldKind) -> String {
    match default {
        None | Some(Value::Null) => String::new(),
        Some(Value::Array(lines)) if kind == FieldKind::Lines => {
            lines.iter().map(plain).collect::<Vec<_>>().join("\n")
        }
        Some(value) => plain(value),
    }
}

/// The `config:` block from what was typed. Empty inputs are left out, so the
/// module's defaults apply. Errors are by field key.
pub fn build_config(
    fields: &[Field],
    values: &HashMap<String, String>,
) -> (Map<String, Value>, HashMap<String, String>) {
    let mut config = Map::new();
    let mut errors = HashMap::new();
    for field in fields {
        let raw = values.get(&field.key).map_or("", |v| v.trim());
        if raw.is_empty() {
            if field.required {
                errors.insert(field.key.clone(), "Required.".to_string());
            }
            continue;
        }
        let key = field.key.clone();
        match field.kind {
            FieldKind::Integer => match raw.parse::<i64>() {
                Ok(n) => {
                    config.insert(key, json!(n));
                }
                Err(_) => {
                    errors.insert(key, "Not a whole number.".to_string());
                }
            },
            FieldKind::Number => match raw.parse::<f64>() {
                Ok(n) if n.is_finite() => {
                    config.insert(key, json!(n));
                }
                _ => {
                    errors.insert(key, "Not a number.".to_string());
                }
            },
            FieldKind::Boolean => {
                config.insert(key, json!(raw == "true"));
            }
            FieldKind::Lines => {
                let lines: Vec<&str> = raw
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .collect();
                config.insert(key, json!(lines));
            }
            FieldKind::Json => match serde_json::from_str::<Value>(raw) {
                Ok(value) => {
                    config.insert(key, value);
                }
                Err(_) => {
                    errors.insert(key, "Not valid JSON.".to_string());
                }
            },
            FieldKind::Text | FieldKind::Choice => {
                config.insert(key, json!(raw));
            }
        }
    }
    (config, errors)
}

/// `id`, or `id-2`, `id-3`... the first one not in `taken`.
pub fn free_id(id: &str, taken: &HashSet<String>) -> String {
    if !taken.contains(id) {
        return id.to_string();
    }
    (2..)
        .map(|n| format!("{id}-{n}"))
        .find(|candidate| !taken.contains(candidate))
        .expect("there is always a free number")
}

/// What's wrong with a new module's ID, if anything.
pub fn id_error(id: &str, taken: &HashSet<String>) -> Option<String> {
    let id = id.trim();
    let mut chars = id.chars();
    let starts = chars.next().is_some_and(|c| c.is_ascii_alphabetic());
    if !starts || !chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Some("Letters, digits, - and _, starting with a letter.".to_string());
    }
    taken
        .contains(id)
        .then(|| format!("{id} is already in the config."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> Value {
        json!({
            "required": ["url"],
            "properties": {
                "url": { "type": "string", "description": "Where" },
                "limit": { "type": ["integer", "null"], "default": 5 },
                "ratio": { "type": "number" },
                "on": { "type": "boolean" },
                "repos": { "type": "array", "items": { "type": "string" }, "default": ["a", "b"] },
                "mode": { "$ref": "#/$defs/Mode" },
                "extra": { "type": "object" }
            },
            "$defs": { "Mode": { "enum": ["fast", "slow"] } }
        })
    }

    fn field<'a>(all: &'a [Field], key: &str) -> &'a Field {
        all.iter().find(|f| f.key == key).unwrap()
    }

    #[test]
    fn makes_a_field_for_each_kind() {
        let all = fields(Some(&schema()));
        assert_eq!(field(&all, "url").kind, FieldKind::Text);
        assert!(field(&all, "url").required);
        assert_eq!(field(&all, "limit").kind, FieldKind::Integer);
        assert_eq!(field(&all, "limit").placeholder, "5");
        assert_eq!(field(&all, "repos").kind, FieldKind::Lines);
        assert_eq!(field(&all, "repos").placeholder, "a\nb");
        assert_eq!(field(&all, "mode").kind, FieldKind::Choice);
        assert_eq!(field(&all, "extra").kind, FieldKind::Json);
    }

    #[test]
    fn builds_config_from_what_was_typed() {
        let all = fields(Some(&schema()));
        let values: HashMap<String, String> = [
            ("url", "https://x"),
            ("limit", "3"),
            ("repos", "a\n\n b "),
            ("on", "true"),
            ("extra", "{\"a\":1}"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .into();
        let (config, errors) = build_config(&all, &values);
        assert!(errors.is_empty());
        assert_eq!(
            Value::Object(config),
            json!({"url": "https://x", "limit": 3, "repos": ["a", "b"], "on": true, "extra": {"a": 1}})
        );
    }

    #[test]
    fn reports_bad_values_and_leaves_out_empty_ones() {
        let all = fields(Some(&schema()));
        let values: HashMap<String, String> = [("limit", "x"), ("extra", "{")]
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .into();
        let (config, errors) = build_config(&all, &values);
        assert!(config.is_empty());
        assert_eq!(errors["url"], "Required.");
        assert_eq!(errors["limit"], "Not a whole number.");
        assert_eq!(errors["extra"], "Not valid JSON.");
    }

    #[test]
    fn finds_a_free_id_and_checks_new_ones() {
        let taken: HashSet<String> = ["cpu".to_string(), "cpu-2".to_string()].into();
        assert_eq!(free_id("cpu", &taken), "cpu-3");
        assert_eq!(free_id("disk", &taken), "disk");
        assert!(id_error("cpu", &taken).is_some());
        assert!(id_error("2fast", &taken).is_some());
        assert!(id_error("ok_id-1", &taken).is_none());
    }
}
