# MCP Dev Utilities — Implementation Plan (Slice 1)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate `reclaim-mcp` from its hand-rolled JSON-RPC loop to the official `rmcp` SDK, then add the first slice of offline developer-utility tools (JSON, encode/hash, time).

**Architecture:** `reclaim-mcp` becomes an async `rmcp` stdio server. A single `ReclaimServer` struct owns one combined `ToolRouter`, assembled from several per-feature `#[tool_router]` impl blocks (one file each). The three existing disk tools keep delegating to `reclaim-core` (safety gate untouched). New utility tools are pure functions with thin `#[tool]` wrappers — no filesystem, no network.

**Tech Stack:** Rust, `rmcp` 2.x (`server`/`macros`/`transport-io`), `tokio`, `schemars` 1, `serde`/`serde_json`, `base64`, `hex`, `md-5`, `sha1`, `sha2`, `crc32fast`, `percent-encoding`, `chrono`, `chrono-tz`.

## Global Constraints

- **Edition / workspace:** `reclaim-mcp` stays a member of the root workspace (`Cargo.toml` `members`), edition 2021.
- **Thin client over `reclaim-core`:** the three disk tools hold NO delete logic; `reclaim_space` calls `reclaim_core::reclaim` so the identical safety gate applies. Do not change `reclaim-core`.
- **Local and silent:** no network calls anywhere. New utility tools touch neither the filesystem nor the network.
- **Inline-string input only** for the new utilities (no file paths).
- **Tool errors, not panics:** every tool maps bad input to an `Err(String)` (an MCP tool error / `isError`), never a panic, never a protocol error.
- **Logs to stderr only:** stdout is the protocol stream. Use `eprintln!` for any logging.
- **Low tool count:** encoders use a `scheme`/`algo` enum argument rather than one tool per algorithm.
- **rmcp dependency line (use everywhere it's referenced):**
  `rmcp = { version = "2", features = ["server", "macros", "transport-io"] }`

---

### Task 1: Migrate `reclaim-mcp` to the `rmcp` SDK (the 3 disk tools)

Foundational task. Replaces the manual JSON-RPC loop with an `rmcp` stdio server exposing the existing `scan_disk` / `propose_reclaim` / `reclaim_space` tools with identical behavior. Folds in: Cargo deps, the `ReclaimServer` struct, `get_info`, `main()`, deletion of `protocol.rs`/`server.rs`.

**Files:**
- Modify: `reclaim-mcp/Cargo.toml`
- Create: `reclaim-mcp/src/server.rs` (struct, router combine, `ServerHandler`/`get_info`)
- Create: `reclaim-mcp/src/reclaim_tools.rs` (the 3 disk tools + preserved gate test)
- Rewrite: `reclaim-mcp/src/main.rs` (tokio main, serve stdio)
- Delete: `reclaim-mcp/src/protocol.rs`, `reclaim-mcp/src/server.rs` (old dispatch — superseded; see note)
- Keep unchanged: `reclaim-mcp/src/policy.rs`

> Note: the old `server.rs` (manual dispatch) is removed and a NEW `server.rs` (rmcp server struct) takes its place. Simplest: overwrite the file's contents.

**Interfaces:**
- Produces:
  - `pub struct ReclaimServer { tool_router: rmcp::handler::server::router::tool::ToolRouter<Self> }`
  - `impl ReclaimServer { pub fn new() -> Self }` — `tool_router: Self::reclaim_router()` (later tasks extend this sum with `+ Self::<feature>_router()`)
  - `#[tool_router(router = reclaim_router)] impl ReclaimServer { … }` exposing tools `scan_disk`, `propose_reclaim`, `reclaim_space`
  - Helper (in `reclaim_tools.rs`, used by later wrappers too): none required cross-module.

- [ ] **Step 1: Update `reclaim-mcp/Cargo.toml` dependencies**

Replace the `[dependencies]` and `[dev-dependencies]` sections with:

```toml
[dependencies]
reclaim-core = { path = "../reclaim-core" }
rmcp = { version = "2", features = ["server", "macros", "transport-io"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "io-std"] }
schemars = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Create `reclaim-mcp/src/server.rs`**

```rust
//! The `rmcp` server: one `ReclaimServer` owning a combined ToolRouter.
//!
//! Per-feature `#[tool_router(router = …)]` impl blocks live in sibling files
//! (`reclaim_tools.rs`, `tools/*.rs`); `new()` sums their routers. The server is
//! a thin client over `reclaim-core` for disk tools and holds only pure logic
//! for the utility tools — no network, and (for utilities) no filesystem.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{
    Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::{tool_handler, ServerHandler};

#[derive(Clone)]
pub struct ReclaimServer {
    pub(crate) tool_router: ToolRouter<Self>,
}

impl Default for ReclaimServer {
    fn default() -> Self {
        Self::new()
    }
}

impl ReclaimServer {
    pub fn new() -> Self {
        Self {
            // Later tasks extend this sum: `+ Self::json_router()` etc.
            tool_router: Self::reclaim_router(),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ReclaimServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2025_06_18,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "Reclaim exposes a local, risk-aware disk-recovery engine plus \
                 offline developer utilities. Disk tools: scan_disk, \
                 propose_reclaim, reclaim_space — deletion is confined to $HOME, \
                 defaults to move-to-Trash, and Protected items can never be \
                 deleted (the core re-validates every target). Utility tools \
                 (json_*, encode/decode/hash, time_*) are pure and operate on \
                 inline strings only."
                    .to_string(),
            ),
            ..Default::default()
        }
    }
}
```

> If `ProtocolVersion::V_2025_06_18` does not exist in the installed rmcp, use the newest `ProtocolVersion::V_*` variant the crate exposes (check `cargo doc -p rmcp --open` or compiler suggestions). The protocol version is not contract-critical here.

- [ ] **Step 3: Create `reclaim-mcp/src/reclaim_tools.rs` with the three disk tools**

```rust
//! The three disk tools, ported to rmcp. Each delegates to `reclaim-core`;
//! `reclaim_space` goes through the core safety gate exactly as before.

use crate::policy::{self, Policy};
use crate::server::ReclaimServer;
use reclaim_core::{reclaim, safety, scan, Mode, ReclaimTarget};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router, Json};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScanArgs {
    /// Absolute path to scan. Must be inside $HOME. Defaults to ~/Library.
    pub root: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProposeArgs {
    pub root: Option<String>,
    /// Autonomy stage: shadow (default), assisted, auto_safe.
    pub policy: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReclaimArgs {
    pub root: Option<String>,
    /// Item IDs from scan_disk/propose_reclaim to reclaim.
    pub ids: Vec<String>,
    /// "trash" (default, reversible) or "permanent" (irreversible, opt-in).
    pub mode: Option<String>,
}

fn default_root() -> PathBuf {
    safety::home_dir().join("Library")
}

fn root_of(root: Option<String>) -> PathBuf {
    root.map(PathBuf::from).unwrap_or_else(default_root)
}

#[tool_router(router = reclaim_router)]
impl ReclaimServer {
    #[tool(
        name = "scan_disk",
        description = "Scan a directory tree (under $HOME) and classify every reclaimable item by delete-risk: safe (regenerates, no loss), review (reclaimable with a cost), or protected (real data — never deletable). Read-only."
    )]
    pub async fn scan_disk(&self, params: Parameters<ScanArgs>) -> Result<Json<Value>, String> {
        let root = root_of(params.0.root);
        let result = scan::scan(&root).map_err(|e| format!("scan failed for {}: {e}", root.display()))?;
        let value = serde_json::to_value(&result).map_err(|e| e.to_string())?;
        Ok(Json(value))
    }

    #[tool(
        name = "propose_reclaim",
        description = "Produce a staged-autonomy reclaim plan from a fresh scan. Candidates are Safe-class items ONLY; Review and Protected are never proposed. Policies: shadow (report only), assisted (human approves), auto_safe (may self-execute Safe items to Trash). Never proposes permanent deletion."
    )]
    pub async fn propose_reclaim(&self, params: Parameters<ProposeArgs>) -> Result<Json<Value>, String> {
        let root = root_of(params.0.root);
        let policy: Policy = params
            .0
            .policy
            .map(|p| serde_json::from_value(Value::String(p)).unwrap_or_default())
            .unwrap_or_default();
        let scanned = scan::scan(&root).map_err(|e| format!("scan failed for {}: {e}", root.display()))?;
        let proposal = policy::propose(&scanned, policy);
        let value = serde_json::to_value(&proposal).map_err(|e| e.to_string())?;
        Ok(Json(value))
    }

    #[tool(
        name = "reclaim_space",
        description = "Reclaim the given item IDs through the SAME core safety gate as the UI: the core re-scans, re-derives each item's risk class and path, and refuses anything Protected or outside $HOME. Defaults to move-to-Trash (reversible). `permanent` is opt-in."
    )]
    pub async fn reclaim_space(&self, params: Parameters<ReclaimArgs>) -> Result<Json<Value>, String> {
        let root = root_of(params.0.root);
        if params.0.ids.is_empty() {
            return Err("reclaim_space: `ids` is empty".to_string());
        }
        let mode: Mode = params
            .0
            .mode
            .map(|m| serde_json::from_value(Value::String(m)).unwrap_or_default())
            .unwrap_or_default();
        let targets: Vec<ReclaimTarget> = params
            .0
            .ids
            .into_iter()
            .map(|id| ReclaimTarget { id, mode })
            .collect();
        let result = reclaim::reclaim(&root, &targets)
            .map_err(|e| format!("reclaim failed for {}: {e}", root.display()))?;
        let value = serde_json::to_value(&result).map_err(|e| e.to_string())?;
        Ok(Json(value))
    }
}

#[cfg(test)]
mod tests {
    use crate::server::ReclaimServer;
    use reclaim_core::reclaim::{reclaim_with, Executor};
    use reclaim_core::{Item, Mode, ReclaimTarget, Risk};
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    #[test]
    fn reclaim_router_exposes_the_three_disk_tools() {
        let names: Vec<String> = ReclaimServer::reclaim_router()
            .list_all()
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        assert!(names.contains(&"scan_disk".to_string()));
        assert!(names.contains(&"propose_reclaim".to_string()));
        assert!(names.contains(&"reclaim_space".to_string()));
    }

    // Preserved verbatim from the old server.rs: the load-bearing guarantee that
    // an agent's reclaim request containing a Protected ID is rejected by the
    // SAME core gate as a UI request.
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

        let targets = vec![
            ReclaimTarget { id: "safe-1".into(), mode: Mode::Trash },
            ReclaimTarget { id: "protected-1".into(), mode: Mode::Permanent },
        ];

        let exec = FakeExec(RefCell::new(Vec::new()));
        let result = reclaim_with(&authoritative, &targets, home.path(), &exec);

        assert_eq!(result.moved_to_trash.len(), 1);
        assert_eq!(result.moved_to_trash[0].id, "safe-1");
        assert!(result.permanently_deleted.is_empty());
        assert_eq!(result.skipped.len(), 1);
        assert_eq!(result.skipped[0].id, "protected-1");
        assert!(!exec.0.borrow().iter().any(|p| p == &protected_path));
    }
}
```

- [ ] **Step 4: Rewrite `reclaim-mcp/src/main.rs`**

```rust
//! `reclaim-mcp` — an MCP server exposing the Reclaim engine plus offline
//! developer utilities to agents, over the `rmcp` stdio transport.
//!
//! Disk tools delegate to `reclaim-core` and its safety gate; utility tools are
//! pure (no fs, no network). Logs go to stderr so they never corrupt stdout.

mod policy;
mod reclaim_tools;
mod server;
mod tools;

use rmcp::transport::stdio;
use rmcp::ServiceExt;
use server::ReclaimServer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    eprintln!("reclaim-mcp {} starting on stdio", env!("CARGO_PKG_VERSION"));
    let service = ReclaimServer::new().serve(stdio()).await.inspect_err(|e| {
        eprintln!("reclaim-mcp: serving error: {e:?}");
    })?;
    service.waiting().await?;
    Ok(())
}
```

> `mod tools;` is declared now but its file is created in Task 2. To keep Task 1 compiling on its own, also do Step 5.

- [ ] **Step 5: Create an empty `reclaim-mcp/src/tools/mod.rs` placeholder**

```rust
//! Offline developer-utility tool modules. Each declares a
//! `#[tool_router(router = …)]` impl block on `ReclaimServer`.
```

(Feature modules are added here in Tasks 2–4.)

- [ ] **Step 6: Delete the obsolete files**

```bash
git rm reclaim-mcp/src/protocol.rs
rm -f reclaim-mcp/src/server.rs.orig
```

(The old `server.rs` content was overwritten in Step 2, so only `protocol.rs` needs removing.)

- [ ] **Step 7: Build and run the test suite**

Run: `cargo test -p reclaim-mcp`
Expected: PASS — including `reclaim_router_exposes_the_three_disk_tools` and `agent_request_for_protected_id_is_rejected_by_the_gate`. Also run `cargo test -p reclaim-core` to confirm the core is untouched (its tests still pass).

- [ ] **Step 8: Clippy + smoke-test the stdio handshake**

Run: `cargo clippy -p reclaim-mcp --all-targets -- -D warnings`
Expected: clean.

Smoke test the protocol (server should print serverInfo and a tools list containing the 3 tools):

```bash
printf '%s\n%s\n%s\n' \
 '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
 '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
 '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
 | cargo run -q -p reclaim-mcp 2>/dev/null
```
Expected: two JSON lines on stdout — an `initialize` result with `serverInfo.name == "reclaim-mcp"`, and a `tools/list` result whose `tools[].name` includes `scan_disk`, `propose_reclaim`, `reclaim_space`.

- [ ] **Step 9: Commit**

```bash
git add reclaim-mcp/Cargo.toml reclaim-mcp/src/main.rs reclaim-mcp/src/server.rs reclaim-mcp/src/reclaim_tools.rs reclaim-mcp/src/tools/mod.rs Cargo.lock
git rm reclaim-mcp/src/protocol.rs
git commit -m "refactor(mcp): migrate reclaim-mcp to the rmcp SDK (disk tools unchanged)"
```

---

### Task 2: JSON tools (`json_prettify`, `json_minify`, `json_compare`, `json_validate`)

**Files:**
- Create: `reclaim-mcp/src/tools/json.rs`
- Modify: `reclaim-mcp/src/tools/mod.rs` (add `pub mod json;`)
- Modify: `reclaim-mcp/src/server.rs` (`new()`: `+ Self::json_router()`)
- Test: inline `#[cfg(test)]` in `reclaim-mcp/src/tools/json.rs`

**Interfaces:**
- Consumes: `crate::server::ReclaimServer`.
- Produces: `#[tool_router(router = json_router)] impl ReclaimServer` with tools `json_prettify`, `json_minify`, `json_compare`, `json_validate`; pure fns `prettify`, `minify`, `validate`, `compare`.

- [ ] **Step 1: Write the failing tests** (append to `reclaim-mcp/src/tools/json.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prettify_uses_requested_indent() {
        let out = prettify(r#"{"a":1}"#, &Indent::Four).unwrap();
        assert_eq!(out, "{\n    \"a\": 1\n}");
        let tabbed = prettify(r#"{"a":1}"#, &Indent::Tab).unwrap();
        assert_eq!(tabbed, "{\n\t\"a\": 1\n}");
    }

    #[test]
    fn prettify_rejects_invalid_json() {
        assert!(prettify("{nope}", &Indent::Two).is_err());
    }

    #[test]
    fn minify_strips_whitespace() {
        let out = minify("{\n  \"a\": [1, 2]\n}").unwrap();
        assert_eq!(out, r#"{"a":[1,2]}"#);
    }

    #[test]
    fn validate_reports_error_location() {
        let ok = validate(r#"{"a":1}"#);
        assert!(ok.valid && ok.error.is_none());
        let bad = validate("{\n  \"a\": }");
        assert!(!bad.valid);
        let e = bad.error.unwrap();
        assert!(e.line >= 1 && e.column >= 1);
    }

    #[test]
    fn compare_equal_objects_ignores_key_order() {
        let c = compare(r#"{"a":1,"b":2}"#, r#"{"b":2,"a":1}"#).unwrap();
        assert!(c.equal);
        assert!(c.diffs.is_empty());
    }

    #[test]
    fn compare_reports_added_removed_changed_and_typechange() {
        let c = compare(
            r#"{"keep":1,"gone":2,"num":3,"t":1}"#,
            r#"{"keep":1,"num":4,"t":"s","new":9}"#,
        )
        .unwrap();
        assert!(!c.equal);
        let kinds: std::collections::BTreeMap<String, String> = c
            .diffs
            .iter()
            .map(|d| (d.path.clone(), d.kind.clone()))
            .collect();
        assert_eq!(kinds.get("/gone").unwrap(), "removed");
        assert_eq!(kinds.get("/new").unwrap(), "added");
        assert_eq!(kinds.get("/num").unwrap(), "changed");
        assert_eq!(kinds.get("/t").unwrap(), "type_changed");
    }

    #[test]
    fn compare_arrays_are_index_sensitive() {
        let c = compare(r#"[1,2,3]"#, r#"[1,9,3]"#).unwrap();
        assert_eq!(c.diffs.len(), 1);
        assert_eq!(c.diffs[0].path, "/1");
        assert_eq!(c.diffs[0].kind, "changed");
    }

    #[test]
    fn json_router_lists_four_tools() {
        let names: Vec<String> = ReclaimServer::json_router()
            .list_all()
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        for n in ["json_prettify", "json_minify", "json_compare", "json_validate"] {
            assert!(names.contains(&n.to_string()), "missing {n}");
        }
    }
}
```

- [ ] **Step 2: Run tests to confirm they fail**

Run: `cargo test -p reclaim-mcp json`
Expected: FAIL — `prettify`, `minify`, etc. not yet defined.

- [ ] **Step 3: Implement the pure functions and types** (top of `reclaim-mcp/src/tools/json.rs`)

```rust
//! JSON tools: prettify, minify, semantic compare, validate. Pure functions on
//! inline strings; object keys compare order-insensitively, arrays by index.

use crate::server::ReclaimServer;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router, Json};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub enum Indent {
    Two,
    Four,
    Tab,
}

pub fn parse_indent(s: &str) -> Indent {
    match s {
        "4" => Indent::Four,
        "tab" => Indent::Tab,
        _ => Indent::Two,
    }
}

fn indent_bytes(i: &Indent) -> &'static [u8] {
    match i {
        Indent::Two => b"  ",
        Indent::Four => b"    ",
        Indent::Tab => b"\t",
    }
}

pub fn prettify(json: &str, indent: &Indent) -> Result<String, String> {
    let value: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(indent_bytes(indent));
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
    use serde::Serialize as _;
    value.serialize(&mut ser).map_err(|e| e.to_string())?;
    String::from_utf8(buf).map_err(|e| e.to_string())
}

pub fn minify(json: &str) -> Result<String, String> {
    let value: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    serde_json::to_string(&value).map_err(|e| e.to_string())
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ParseError {
    pub message: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ValidateOut {
    pub valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ParseError>,
}

pub fn validate(json: &str) -> ValidateOut {
    match serde_json::from_str::<Value>(json) {
        Ok(_) => ValidateOut { valid: true, error: None },
        Err(e) => ValidateOut {
            valid: false,
            error: Some(ParseError {
                message: e.to_string(),
                line: e.line(),
                column: e.column(),
            }),
        },
    }
}

#[derive(Debug, Serialize, JsonSchema, PartialEq)]
pub struct Diff {
    pub path: String,
    /// "added" | "removed" | "changed" | "type_changed"
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right: Option<Value>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CompareOut {
    pub equal: bool,
    pub diffs: Vec<Diff>,
}

fn type_tag(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Escape a JSON Pointer reference token (RFC 6901): ~ -> ~0, / -> ~1.
fn esc(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

fn diff_into(path: &str, l: &Value, r: &Value, out: &mut Vec<Diff>) {
    if type_tag(l) != type_tag(r) {
        out.push(Diff {
            path: path.to_string(),
            kind: "type_changed".into(),
            left: Some(l.clone()),
            right: Some(r.clone()),
        });
        return;
    }
    match (l, r) {
        (Value::Object(lo), Value::Object(ro)) => {
            for (k, lv) in lo {
                let child = format!("{path}/{}", esc(k));
                match ro.get(k) {
                    Some(rv) => diff_into(&child, lv, rv, out),
                    None => out.push(Diff {
                        path: child,
                        kind: "removed".into(),
                        left: Some(lv.clone()),
                        right: None,
                    }),
                }
            }
            for (k, rv) in ro {
                if !lo.contains_key(k) {
                    out.push(Diff {
                        path: format!("{path}/{}", esc(k)),
                        kind: "added".into(),
                        left: None,
                        right: Some(rv.clone()),
                    });
                }
            }
        }
        (Value::Array(la), Value::Array(ra)) => {
            let common = la.len().min(ra.len());
            for i in 0..common {
                diff_into(&format!("{path}/{i}"), &la[i], &ra[i], out);
            }
            for (i, lv) in la.iter().enumerate().skip(common) {
                out.push(Diff {
                    path: format!("{path}/{i}"),
                    kind: "removed".into(),
                    left: Some(lv.clone()),
                    right: None,
                });
            }
            for (i, rv) in ra.iter().enumerate().skip(common) {
                out.push(Diff {
                    path: format!("{path}/{i}"),
                    kind: "added".into(),
                    left: None,
                    right: Some(rv.clone()),
                });
            }
        }
        _ => {
            if l != r {
                out.push(Diff {
                    path: path.to_string(),
                    kind: "changed".into(),
                    left: Some(l.clone()),
                    right: Some(r.clone()),
                });
            }
        }
    }
}

pub fn compare(left: &str, right: &str) -> Result<CompareOut, String> {
    let l: Value = serde_json::from_str(left).map_err(|e| format!("left: {e}"))?;
    let r: Value = serde_json::from_str(right).map_err(|e| format!("right: {e}"))?;
    let mut diffs = Vec::new();
    diff_into("", &l, &r, &mut diffs);
    Ok(CompareOut { equal: diffs.is_empty(), diffs })
}
```

- [ ] **Step 4: Add the `#[tool]` wrappers + request/response structs** (same file, below the pure fns)

```rust
#[derive(Debug, Deserialize, JsonSchema)]
pub struct PrettifyReq {
    pub json: String,
    /// "2" (default), "4", or "tab".
    pub indent: Option<String>,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct PrettifyResp {
    pub pretty: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct JsonReq {
    pub json: String,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct MinifyResp {
    pub minified: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompareReq {
    pub left: String,
    pub right: String,
}

#[tool_router(router = json_router)]
impl ReclaimServer {
    #[tool(name = "json_prettify", description = "Pretty-print JSON with a chosen indent (2/4/tab).")]
    pub async fn json_prettify(&self, p: Parameters<PrettifyReq>) -> Result<Json<PrettifyResp>, String> {
        let indent = parse_indent(p.0.indent.as_deref().unwrap_or("2"));
        Ok(Json(PrettifyResp { pretty: prettify(&p.0.json, &indent)? }))
    }

    #[tool(name = "json_minify", description = "Minify JSON, stripping all insignificant whitespace.")]
    pub async fn json_minify(&self, p: Parameters<JsonReq>) -> Result<Json<MinifyResp>, String> {
        Ok(Json(MinifyResp { minified: minify(&p.0.json)? }))
    }

    #[tool(name = "json_validate", description = "Validate JSON; report parse errors with line/column.")]
    pub async fn json_validate(&self, p: Parameters<JsonReq>) -> Result<Json<ValidateOut>, String> {
        Ok(Json(validate(&p.0.json)))
    }

    #[tool(name = "json_compare", description = "Semantic diff of two JSON docs: object keys order-insensitive, arrays index-sensitive. Reports added/removed/changed/type_changed paths.")]
    pub async fn json_compare(&self, p: Parameters<CompareReq>) -> Result<Json<CompareOut>, String> {
        Ok(Json(compare(&p.0.left, &p.0.right)?))
    }
}
```

- [ ] **Step 5: Register the module and router**

In `reclaim-mcp/src/tools/mod.rs` add:

```rust
pub mod json;
```

In `reclaim-mcp/src/server.rs`, change `new()`:

```rust
            tool_router: Self::reclaim_router() + Self::json_router(),
```

- [ ] **Step 6: Run tests + clippy**

Run: `cargo test -p reclaim-mcp json`
Expected: PASS (all 8 tests).
Run: `cargo clippy -p reclaim-mcp --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add reclaim-mcp/src/tools/json.rs reclaim-mcp/src/tools/mod.rs reclaim-mcp/src/server.rs
git commit -m "feat(mcp): add JSON tools (prettify, minify, compare, validate)"
```

---

### Task 3: Encode/hash tools (`encode`, `decode`, `hash`)

**Files:**
- Create: `reclaim-mcp/src/tools/encode.rs`
- Modify: `reclaim-mcp/src/tools/mod.rs` (add `pub mod encode;`)
- Modify: `reclaim-mcp/src/server.rs` (`new()`: `+ Self::encode_router()`)
- Modify: `reclaim-mcp/Cargo.toml` (add encode/hash deps)
- Test: inline `#[cfg(test)]` in `reclaim-mcp/src/tools/encode.rs`

**Interfaces:**
- Consumes: `crate::server::ReclaimServer`.
- Produces: `#[tool_router(router = encode_router)] impl ReclaimServer` with tools `encode`, `decode`, `hash`; pure fns `encode`, `decode`, `hash`, and `DecodeOut { text: Option<String>, hex: String }`.

- [ ] **Step 1: Add dependencies to `reclaim-mcp/Cargo.toml`** (under `[dependencies]`)

```toml
base64 = "0.22"
hex = "0.4"
md-5 = "0.10"
sha1 = "0.10"
sha2 = "0.10"
crc32fast = "1"
percent-encoding = "2"
```

- [ ] **Step 2: Write the failing tests** (in `reclaim-mcp/src/tools/encode.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips() {
        let e = encode("hello", "base64").unwrap();
        assert_eq!(e, "aGVsbG8=");
        let d = decode(&e, "base64").unwrap();
        assert_eq!(d.text.as_deref(), Some("hello"));
        assert_eq!(d.hex, "68656c6c6f");
    }

    #[test]
    fn base64url_has_no_padding_and_url_alphabet() {
        let e = encode("<<???>>", "base64url").unwrap();
        assert!(!e.contains('='));
        assert!(!e.contains('+') && !e.contains('/'));
        let d = decode(&e, "base64url").unwrap();
        assert_eq!(d.text.as_deref(), Some("<<???>>"));
    }

    #[test]
    fn hex_round_trips() {
        assert_eq!(encode("AB", "hex").unwrap(), "4142");
        assert_eq!(decode("4142", "hex").unwrap().text.as_deref(), Some("AB"));
    }

    #[test]
    fn url_encodes_reserved_chars() {
        assert_eq!(encode("a b&c", "url").unwrap(), "a%20b%26c");
        assert_eq!(decode("a%20b%26c", "url").unwrap().text.as_deref(), Some("a b&c"));
    }

    #[test]
    fn decode_of_non_utf8_omits_text_but_keeps_hex() {
        // 0xFF is not valid UTF-8.
        let d = decode("/w==", "base64").unwrap();
        assert!(d.text.is_none());
        assert_eq!(d.hex, "ff");
    }

    #[test]
    fn bad_input_is_an_error() {
        assert!(decode("zzz!", "hex").is_err());
        assert!(encode("x", "rot13").is_err());
    }

    #[test]
    fn hashes_match_known_vectors() {
        assert_eq!(hash("abc", "md5").unwrap(), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(hash("abc", "sha1").unwrap(), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            hash("abc", "sha256").unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(hash("", "crc32").unwrap(), "00000000");
        assert_eq!(hash("abc", "crc32").unwrap(), "352441c2");
    }

    #[test]
    fn encode_router_lists_three_tools() {
        let names: Vec<String> = ReclaimServer::encode_router()
            .list_all()
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        for n in ["encode", "decode", "hash"] {
            assert!(names.contains(&n.to_string()), "missing {n}");
        }
    }
}
```

- [ ] **Step 3: Run tests to confirm they fail**

Run: `cargo test -p reclaim-mcp encode`
Expected: FAIL — functions not defined.

- [ ] **Step 4: Implement the pure functions** (top of `reclaim-mcp/src/tools/encode.rs`)

```rust
//! Encoding + hashing tools. Pure functions on inline strings.
//! `encode`/`decode` schemes: base64 | base64url (no padding) | hex | url.
//! `hash` algos: md5 | sha1 | sha256 | sha512 | crc32.

use crate::server::ReclaimServer;
use base64::Engine;
use percent_encoding::{percent_decode_str, utf8_percent_encode, NON_ALPHANUMERIC};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router, Json};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub fn encode(input: &str, scheme: &str) -> Result<String, String> {
    let bytes = input.as_bytes();
    Ok(match scheme {
        "base64" => base64::engine::general_purpose::STANDARD.encode(bytes),
        "base64url" => base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes),
        "hex" => hex::encode(bytes),
        "url" => utf8_percent_encode(input, NON_ALPHANUMERIC).to_string(),
        other => return Err(format!("unknown scheme: {other}")),
    })
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DecodeOut {
    /// Decoded bytes as UTF-8, omitted when the bytes are not valid UTF-8.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Decoded bytes as lowercase hex (always present).
    pub hex: String,
}

pub fn decode(input: &str, scheme: &str) -> Result<DecodeOut, String> {
    let bytes: Vec<u8> = match scheme {
        "base64" => base64::engine::general_purpose::STANDARD
            .decode(input)
            .map_err(|e| e.to_string())?,
        "base64url" => base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(input)
            .map_err(|e| e.to_string())?,
        "hex" => hex::decode(input).map_err(|e| e.to_string())?,
        "url" => percent_decode_str(input).collect(),
        other => return Err(format!("unknown scheme: {other}")),
    };
    Ok(DecodeOut {
        text: String::from_utf8(bytes.clone()).ok(),
        hex: hex::encode(&bytes),
    })
}

pub fn hash(input: &str, algo: &str) -> Result<String, String> {
    let bytes = input.as_bytes();
    Ok(match algo {
        "md5" => {
            use md5::{Digest, Md5};
            hex::encode(Md5::digest(bytes))
        }
        "sha1" => {
            use sha1::{Digest, Sha1};
            hex::encode(Sha1::digest(bytes))
        }
        "sha256" => {
            use sha2::{Digest, Sha256};
            hex::encode(Sha256::digest(bytes))
        }
        "sha512" => {
            use sha2::{Digest, Sha512};
            hex::encode(Sha512::digest(bytes))
        }
        "crc32" => {
            let mut h = crc32fast::Hasher::new();
            h.update(bytes);
            format!("{:08x}", h.finalize())
        }
        other => return Err(format!("unknown algo: {other}")),
    })
}
```

> Crate note: the MD5 crate is named `md-5` in Cargo.toml but is imported as `md5` in code (that is the crate's library name).

- [ ] **Step 5: Add the `#[tool]` wrappers + request/response structs** (same file)

```rust
#[derive(Debug, Deserialize, JsonSchema)]
pub struct EncodeReq {
    pub input: String,
    /// base64 | base64url | hex | url
    pub scheme: String,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct EncodeResp {
    pub output: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct HashReq {
    pub input: String,
    /// md5 | sha1 | sha256 | sha512 | crc32
    pub algo: String,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct HashResp {
    pub algo: String,
    pub hex: String,
}

#[tool_router(router = encode_router)]
impl ReclaimServer {
    #[tool(name = "encode", description = "Encode a string. scheme: base64 | base64url (no padding) | hex | url.")]
    pub async fn encode(&self, p: Parameters<EncodeReq>) -> Result<Json<EncodeResp>, String> {
        Ok(Json(EncodeResp { output: encode(&p.0.input, &p.0.scheme)? }))
    }

    #[tool(name = "decode", description = "Decode a string. scheme: base64 | base64url | hex | url. Returns hex always, plus text when the bytes are valid UTF-8.")]
    pub async fn decode(&self, p: Parameters<EncodeReq>) -> Result<Json<DecodeOut>, String> {
        Ok(Json(decode(&p.0.input, &p.0.scheme)?))
    }

    #[tool(name = "hash", description = "Hash a string. algo: md5 | sha1 | sha256 | sha512 | crc32. Returns lowercase hex.")]
    pub async fn hash(&self, p: Parameters<HashReq>) -> Result<Json<HashResp>, String> {
        let hex = hash(&p.0.input, &p.0.algo)?;
        Ok(Json(HashResp { algo: p.0.algo, hex }))
    }
}
```

- [ ] **Step 6: Register the module and router**

In `reclaim-mcp/src/tools/mod.rs` add `pub mod encode;`.
In `reclaim-mcp/src/server.rs` `new()`:

```rust
            tool_router: Self::reclaim_router() + Self::json_router() + Self::encode_router(),
```

- [ ] **Step 7: Run tests + clippy**

Run: `cargo test -p reclaim-mcp encode`
Expected: PASS.
Run: `cargo clippy -p reclaim-mcp --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add reclaim-mcp/Cargo.toml reclaim-mcp/src/tools/encode.rs reclaim-mcp/src/tools/mod.rs reclaim-mcp/src/server.rs Cargo.lock
git commit -m "feat(mcp): add encode/decode/hash tools (base64/hex/url, md5/sha/crc32)"
```

---

### Task 4: Time tools (`time_convert`, `time_now`, `time_diff`)

**Files:**
- Create: `reclaim-mcp/src/tools/timeconv.rs`
- Modify: `reclaim-mcp/src/tools/mod.rs` (add `pub mod timeconv;`)
- Modify: `reclaim-mcp/src/server.rs` (`new()`: `+ Self::time_router()`)
- Modify: `reclaim-mcp/Cargo.toml` (add chrono deps)
- Test: inline `#[cfg(test)]` in `reclaim-mcp/src/tools/timeconv.rs`

**Interfaces:**
- Consumes: `crate::server::ReclaimServer`.
- Produces: `#[tool_router(router = time_router)] impl ReclaimServer` with tools `time_convert`, `time_now`, `time_diff`; pure fns `time_convert`, `time_now`, `time_diff`; types `TimeOut { rfc3339, epoch_ms, formatted, tz }`, `DiffOut { seconds, human }`.

- [ ] **Step 1: Add dependencies to `reclaim-mcp/Cargo.toml`** (under `[dependencies]`)

```toml
chrono = { version = "0.4", default-features = false, features = ["clock", "std"] }
chrono-tz = "0.10"
```

- [ ] **Step 2: Write the failing tests** (in `reclaim-mcp/src/tools/timeconv.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_seconds_to_utc_rfc3339() {
        let out = time_convert("0", "epoch_s", "UTC", None).unwrap();
        assert_eq!(out.rfc3339, "1970-01-01T00:00:00+00:00");
        assert_eq!(out.epoch_ms, 0);
        assert_eq!(out.tz, "UTC");
    }

    #[test]
    fn epoch_ms_to_named_zone() {
        // 1_000_000_000_000 ms = 2001-09-09T01:46:40Z
        let out = time_convert("1000000000000", "epoch_ms", "America/New_York", None).unwrap();
        assert!(out.rfc3339.starts_with("2001-09-08T21:46:40"));
        assert_eq!(out.epoch_ms, 1_000_000_000_000);
    }

    #[test]
    fn rfc3339_to_epoch_and_custom_format() {
        let out = time_convert(
            "2001-09-09T01:46:40+00:00",
            "rfc3339",
            "UTC",
            Some("%Y/%m/%d %H:%M"),
        )
        .unwrap();
        assert_eq!(out.epoch_ms, 1_000_000_000_000);
        assert_eq!(out.formatted, "2001/09/09 01:46");
    }

    #[test]
    fn unknown_timezone_is_an_error() {
        assert!(time_convert("0", "epoch_s", "Mars/Olympus", None).is_err());
    }

    #[test]
    fn bad_value_is_an_error() {
        assert!(time_convert("not-a-number", "epoch_s", "UTC", None).is_err());
        assert!(time_convert("nonsense", "rfc3339", "UTC", None).is_err());
    }

    #[test]
    fn now_returns_a_valid_structure() {
        let out = time_now("UTC", None).unwrap();
        assert_eq!(out.tz, "UTC");
        assert!(out.epoch_ms > 1_700_000_000_000); // after 2023-11
    }

    #[test]
    fn diff_is_b_minus_a_and_human_readable() {
        let d = time_diff("0", "90061").unwrap(); // 1d 1h 1m 1s
        assert_eq!(d.seconds, 90061);
        assert_eq!(d.human, "1d 1h 1m 1s");
        let neg = time_diff("90061", "0").unwrap();
        assert_eq!(neg.seconds, -90061);
        assert_eq!(neg.human, "-1d 1h 1m 1s");
    }

    #[test]
    fn time_router_lists_three_tools() {
        let names: Vec<String> = ReclaimServer::time_router()
            .list_all()
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        for n in ["time_convert", "time_now", "time_diff"] {
            assert!(names.contains(&n.to_string()), "missing {n}");
        }
    }
}
```

- [ ] **Step 3: Run tests to confirm they fail**

Run: `cargo test -p reclaim-mcp time`
Expected: FAIL — functions not defined.

- [ ] **Step 4: Implement the pure functions** (top of `reclaim-mcp/src/tools/timeconv.rs`)

```rust
//! Time tools: epoch <-> RFC3339 in any IANA zone, "now", and diffs.
//! Pure except `time_now`, which reads the system clock (no network).

use crate::server::ReclaimServer;
use chrono::{DateTime, TimeZone, Utc};
use chrono_tz::Tz;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router, Json};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, JsonSchema)]
pub struct TimeOut {
    /// RFC3339 timestamp rendered in the requested timezone.
    pub rfc3339: String,
    pub epoch_ms: i64,
    /// `format`-rendered string when a pattern was given, else equals rfc3339.
    pub formatted: String,
    pub tz: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DiffOut {
    pub seconds: i64,
    pub human: String,
}

fn parse_tz(name: &str) -> Result<Tz, String> {
    name.parse::<Tz>().map_err(|_| format!("unknown timezone: {name}"))
}

/// Parse a `value` of kind `from` into a UTC instant.
fn parse_instant(value: &str, from: &str) -> Result<DateTime<Utc>, String> {
    match from {
        "epoch_s" => {
            let s: i64 = value.parse().map_err(|_| format!("not an integer: {value}"))?;
            Utc.timestamp_opt(s, 0)
                .single()
                .ok_or_else(|| format!("epoch seconds out of range: {value}"))
        }
        "epoch_ms" => {
            let ms: i64 = value.parse().map_err(|_| format!("not an integer: {value}"))?;
            Utc.timestamp_millis_opt(ms)
                .single()
                .ok_or_else(|| format!("epoch millis out of range: {value}"))
        }
        "epoch_us" => {
            let us: i64 = value.parse().map_err(|_| format!("not an integer: {value}"))?;
            Utc.timestamp_micros(us)
                .single()
                .ok_or_else(|| format!("epoch micros out of range: {value}"))
        }
        "rfc3339" => DateTime::parse_from_rfc3339(value)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| format!("invalid rfc3339: {e}")),
        other => Err(format!("unknown `from`: {other}")),
    }
}

fn render(dt_utc: DateTime<Utc>, tz: Tz, tz_name: &str, format: Option<&str>) -> TimeOut {
    let local = dt_utc.with_timezone(&tz);
    let rfc3339 = local.to_rfc3339();
    let formatted = match format {
        Some(f) => local.format(f).to_string(),
        None => rfc3339.clone(),
    };
    TimeOut {
        rfc3339,
        epoch_ms: dt_utc.timestamp_millis(),
        formatted,
        tz: tz_name.to_string(),
    }
}

pub fn time_convert(
    value: &str,
    from: &str,
    to_tz: &str,
    format: Option<&str>,
) -> Result<TimeOut, String> {
    let dt = parse_instant(value, from)?;
    let tz = parse_tz(to_tz)?;
    Ok(render(dt, tz, to_tz, format))
}

pub fn time_now(tz_name: &str, format: Option<&str>) -> Result<TimeOut, String> {
    let tz = parse_tz(tz_name)?;
    Ok(render(Utc::now(), tz, tz_name, format))
}

/// Auto-detect each side: a bare integer is epoch seconds; otherwise RFC3339.
fn parse_either(s: &str) -> Result<DateTime<Utc>, String> {
    if s.trim().parse::<i64>().is_ok() {
        parse_instant(s.trim(), "epoch_s")
    } else {
        parse_instant(s, "rfc3339")
    }
}

fn humanize(total: i64) -> String {
    let sign = if total < 0 { "-" } else { "" };
    let mut s = total.abs();
    let d = s / 86_400;
    s %= 86_400;
    let h = s / 3_600;
    s %= 3_600;
    let m = s / 60;
    let sec = s % 60;
    format!("{sign}{d}d {h}h {m}m {sec}s")
}

pub fn time_diff(a: &str, b: &str) -> Result<DiffOut, String> {
    let ta = parse_either(a)?;
    let tb = parse_either(b)?;
    let seconds = (tb - ta).num_seconds();
    Ok(DiffOut { seconds, human: humanize(seconds) })
}
```

- [ ] **Step 5: Add the `#[tool]` wrappers + request structs** (same file)

```rust
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ConvertReq {
    pub value: String,
    /// epoch_s | epoch_ms | epoch_us | rfc3339
    pub from: String,
    /// IANA timezone name, e.g. "America/New_York".
    pub to_tz: String,
    /// Optional strftime pattern.
    pub format: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NowReq {
    pub tz: String,
    pub format: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DiffReq {
    /// epoch seconds or RFC3339.
    pub a: String,
    pub b: String,
}

#[tool_router(router = time_router)]
impl ReclaimServer {
    #[tool(name = "time_convert", description = "Convert a timestamp (epoch_s/epoch_ms/epoch_us/rfc3339) into RFC3339 in any IANA timezone, with optional strftime formatting.")]
    pub async fn time_convert(&self, p: Parameters<ConvertReq>) -> Result<Json<TimeOut>, String> {
        Ok(Json(time_convert(&p.0.value, &p.0.from, &p.0.to_tz, p.0.format.as_deref())?))
    }

    #[tool(name = "time_now", description = "Current time in the given IANA timezone (reads system clock; no network).")]
    pub async fn time_now(&self, p: Parameters<NowReq>) -> Result<Json<TimeOut>, String> {
        Ok(Json(time_now(&p.0.tz, p.0.format.as_deref())?))
    }

    #[tool(name = "time_diff", description = "Difference b - a between two timestamps (epoch seconds or RFC3339). Returns total seconds and a human string.")]
    pub async fn time_diff(&self, p: Parameters<DiffReq>) -> Result<Json<DiffOut>, String> {
        Ok(Json(time_diff(&p.0.a, &p.0.b)?))
    }
}
```

- [ ] **Step 6: Register the module and router**

In `reclaim-mcp/src/tools/mod.rs` add `pub mod timeconv;`.
In `reclaim-mcp/src/server.rs` `new()`:

```rust
            tool_router: Self::reclaim_router()
                + Self::json_router()
                + Self::encode_router()
                + Self::time_router(),
```

- [ ] **Step 7: Run tests + clippy**

Run: `cargo test -p reclaim-mcp time`
Expected: PASS.
Run: `cargo clippy -p reclaim-mcp --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add reclaim-mcp/Cargo.toml reclaim-mcp/src/tools/timeconv.rs reclaim-mcp/src/tools/mod.rs reclaim-mcp/src/server.rs Cargo.lock
git commit -m "feat(mcp): add time tools (epoch/tz convert, now, diff)"
```

---

### Task 5: Docs + full-server verification

Final task: update the MCP docs to describe the new tool families and verify the whole server end-to-end over the stdio protocol.

**Files:**
- Modify: `docs/mcp.md`
- Modify: `README.md` (tool list, if it enumerates MCP tools — check first)

**Interfaces:**
- Consumes: all routers from Tasks 1–4.
- Produces: documentation only.

- [ ] **Step 1: Add a full-router aggregate test** (append to `reclaim-mcp/src/server.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_exposes_all_thirteen_tools() {
        let s = ReclaimServer::new();
        let names: std::collections::BTreeSet<String> = s
            .tool_router
            .list_all()
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        for n in [
            "scan_disk", "propose_reclaim", "reclaim_space",
            "json_prettify", "json_minify", "json_compare", "json_validate",
            "encode", "decode", "hash",
            "time_convert", "time_now", "time_diff",
        ] {
            assert!(names.contains(n), "missing tool: {n}");
        }
        assert_eq!(names.len(), 13);
    }
}
```

- [ ] **Step 2: Run the full test suite + clippy**

Run: `cargo test -p reclaim-mcp && cargo test -p reclaim-core`
Expected: PASS (all tests, both crates).
Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 3: End-to-end stdio smoke test of a new tool**

```bash
printf '%s\n%s\n%s\n' \
 '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
 '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
 '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"hash","arguments":{"input":"abc","algo":"sha256"}}}' \
 | cargo run -q -p reclaim-mcp 2>/dev/null
```
Expected: the `tools/call` response's `structuredContent.hex` equals `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`.

- [ ] **Step 4: Update `docs/mcp.md`**

Read the existing `docs/mcp.md` first to match its style, then add a section listing the new tool families:
- **JSON:** `json_prettify`, `json_minify`, `json_compare`, `json_validate`
- **Encode/hash:** `encode`, `decode`, `hash`
- **Time:** `time_convert`, `time_now`, `time_diff`

Note for each: pure functions, inline-string input, no filesystem/network. Mention the migration to the `rmcp` SDK and that the disk tools' behavior and safety gate are unchanged.

- [ ] **Step 5: Update `README.md` if it enumerates MCP tools**

Run: `grep -n "scan_disk" README.md` — if the tool list appears there, add the new families in the same format. If not, skip.

- [ ] **Step 6: Commit**

```bash
git add docs/mcp.md README.md reclaim-mcp/src/server.rs
git commit -m "docs(mcp): document JSON/encode/time tools; add full-server tool test"
```

---

## Self-Review

**Spec coverage:**
- rmcp migration → Task 1. ✓
- JSON prettify/minify/compare/validate → Task 2. ✓
- encode/decode/hash (base64/base64url/hex/url; md5/sha1/sha256/sha512/crc32) → Task 3. ✓
- time_convert/time_now/time_diff (IANA via chrono-tz) → Task 4. ✓
- Inline-string-only, no fs/network → enforced by design across Tasks 2–4. ✓
- Safety gate preserved → gate test ported verbatim in Task 1; disk tools delegate to core. ✓
- Tool-error-not-panic → every pure fn returns `Result<_, String>`; wrappers propagate. ✓
- Low tool count via enums → `encode`/`decode`/`hash` use `scheme`/`algo`. ✓
- Roadmap (Docker/API runner/webpage perf) → out of scope for this slice, per spec. ✓

**Placeholder scan:** No TBD/TODO; all code shown in full. The one conditional ("update README if it lists tools") is gated by an explicit `grep` check.

**Type consistency:** Router fn names (`reclaim_router`, `json_router`, `encode_router`, `time_router`) are referenced consistently in `new()` and per-module tests. Response types (`PrettifyResp`, `MinifyResp`, `ValidateOut`, `CompareOut`, `DecodeOut`, `HashResp`, `TimeOut`, `DiffOut`) are each defined once and used by their wrapper. Pure-fn signatures match their call sites in the wrappers and tests.

**Known API-validation point:** `ProtocolVersion::V_2025_06_18` and the exact `ServerInfo` field set are validated against the compiler in Task 1, Step 7 (a fallback is noted in Step 2). If rmcp's `Json<T>` requires a trait the response structs lack, the compiler error names it — all response structs already derive `Serialize + JsonSchema`.
