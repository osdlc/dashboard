//! Prints the JSON Schema for the web view's WebSocket messages, both ways,
//! and the catalog it fetches.
//! Used by `pnpm gen:types` in web/. Lives here, not in the CLI, so generating
//! types doesn't build the web crate (whose build script builds the UI).

use dashboard_contract::{CatalogResponse, ClientMessage, ServerMessage};
use schemars::JsonSchema;

/// Root type so both message enums end up in one schema file.
#[allow(dead_code)]
#[derive(JsonSchema)]
struct WebProtocol {
    server: ServerMessage,
    client: ClientMessage,
    catalog: CatalogResponse,
}

fn main() {
    let mut schema = serde_json::to_value(schemars::schema_for!(WebProtocol)).unwrap();
    wrap_refs(&mut schema);
    println!("{}", serde_json::to_string_pretty(&schema).unwrap());
}

/// json2ts reads a `$ref` with a `description` beside it as a new inline
/// type, so a documented field of type `Size` would lose the name `Size`.
/// Moving the `$ref` into `allOf` keeps both the name and the field's doc.
fn wrap_refs(value: &mut serde_json::Value) {
    use serde_json::{Value, json};
    match value {
        Value::Object(map) => {
            if map.len() > 1
                && let Some(target) = map.remove("$ref")
            {
                map.insert("allOf".into(), json!([{ "$ref": target }]));
            }
            map.values_mut().for_each(wrap_refs);
        }
        Value::Array(items) => items.iter_mut().for_each(wrap_refs),
        _ => {}
    }
}
