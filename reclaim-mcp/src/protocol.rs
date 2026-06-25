//! Minimal JSON-RPC 2.0 types for the MCP stdio transport.
//!
//! MCP messages are newline-delimited JSON on stdin/stdout (no Content-Length
//! framing). This module models just the request/response envelope; the method
//! payloads are plain `serde_json::Value` so the server stays decoupled from the
//! evolving MCP schema.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A decoded JSON-RPC request (or notification, when `id` is absent).
#[derive(Debug, Deserialize)]
pub struct Request {
    #[allow(dead_code)]
    pub jsonrpc: Option<String>,
    /// Present for requests, absent for notifications.
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl Request {
    /// A notification expects no response.
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

/// A JSON-RPC response. Exactly one of `result` / `error` is set.
#[derive(Debug, Serialize)]
pub struct Response {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl Response {
    pub fn ok(id: Value, result: Value) -> Self {
        Response {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: Value, error: RpcError) -> Self {
        Response {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(error),
        }
    }
}

/// A JSON-RPC error object. These are *protocol* errors (bad method, parse
/// failure); a tool that runs but fails reports that in its result with
/// `isError: true`, not here.
#[derive(Debug, Serialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl RpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        RpcError {
            code,
            message: message.into(),
        }
    }
}

// Standard JSON-RPC error codes.
pub const PARSE_ERROR: i64 = -32700;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
