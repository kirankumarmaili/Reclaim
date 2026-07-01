//! The `rmcp` server: one `ReclaimServer` owning a combined ToolRouter.
//!
//! Per-feature `#[tool_router(router = …)]` impl blocks live in sibling files
//! (`reclaim_tools.rs`, `tools/*.rs`); `new()` sums their routers. The server is
//! a thin client over `reclaim-core` for disk tools and holds only pure logic
//! for the utility tools — no network, and (for utilities) no filesystem.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{Implementation, ProtocolVersion, ServerCapabilities, ServerInfo};
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
            // Later tasks extend this sum: `+ Self::encode_router()` etc.
            tool_router: Self::reclaim_router()
                + Self::json_router()
                + Self::encode_router()
                + Self::time_router(),
        }
    }
}

// NOTE: InitializeResult (= ServerInfo) is #[non_exhaustive], so struct-literal
// construction is forbidden outside the rmcp crate. We use the builder methods
// (new + with_*) instead of the struct literal shown in the task brief.
#[tool_handler(router = self.tool_router)]
impl ServerHandler for ReclaimServer {
    fn get_info(&self) -> ServerInfo {
        // Use Implementation::new with env!() expanded here (in reclaim-mcp's crate
        // context) so serverInfo.name is "reclaim-mcp", not the rmcp library name.
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::V_2025_06_18)
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "Reclaim exposes a local, risk-aware disk-recovery engine plus \
                 offline developer utilities. Disk tools: scan_disk, \
                 propose_reclaim, reclaim_space — deletion is confined to $HOME, \
                 defaults to move-to-Trash, and Protected items can never be \
                 deleted (the core re-validates every target). Utility tools \
                 (json_*, encode/decode/hash, time_*) are pure and operate on \
                 inline strings only.",
            )
    }
}

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
