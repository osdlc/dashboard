//! JSON-RPC 2.0 messages for stdio plugins. One message per line on stdin/stdout.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Manifest, View};

pub mod method {
    //! JSON-RPC method names.
    /// Host to plugin request: send the config, get the `Manifest` back.
    pub const INITIALIZE: &str = "initialize";
    /// Host to plugin request (poll mode): return the current `View`.
    pub const VIEW_RENDER: &str = "view/render";
    /// Host to plugin request: stop work and reply, then wait for `exit`.
    pub const SHUTDOWN: &str = "shutdown";
    /// Host to plugin notification: exit the process.
    pub const EXIT: &str = "exit";
    /// Plugin to host notification (push mode): a new `View`.
    pub const VIEW_UPDATE: &str = "view/update";
    /// Plugin to host notification: a line for the host log.
    pub const LOG: &str = "log";
}

pub mod error_code {
    //! Error codes a plugin may reply with.
    /// JSON-RPC's own code for a method the receiver does not know.
    pub const METHOD_NOT_FOUND: i64 = -32601;
    /// An upstream service or tool is unavailable, e.g. `gh` not logged in. The host shows `message` in the tile.
    pub const UPSTREAM_UNAVAILABLE: i64 = -32001;
    /// The module's config is invalid.
    pub const CONFIG_INVALID: i64 = -32002;
}

/// Who is running the plugin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HostInfo {
    /// The host's name, e.g. "sitroom".
    pub name: String,
    /// The host's version, e.g. "0.1.0".
    pub version: String,
}

/// Params of the `initialize` request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InitializeParams {
    /// The host's contract version, e.g. "0.1".
    pub contract_version: String,
    /// The host running the plugin.
    pub host: HostInfo,
    /// The module's `config:` block from the config file. Null when absent.
    #[serde(default)]
    pub config: serde_json::Value,
}

/// Result of the `initialize` request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InitializeResult {
    /// What the plugin declares about itself.
    pub manifest: Manifest,
}

/// Params of the `view/update` notification (push mode).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ViewUpdateParams {
    /// The new view.
    pub view: View,
}

/// How serious a log line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    /// Something failed.
    Error,
    /// Something looks wrong.
    Warn,
    /// Normal progress.
    Info,
    /// Detail for debugging.
    Debug,
}

/// Params of the `log` notification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LogParams {
    /// How serious it is.
    pub level: LogLevel,
    /// The text to log.
    pub message: String,
}

// ---- Envelope ----

/// A JSON-RPC request: expects a response with the same `id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// Always "2.0".
    pub jsonrpc: String,
    /// Matches the response to the request.
    pub id: u64,
    /// The method to call.
    pub method: String,
    /// The method's params, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// A JSON-RPC notification: no `id`, no response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    /// Always "2.0".
    pub jsonrpc: String,
    /// The method.
    pub method: String,
    /// The method's params, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// A JSON-RPC response: `result` on success, `error` on failure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    /// Always "2.0".
    pub jsonrpc: String,
    /// The `id` of the request it answers.
    pub id: u64,
    /// The method's result on success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// Why the request failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

/// A JSON-RPC error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcError {
    /// A JSON-RPC or `error_code` code.
    pub code: i64,
    /// Shown to the user.
    pub message: String,
    /// Extra detail, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Any message on the wire. Order matters for untagged matching:
/// a request has `id` + `method`, a response has `id` only, a notification `method` only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Message {
    /// A call that expects a response.
    Request(Request),
    /// The answer to a request.
    Response(Response),
    /// A message that expects no response.
    Notification(Notification),
}
