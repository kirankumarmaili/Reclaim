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
