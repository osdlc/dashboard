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
    let schema = schemars::schema_for!(WebProtocol);
    println!("{}", serde_json::to_string_pretty(&schema).unwrap());
}
