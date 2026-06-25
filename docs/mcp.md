# Reclaim MCP server

`reclaim-mcp` exposes the Reclaim engine to agents as MCP tools, under the
**identical safety guarantees** as the desktop UI. It is a thin client over
`reclaim-core` — it holds no delete logic and adds no bypass. Every reclaim flows
through the core safety gate, which re-scans the live filesystem and re-validates
each target's risk class and HOME boundary. An agent can never widen what is
deletable.

- **Transport:** newline-delimited JSON-RPC 2.0 over stdio (the MCP stdio
  transport). Logs go to stderr; stdout carries only protocol messages.
- **Network:** none, beyond the stdio pipe itself. The tool stays local and silent.

## Tools

| Tool | Input | Returns |
|---|---|---|
| `scan_disk` | `{ root? }` | `ScanResult` (the [§10 schema](../Prd.md)) — every reclaimable item classified `safe` / `review` / `protected`. Read-only. |
| `propose_reclaim` | `{ root?, policy? }` | A staged-autonomy `Proposal`. Candidates are **Safe-class only**; never proposes permanent deletion. |
| `reclaim_space` | `{ root?, ids[], mode? }` | `ReclaimResult`, via the core safety gate. Defaults to move-to-Trash. |

`root` defaults to `~/Library`. `mode` is `trash` (default, reversible) or
`permanent` (irreversible, opt-in — never produced by `propose_reclaim`).

### Staged autonomy (`policy`)

Mirrors the Brhaspati rollout ladder. Only `auto_safe` may execute its own
proposal, and even then only Safe items, only to Trash:

| Policy | Proposes | `auto_executable` |
|---|---|---|
| `shadow` (default) | Safe items (report only) | `false` |
| `assisted` | Safe items (human approves) | `false` |
| `auto_safe` | Safe items | `true` |

Review and Protected items are **never** proposed for automation at any stage, and
permanent deletion is **never** automated.

## Build

```bash
cargo build -p reclaim-mcp --release
# binary at: target/release/reclaim-mcp
```

## Register with an MCP client

### Claude Code (project `.mcp.json`)

```json
{
  "mcpServers": {
    "reclaim": {
      "command": "./target/release/reclaim-mcp"
    }
  }
}
```

### Claude Desktop (`claude_desktop_config.json`)

```json
{
  "mcpServers": {
    "reclaim": {
      "command": "/absolute/path/to/Reclaim/target/release/reclaim-mcp"
    }
  }
}
```

## Quick smoke test

```bash
printf '%s\n%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
  | cargo run -q -p reclaim-mcp
```

You should get two JSON-RPC responses: the `initialize` result (server info +
`tools` capability) and the three tool schemas.
