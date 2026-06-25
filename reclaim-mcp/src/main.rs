//! `reclaim-mcp` — an MCP server exposing the Reclaim engine to agents.
//!
//! Transport: newline-delimited JSON-RPC 2.0 on stdin/stdout (the MCP stdio
//! transport). One message per line in, one response per line out; logs go to
//! stderr so they never corrupt the protocol stream.
//!
//! This is a thin client over `reclaim-core`. Every delete flows through the
//! core safety gate — the MCP layer adds no bypass and holds no delete logic, so
//! an agent operates under the identical guarantees as the desktop UI (PRD §9).
//! No network is opened beyond the stdio transport itself; the tool stays local
//! and silent.

mod policy;
mod protocol;
mod server;

use std::io::{self, BufRead, Write};

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    eprintln!("reclaim-mcp {} ready on stdio", env!("CARGO_PKG_VERSION"));

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("reclaim-mcp: stdin read error: {e}");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }

        if let Some(response) = server::handle_line(&line) {
            match serde_json::to_string(&response) {
                Ok(json) => {
                    if writeln!(out, "{json}").and_then(|_| out.flush()).is_err() {
                        break; // stdout closed — client went away.
                    }
                }
                Err(e) => eprintln!("reclaim-mcp: failed to serialize response: {e}"),
            }
        }
    }
}
