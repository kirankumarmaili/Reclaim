#!/usr/bin/env bash
# Run the MCP server (JSON-RPC over stdio). Real use is via an MCP client
# (see docs/mcp.md); this is mainly for a quick sanity check.
# With --smoke, sends initialize + tools/list and prints the responses.
source "$(dirname "${BASH_SOURCE[0]}")/_common.sh"

need cargo
if [[ "${1:-}" == "--smoke" ]]; then
  log "smoke: initialize → initialized → tools/list"
  # rmcp 2.x needs a real handshake (protocolVersion/capabilities/clientInfo)
  # plus the initialized notification before it will answer tools/list.
  printf '%s\n%s\n%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"reclaim-smoke","version":"0.0.0"}}}' \
    '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
    | cargo run -q -p reclaim-mcp
  exit 0
fi

log "reclaim-mcp server on stdio (Ctrl-D / Ctrl-C to stop)"
exec cargo run -q -p reclaim-mcp
