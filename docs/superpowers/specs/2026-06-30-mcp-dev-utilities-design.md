# MCP Dev Utilities — Design

**Date:** 2026-06-30
**Status:** Approved (design); implementation pending
**Branch:** `feat/mcp-server`

## Summary

Extend Reclaim with a suite of **MCP-first developer utilities**, bolted onto the
existing `reclaim-mcp` server. The work begins by migrating that server from its
hand-rolled JSON-RPC loop to the official Rust MCP SDK (`rmcp`), then adds the
first slice of utility tools (JSON, encoding/hashing, time). Docker, an API
runner, and webpage-performance analysis follow in later slices, each with its
own spec.

The disk-reclaim engine and its safety gate are **untouched** — the existing three
tools keep identical behavior. The new tools are pure, stateless, and operate on
inline strings only: zero filesystem access, zero network. This preserves
Reclaim's "local and silent" product invariant (PRD §5/§11).

## Decisions (settled during brainstorming)

| Decision | Choice | Rationale |
|---|---|---|
| Where utilities live | Bolt onto the existing `reclaim-mcp` server | User's call; mitigated by per-feature modules + tool namespacing |
| Stack | Migrate to official `rmcp` SDK | Less boilerplate as tools multiply; generated schemas |
| First slice | rmcp migration + offline utils (JSON, encode/hash, time) | Lowest-risk tools prove the SDK end-to-end before Docker/HTTP |
| Input model | Inline strings only | Keeps utilities cleanly separate from Reclaim's file-safety concerns; no `$HOME` boundary surface |
| #4 + #6 | Merged into one "Bruno-style API runner" slice | They are the same idea stated twice |
| #7 (webpage perf) | Rust network-level only; recommend chrome-devtools MCP for render metrics | Don't rebuild a browser/Lighthouse stack |

## Architecture

```
reclaim-mcp/src/
  main.rs            # rmcp stdio server wiring (replaces the manual read loop)
  reclaim_tools.rs   # scan_disk / propose_reclaim / reclaim_space (moved from server.rs)
  policy.rs          # unchanged (staged-autonomy proposal logic)
  tools/
    json.rs          # JSON tools
    encode.rs        # encode / decode / hash
    timeconv.rs      # time tools
```

- `protocol.rs` is **deleted** — `rmcp` owns JSON-RPC framing and the stdio transport.
- `server.rs`'s dispatch logic is replaced by `rmcp`'s tool router; the three
  reclaim tool handlers move to `reclaim_tools.rs` and keep delegating to
  `reclaim_core` exactly as today.
- Each feature is an independent module under `tools/`. Adding a feature must not
  require touching another feature's module — mirrors the detector catalogue's
  additive design.
- The server stays a **thin client over `reclaim-core`** for anything disk-related.
  The new tools hold their own (pure) logic but open no files and no sockets.

### Why these boundaries

- `json.rs`, `encode.rs`, `timeconv.rs` are each a set of pure functions with a
  thin rmcp tool wrapper. Each can be understood and tested without reading the
  others or the reclaim core.
- `reclaim_tools.rs` is the only module that depends on `reclaim_core`; the
  utilities depend on neither it nor each other.

## Tool catalogue — first slice (10 tools)

Encoders use a `scheme`/`algo` enum rather than one tool per algorithm, to keep
the server's tool count (and therefore agent context cost) low.

### JSON (`json.rs`)

| Tool | Input | Output |
|---|---|---|
| `json_prettify` | `json: string`, `indent: "2" \| "4" \| "tab"` (default `"2"`) | `{ pretty: string }` |
| `json_minify` | `json: string` | `{ minified: string }` |
| `json_compare` | `left: string`, `right: string` | `{ equal: bool, diffs: Diff[] }` |
| `json_validate` | `json: string` | `{ valid: bool, error?: { message, line, column } }` |

`Diff = { path: string, kind: "added" | "removed" | "changed" | "type_changed", left?: Value, right?: Value }`

Compare semantics: **object keys are order-insensitive; array elements are
order-sensitive** (compared by index). `path` is a JSON-Pointer-style string
(e.g. `/users/0/name`). `kind`:
- `added` — present in `right`, absent in `left`
- `removed` — present in `left`, absent in `right`
- `changed` — present in both, same JSON type, different value
- `type_changed` — present in both, different JSON type

Invalid JSON in any input is a tool error with the parse message.

### Encode / hash (`encode.rs`)

| Tool | Input | Output |
|---|---|---|
| `encode` | `input: string`, `scheme: "base64" \| "base64url" \| "hex" \| "url"` | `{ output: string }` |
| `decode` | `input: string`, `scheme: "base64" \| "base64url" \| "hex" \| "url"` | `{ text?: string, hex: string }` |
| `hash` | `input: string`, `algo: "md5" \| "sha1" \| "sha256" \| "sha512" \| "crc32"` | `{ algo: string, hex: string }` |

- `encode` treats `input` as UTF-8 bytes. `url` scheme percent-encodes.
- `decode` returns the decoded bytes as lowercase `hex` always, plus `text` when
  the bytes are valid UTF-8 (omitted otherwise). Malformed input (bad base64/hex)
  is a tool error.
- `hash` hashes the UTF-8 bytes of `input`; `crc32` is the IEEE polynomial,
  rendered as 8 lowercase hex digits.

### Time (`timeconv.rs`)

| Tool | Input | Output |
|---|---|---|
| `time_convert` | `value: string`, `from: "epoch_s" \| "epoch_ms" \| "epoch_us" \| "rfc3339"`, `to_tz: string` (IANA), `format?: string` (strftime) | `{ rfc3339, epoch_ms, formatted, tz }` |
| `time_now` | `tz: string` (IANA), `format?: string` | `{ rfc3339, epoch_ms, formatted, tz }` |
| `time_diff` | `a: string`, `b: string` (each epoch or rfc3339, auto-detected) | `{ seconds: i64, human: string }` |

- IANA timezones resolved via `chrono-tz`; an unknown zone is a tool error.
- `formatted` uses `format` when supplied, else equals `rfc3339`.
- `time_now` reads the system clock only — still no network.
- `time_diff` result is `b - a`; `human` is e.g. `"2d 3h 4m 5s"` (sign-aware).

## Dependencies (added to `reclaim-mcp/Cargo.toml`)

`rmcp`, `base64`, `hex`, `md-5`, `sha1`, `sha2`, `crc32fast`, `percent-encoding`,
`chrono`, `chrono-tz`. (`serde`, `serde_json`, `tempfile` already present.)

## Error handling

- Every tool maps bad input (unparseable JSON, malformed base64/hex, unknown
  timezone, out-of-range epoch) to a structured MCP **tool error** (`isError`),
  never a panic and never a protocol-level error.
- Tool errors carry a human-readable message; success carries both a short text
  summary and the typed `structuredContent`, matching the existing tools' shape.

## Testing

- **Per-tool unit tests** — pure functions, exhaustive on edge cases: empty
  input, invalid input, round-trips (`encode`→`decode`, epoch→rfc3339→epoch),
  unicode, large numbers, leap seconds / DST boundaries for time.
- **Migration parity tests** (must hold after the rmcp move):
  - `tools/list` still exposes `scan_disk`, `propose_reclaim`, `reclaim_space`
    alongside the new tools.
  - `initialize` still advertises the tools capability and server name.
  - The existing **"agent request for a Protected ID is rejected by the gate"**
    test is preserved verbatim — the safety guarantee is unchanged.
- TDD (red-green-refactor) for every new tool.

## Non-goals (first slice)

- No file-path inputs (inline strings only).
- No Docker, HTTP, or browser code.
- No changes to `reclaim-core`, the safety gate, detectors, or the UI.
- No new network calls of any kind.

## Roadmap (later slices — separate specs)

| Slice | Feature | Notes |
|---|---|---|
| 2 | **Docker explorer** (#2) | Read-only over the docker socket via `bollard`: list containers/images/volumes/networks, disk usage (`df`), logs. Read-only first; any prune behind an explicit gate. |
| 3 | **Bruno-style API runner** (#4 + #6) | HTTP request collections + assertions. #4's "MCP troubleshooting" = a tool/collection that introspects and calls *other* MCP servers. |
| 4 | **Webpage-slowness** (#7) | Rust network-level analysis: DNS/TCP/TLS/TTFB timing, redirect chain, transfer sizes, header hints. **Not** render metrics (LCP/CLS) — recommend the chrome-devtools MCP for those. |
| 5 | **A2A protocol troubleshooting** (added 2026-07-01) | Tools to diagnose Google Agent2Agent (A2A) endpoints: fetch/validate the Agent Card (`/.well-known/agent.json`), send a `message/send` (and streaming) task and inspect the JSON-RPC response/`Task` lifecycle, surface errors. Close sibling of slice 3 (Bruno/MCP-troubleshooting) — likely shares the HTTP client. Needs its own brainstorm/spec (scope: read/probe vs full client). |

Each later slice gets its own brainstorming → spec → plan → implementation cycle.
**Slice 1 status:** implemented, reviewed clean, on branch `feat/mcp-server` (commits 2999d4d..a38ce41).
