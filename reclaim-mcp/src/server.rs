//! MCP method dispatch and the three tools (PRD §9):
//!   • `scan_disk(root)`            → ScanResult (§10 schema)
//!   • `propose_reclaim(root,policy)` → a staged-autonomy Proposal
//!   • `reclaim_space(root,ids,mode)` → ReclaimResult, via the core safety gate
//!
//! The server holds NO delete logic and NO bypass. `reclaim_space` builds plain
//! `ReclaimTarget`s and calls `reclaim_core::reclaim`, which re-scans the live
//! filesystem and re-validates every target's risk class + HOME boundary. An
//! agent can never widen what is deletable.

use crate::policy::{self, Policy};
use crate::protocol::{Request, Response, RpcError, INVALID_PARAMS, METHOD_NOT_FOUND, PARSE_ERROR};
use reclaim_core::{reclaim, safety, scan, Mode, ReclaimTarget};
use serde_json::{json, Value};
use std::path::PathBuf;

/// The protocol version this server speaks.
const PROTOCOL_VERSION: &str = "2025-06-18";

/// Parse one newline-delimited JSON-RPC message and produce the response to
/// write back, or `None` for notifications (which expect no reply).
pub fn handle_line(line: &str) -> Option<Response> {
    let req: Request = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            return Some(Response::err(
                Value::Null,
                RpcError::new(PARSE_ERROR, format!("parse error: {e}")),
            ));
        }
    };

    let is_notification = req.is_notification();
    let id = req.id.clone().unwrap_or(Value::Null);

    match dispatch(&req.method, &req.params) {
        // Notifications never get a reply, even on success.
        Ok(_) if is_notification => None,
        Ok(result) => Some(Response::ok(id, result)),
        Err(_) if is_notification => None,
        Err(e) => Some(Response::err(id, e)),
    }
}

fn dispatch(method: &str, params: &Value) -> Result<Value, RpcError> {
    match method {
        "initialize" => Ok(initialize_result()),
        // Lifecycle notifications we simply acknowledge with no-op.
        "notifications/initialized" | "notifications/cancelled" => Ok(Value::Null),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_schemas() })),
        "tools/call" => tools_call(params),
        other => Err(RpcError::new(
            METHOD_NOT_FOUND,
            format!("method not found: {other}"),
        )),
    }
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "reclaim-mcp", "version": env!("CARGO_PKG_VERSION") },
        "instructions":
            "Reclaim exposes a local, risk-aware disk-recovery engine. Call \
             scan_disk to see reclaimable items classified by delete-risk \
             (safe/review/protected), propose_reclaim to get a staged-autonomy \
             plan (Safe-class only), and reclaim_space to execute. Deletion is \
             confined to $HOME, defaults to move-to-Trash, and Protected items \
             can never be deleted — the core re-validates every target."
    })
}

// ── tools/call ───────────────────────────────────────────────────────────────

fn tools_call(params: &Value) -> Result<Value, RpcError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::new(INVALID_PARAMS, "tools/call requires `name`"))?;
    let args = params.get("arguments").cloned().unwrap_or(json!({}));

    let result = match name {
        "scan_disk" => call_scan_disk(&args),
        "propose_reclaim" => call_propose_reclaim(&args),
        "reclaim_space" => call_reclaim_space(&args),
        other => {
            return Err(RpcError::new(
                INVALID_PARAMS,
                format!("unknown tool: {other}"),
            ))
        }
    };
    Ok(result)
}

/// Build an MCP tool result. `text` is the human-readable summary; `structured`
/// is the typed payload (the §10 schema, a Proposal, or a ReclaimResult).
fn tool_result(text: String, structured: Value, is_error: bool) -> Value {
    json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": structured,
        "isError": is_error,
    })
}

fn tool_error(message: String) -> Value {
    tool_result(message, Value::Null, true)
}

/// `~/Library` by default — the same default as the CLI.
fn default_root() -> PathBuf {
    safety::home_dir().join("Library")
}

fn root_arg(args: &Value) -> PathBuf {
    args.get("root")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(default_root)
}

fn call_scan_disk(args: &Value) -> Value {
    let root = root_arg(args);
    match scan::scan(&root) {
        Ok(result) => {
            let summary = format!(
                "Scanned {}: {} reclaimable items, {} on disk under root.",
                result.root.display(),
                result.items.len(),
                human(result.root_total_bytes),
            );
            tool_result(summary, to_value(&result), false)
        }
        Err(e) => tool_error(format!("scan failed for {}: {e}", root.display())),
    }
}

fn call_propose_reclaim(args: &Value) -> Value {
    let root = root_arg(args);
    let policy = args
        .get("policy")
        .cloned()
        .map(|v| serde_json::from_value::<Policy>(v).unwrap_or_default())
        .unwrap_or_default();

    match scan::scan(&root) {
        Ok(scanned) => {
            let proposal = policy::propose(&scanned, policy);
            let summary = format!(
                "{:?} proposal: {} Safe item(s), ~{} reclaimable to Trash. {}",
                proposal.policy,
                proposal.item_count,
                human(proposal.projected_freed_bytes),
                if proposal.auto_executable {
                    "Policy may execute this automatically."
                } else {
                    "Human approval required before reclaim_space."
                },
            );
            tool_result(summary, to_value(&proposal), false)
        }
        Err(e) => tool_error(format!("scan failed for {}: {e}", root.display())),
    }
}

fn call_reclaim_space(args: &Value) -> Value {
    let root = root_arg(args);

    let Some(ids) = args.get("ids").and_then(Value::as_array) else {
        return tool_error("reclaim_space requires `ids` (array of item IDs)".into());
    };

    // Mode applies to the whole batch. Defaults to Trash (reversible); the core
    // honors per-target mode and re-validates regardless of what is requested.
    let mode = args
        .get("mode")
        .cloned()
        .map(|v| serde_json::from_value::<Mode>(v).unwrap_or_default())
        .unwrap_or_default();

    let targets: Vec<ReclaimTarget> = ids
        .iter()
        .filter_map(Value::as_str)
        .map(|id| ReclaimTarget {
            id: id.to_string(),
            mode,
        })
        .collect();

    if targets.is_empty() {
        return tool_error("reclaim_space: `ids` contained no string IDs".into());
    }

    match reclaim::reclaim(&root, &targets) {
        Ok(result) => {
            let summary = format!(
                "Freed {}: {} trashed, {} permanently deleted, {} skipped (gate/missing/failed).",
                human(result.freed_bytes),
                result.moved_to_trash.len(),
                result.permanently_deleted.len(),
                result.skipped.len(),
            );
            tool_result(summary, to_value(&result), false)
        }
        Err(e) => tool_error(format!("reclaim failed for {}: {e}", root.display())),
    }
}

fn to_value<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).expect("core types serialize")
}

// ── tool schemas ─────────────────────────────────────────────────────────────

fn tool_schemas() -> Value {
    json!([
        {
            "name": "scan_disk",
            "description":
                "Scan a directory tree (under $HOME) and classify every reclaimable \
                 item by delete-risk: safe (regenerates, no loss), review (reclaimable \
                 with a cost), or protected (real data — never deletable). Read-only.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "root": {
                        "type": "string",
                        "description": "Absolute path to scan. Must be inside $HOME. Defaults to ~/Library."
                    }
                }
            }
        },
        {
            "name": "propose_reclaim",
            "description":
                "Produce a staged-autonomy reclaim plan from a fresh scan. Candidates \
                 are Safe-class items ONLY; Review and Protected are never proposed for \
                 automation. Modes: shadow (report only), assisted (human approves), \
                 auto_safe (may self-execute Safe items to Trash). Never proposes \
                 permanent deletion.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "root": {
                        "type": "string",
                        "description": "Absolute path to scan. Defaults to ~/Library."
                    },
                    "policy": {
                        "type": "string",
                        "enum": ["shadow", "assisted", "auto_safe"],
                        "description": "Autonomy stage. Defaults to shadow (safest)."
                    }
                }
            }
        },
        {
            "name": "reclaim_space",
            "description":
                "Reclaim the given item IDs. Goes through the SAME core safety gate as \
                 the UI: the core re-scans, re-derives each item's risk class and path, \
                 and refuses anything Protected or outside $HOME. Defaults to \
                 move-to-Trash (reversible). `permanent` is opt-in and must be a \
                 deliberate human choice — it is never produced by propose_reclaim.",
            "inputSchema": {
                "type": "object",
                "required": ["ids"],
                "properties": {
                    "root": {
                        "type": "string",
                        "description": "Root the IDs were scanned under. Defaults to ~/Library."
                    },
                    "ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Item IDs from scan_disk/propose_reclaim to reclaim."
                    },
                    "mode": {
                        "type": "string",
                        "enum": ["trash", "permanent"],
                        "description": "trash (default, reversible) or permanent (irreversible, opt-in)."
                    }
                }
            }
        }
    ])
}

/// Human-readable byte size for tool summaries.
fn human(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reclaim_core::reclaim::{reclaim_with, Executor};
    use reclaim_core::{Item, Risk};
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    fn parse(line: &str) -> Option<Value> {
        super::handle_line(line).map(|r| serde_json::to_value(&r).unwrap())
    }

    #[test]
    fn initialize_advertises_tools_capability() {
        let resp = parse(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#).unwrap();
        assert_eq!(resp["id"], json!(1));
        assert_eq!(resp["result"]["serverInfo"]["name"], "reclaim-mcp");
        assert!(resp["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn notification_gets_no_response() {
        assert!(super::handle_line(
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#
        )
        .is_none());
    }

    #[test]
    fn tools_list_exposes_the_three_tools() {
        let resp = parse(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#).unwrap();
        let names: Vec<&str> = resp["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["scan_disk", "propose_reclaim", "reclaim_space"]);
    }

    #[test]
    fn unknown_method_is_a_protocol_error() {
        let resp = parse(r#"{"jsonrpc":"2.0","id":3,"method":"nope"}"#).unwrap();
        assert_eq!(resp["error"]["code"], json!(METHOD_NOT_FOUND));
    }

    #[test]
    fn scan_disk_over_empty_tree_returns_structured_result() {
        let dir = tempfile::tempdir().unwrap();
        let call = json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": { "name": "scan_disk", "arguments": { "root": dir.path() } }
        });
        let resp = parse(&call.to_string()).unwrap();
        let result = &resp["result"];
        assert_eq!(result["isError"], json!(false));
        // §10 schema shape is present.
        assert!(result["structuredContent"]["items"].is_array());
        assert!(result["structuredContent"]["disk"].is_object());
    }

    #[test]
    fn reclaim_space_without_ids_is_a_tool_error() {
        let call = json!({
            "jsonrpc": "2.0", "id": 5, "method": "tools/call",
            "params": { "name": "reclaim_space", "arguments": { "root": "/tmp" } }
        });
        let resp = parse(&call.to_string()).unwrap();
        assert_eq!(resp["result"]["isError"], json!(true));
    }

    // The load-bearing guarantee (mcp-developer.md): an agent's reclaim request
    // containing a Protected ID is rejected by the SAME gate as a UI request.
    // The MCP layer delegates to `reclaim_core::reclaim`; here we drive that core
    // path directly with a fake executor and the exact targets an agent sends.
    struct FakeExec(RefCell<Vec<PathBuf>>);
    impl Executor for FakeExec {
        fn to_trash(&self, path: &Path) -> std::io::Result<()> {
            self.0.borrow_mut().push(path.to_path_buf());
            Ok(())
        }
        fn permanent(&self, path: &Path) -> std::io::Result<()> {
            self.0.borrow_mut().push(path.to_path_buf());
            Ok(())
        }
    }

    #[test]
    fn agent_request_for_protected_id_is_rejected_by_the_gate() {
        let home = tempfile::tempdir().unwrap();
        let safe_path = home.path().join("safe");
        let protected_path = home.path().join("protected");
        std::fs::write(&safe_path, b"x").unwrap();
        std::fs::write(&protected_path, b"x").unwrap();

        let authoritative = vec![
            Item {
                id: "safe-1".into(),
                name: "safe".into(),
                path: safe_path.clone(),
                bytes: 1,
                risk: Risk::Safe,
                detector: "test".into(),
                rationale: String::new(),
                reversible: true,
            },
            Item {
                id: "protected-1".into(),
                name: "protected".into(),
                path: protected_path.clone(),
                bytes: 1,
                risk: Risk::Protected,
                detector: "test".into(),
                rationale: String::new(),
                reversible: false,
            },
        ];

        // The agent asks to reclaim BOTH — including the Protected one.
        let targets = vec![
            ReclaimTarget { id: "safe-1".into(), mode: Mode::Trash },
            ReclaimTarget { id: "protected-1".into(), mode: Mode::Permanent },
        ];

        let exec = FakeExec(RefCell::new(Vec::new()));
        let result = reclaim_with(&authoritative, &targets, home.path(), &exec);

        // Safe item trashed; Protected item skipped by the gate, never deleted.
        assert_eq!(result.moved_to_trash.len(), 1);
        assert_eq!(result.moved_to_trash[0].id, "safe-1");
        assert!(result.permanently_deleted.is_empty());
        assert_eq!(result.skipped.len(), 1);
        assert_eq!(result.skipped[0].id, "protected-1");
        assert!(!exec.0.borrow().iter().any(|p| p == &protected_path));
    }
}
