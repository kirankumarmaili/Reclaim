//! The three disk tools, ported to rmcp. Each delegates to `reclaim-core`;
//! `reclaim_space` goes through the core safety gate exactly as before.
//!
//! Output shape: these tools return their `reclaim-core` result serialized to a
//! JSON string (delivered as the tool result's `content[0].text`), NOT as
//! `structuredContent`. rmcp validates each tool's output schema at registration
//! and panics for an "any value" schema, which is what `Json<serde_json::Value>`
//! would produce — and giving the core types a real schema would require adding
//! `JsonSchema` to `reclaim-core`, which is out of scope here. Consumers parse the
//! text as JSON. (The utility tools under `tools/` return typed `structuredContent`
//! because their result types are local and derive `JsonSchema`.)

use crate::policy::{self, Policy};
use crate::server::ReclaimServer;
use reclaim_core::{reclaim, safety, scan, Mode, ReclaimTarget};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router};
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

#[tool_router(router = reclaim_router, vis = "pub(crate)")]
impl ReclaimServer {
    #[tool(
        name = "scan_disk",
        description = "Scan a directory tree (under $HOME) and classify every reclaimable item by delete-risk: safe (regenerates, no loss), review (reclaimable with a cost), or protected (real data — never deletable). Read-only."
    )]
    pub async fn scan_disk(&self, params: Parameters<ScanArgs>) -> Result<String, String> {
        let root = root_of(params.0.root);
        let result = scan::scan(&root)
            .map_err(|e| format!("scan failed for {}: {e}", root.display()))?;
        serde_json::to_string(&result).map_err(|e| e.to_string())
    }

    #[tool(
        name = "propose_reclaim",
        description = "Produce a staged-autonomy reclaim plan from a fresh scan. Candidates are Safe-class items ONLY; Review and Protected are never proposed. Policies: shadow (report only), assisted (human approves), auto_safe (may self-execute Safe items to Trash). Never proposes permanent deletion."
    )]
    pub async fn propose_reclaim(
        &self,
        params: Parameters<ProposeArgs>,
    ) -> Result<String, String> {
        let root = root_of(params.0.root);
        let policy: Policy = params
            .0
            .policy
            .map(|p| serde_json::from_value(Value::String(p)).unwrap_or_default())
            .unwrap_or_default();
        let scanned = scan::scan(&root)
            .map_err(|e| format!("scan failed for {}: {e}", root.display()))?;
        let proposal = policy::propose(&scanned, policy);
        serde_json::to_string(&proposal).map_err(|e| e.to_string())
    }

    #[tool(
        name = "reclaim_space",
        description = "Reclaim the given item IDs through the SAME core safety gate as the UI: the core re-scans, re-derives each item's risk class and path, and refuses anything Protected or outside $HOME. Defaults to move-to-Trash (reversible). `permanent` is opt-in."
    )]
    pub async fn reclaim_space(
        &self,
        params: Parameters<ReclaimArgs>,
    ) -> Result<String, String> {
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
        serde_json::to_string(&result).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use crate::server::ReclaimServer;
    use reclaim_core::reclaim::{reclaim_with, Executor};
    use reclaim_core::{Item, Mode, ReclaimTarget, Risk};
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    #[tokio::test]
    async fn reclaim_space_with_empty_ids_is_a_tool_error() {
        use crate::reclaim_tools::ReclaimArgs;
        use rmcp::handler::server::wrapper::Parameters;
        let server = ReclaimServer::new();
        let res = server
            .reclaim_space(Parameters(ReclaimArgs {
                root: Some("/tmp".into()),
                ids: vec![],
                mode: None,
            }))
            .await;
        assert!(res.is_err(), "empty ids must be a tool error");
    }

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
