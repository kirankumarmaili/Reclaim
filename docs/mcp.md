# Reclaim MCP server

`reclaim-mcp` exposes the Reclaim engine to agents as MCP tools, under the
**identical safety guarantees** as the desktop UI. It is a thin client over
`reclaim-core` — it holds no delete logic and adds no bypass. Every reclaim flows
through the core safety gate, which re-scans the live filesystem and re-validates
each target's risk class and HOME boundary. An agent can never widen what is
deletable.

- **SDK:** built on the official Rust MCP SDK ([`rmcp`](https://crates.io/crates/rmcp)).
  Tools are declared with the `#[tool]` / `#[tool_router]` macros; input/output
  schemas are generated from Rust types.
- **Transport:** JSON-RPC 2.0 over stdio (the MCP stdio transport). Logs go to
  stderr; stdout carries only protocol messages.
- **Network:** none, beyond the stdio pipe itself. The tool stays local and silent.

Beyond the disk engine, `reclaim-mcp` also exposes a set of **offline developer
utilities** (JSON, encoding/hashing, time). These are pure functions over
**inline strings only** — no filesystem, no network — so they share the server's
local-and-silent guarantee without touching Reclaim's file-safety surface.

## Tools

| Tool | Input | Returns |
|---|---|---|
| `scan_disk` | `{ root? }` | `ScanResult` (the [§10 schema](../Prd.md)) — every reclaimable item classified `safe` / `review` / `protected`. Read-only. |
| `propose_reclaim` | `{ root?, policy? }` | A staged-autonomy `Proposal`. Candidates are **Safe-class only**; never proposes permanent deletion. |
| `reclaim_space` | `{ root?, ids[], mode? }` | `ReclaimResult`, via the core safety gate. Defaults to move-to-Trash. |

`root` defaults to `~/Library`. `mode` is `trash` (default, reversible) or
`permanent` (irreversible, opt-in — never produced by `propose_reclaim`).

> **Output shape:** the disk tools return their JSON (the §10 `ScanResult` /
> `Proposal` / `ReclaimResult`) as the tool result's **`content[0].text`**, not as
> `structuredContent`. This is because rmcp cannot generate an output schema for
> `serde_json::Value` (its schema is "any value") without adding `JsonSchema` to
> the `reclaim-core` types, which is out of scope. Consumers should parse the text
> as JSON. (The utility tools below, whose result types are local, do return
> `structuredContent`.)

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

## Developer utility tools

Pure, stateless tools that take their input as inline string arguments. Utility
tools return their result as MCP `structuredContent` (schemas generated from the
Rust return types). Any bad input (unparseable JSON, malformed base64/hex,
unknown timezone) is returned as a tool error, never a panic.

### JSON

| Tool | Input | Returns |
|---|---|---|
| `json_prettify` | `{ json, indent? }` | `{ pretty }` — `indent` is `"2"` (default), `"4"`, or `"tab"`. |
| `json_minify` | `{ json }` | `{ minified }` |
| `json_validate` | `{ json }` | `{ valid, error? { message, line, column } }` |
| `json_compare` | `{ left, right }` | `{ equal, diffs[] }` — semantic diff: object keys order-insensitive, arrays index-sensitive. Each diff is `{ path, kind: added\|removed\|changed\|type_changed, left?, right? }` with JSON-Pointer paths. |

### Encoding / hashing

| Tool | Input | Returns |
|---|---|---|
| `encode` | `{ input, scheme }` | `{ output }` — `scheme`: `base64` \| `base64url` (no padding) \| `hex` \| `url`. |
| `decode` | `{ input, scheme }` | `{ text?, hex }` — `hex` always; `text` when the decoded bytes are valid UTF-8. |
| `hash` | `{ input, algo }` | `{ algo, hex }` — `algo`: `md5` \| `sha1` \| `sha256` \| `sha512` \| `crc32`. |

### Time

| Tool | Input | Returns |
|---|---|---|
| `time_convert` | `{ value, from, to_tz, format? }` | `{ rfc3339, epoch_ms, formatted, tz }` — `from`: `epoch_s` \| `epoch_ms` \| `epoch_us` \| `rfc3339`; `to_tz` is any IANA name; `format` is an optional strftime pattern. |
| `time_now` | `{ tz, format? }` | Same shape (reads system clock; no network). |
| `time_diff` | `{ a, b }` | `{ seconds, human }` — `b - a`; each side is epoch seconds or RFC3339. |

## Interactive testing

```bash
./run inspect            # opens the MCP Inspector with the reclaim server preloaded
./run inspect --check    # headless: verifies tool list + a pure call + the gate over real stdio
```

Pick a tool, fill the schema-generated form, send it, and read the response and
the raw JSON-RPC — like an API client. Copy-paste requests are in
[`mcp-examples.md`](mcp-examples.md). Note `reclaim_space` is a real call: it
still defaults to Trash and still goes through the safety gate.

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

rmcp 2.x requires a real handshake (`initialize` with `protocolVersion`,
`capabilities`, `clientInfo`, then the `initialized` notification) before it
answers `tools/list`:

```bash
./run mcp --smoke
```

This sends `initialize` → `initialized` → `tools/list` and prints the responses:
the server info (`tools` capability) and the full tool list — the three disk tools
plus the JSON, encoding/hashing, and time utility tools.
