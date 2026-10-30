//! JSON-RPC 2.0 messages for stdio plugins. One message per line on stdin/stdout.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Manifest, View};

pub mod method {
    pub const INITIALIZE: &str = "initialize";
    pub const VIEW_RENDER: &str = "view/render";
    pub const SHUTDOWN: &str = "shutdown";
    pub const EXIT: &str = "exit";
    pub const VIEW_UPDATE: &str = "view/update";
    pub const LOG: &str = "log";
}

pub mod error_code {
    pub const UPSTREAM_UNAVAILABLE: i64 = -32001;
    pub const CONFIG_INVALID: i64 = -32002;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HostInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InitializeParams {
    pub contract_version: String,
    pub host: HostInfo,
    #[serde(default)]
    pub config: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InitializeResult {
    pub manifest: Manifest,
}

/// Params of the `view/update` notification (push mode).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ViewUpdateParams {
    pub view: View,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LogParams {
    pub level: LogLevel,
    pub message: String,
}

// ---- Envelope ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Any message on the wire. Order matters for untagged matching:
/// a request has `id` + `method`, a response has `id` only, a notification `method` only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Message {
    Request(Request),
    Response(Response),
    Notification(Notification),
}
