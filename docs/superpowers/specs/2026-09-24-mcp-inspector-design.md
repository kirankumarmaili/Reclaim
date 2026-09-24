# MCP interactive testing (Inspector wrapper) — Design

Date: 2026-09-24 · Branch: `feat/mcp-server` · Status: approved; revised 2026-09-24 after probing Inspector 2.8.0 against the real server

## Goal

Give the developer a Postman-style way to exercise `reclaim-mcp` by hand: pick a
tool, fill a schema-generated form, send the call, read the response and the raw
JSON-RPC. Developer tool only; not shipped to end users.

## Non-goals

- No Rust changes, no change to tool schemas, the safety gate, or the policy gate.
- No custom UI (a bespoke Postman-like page was considered and rejected for now).
- Not the automated stdio integration test (`reclaim-mcp/tests/`); that is a
  separate follow-up.

## Approach

Wrap the official **MCP Inspector** (`@modelcontextprotocol/inspector`), pinned to
**2.8.0** (`latest` at time of writing). Verified CLI surface for 2.8.0:

- Web UI: `mcp-inspector --web --config <file>` (`--server` is accepted but has no
  effect in the web UI in 2.8.0 — it lists every server in the file — so we omit it)
- Headless: `mcp-inspector --cli --config <file> --server <name> --method tools/list`
  (also `--method tools/call --tool-name <n> --tool-arg k=v`)

Mode flags (`--web`/`--cli`) must precede other options.

## Components

1. **`scripts/inspect.sh`** + `./run inspect [--check]`
   - Sources `_common.sh`; `need node`, `need npx`, `need cargo`.
   - `cargo build -p reclaim-mcp` (debug build is sufficient).
   - Default: launch the Inspector web UI against the preloaded `reclaim` server.
   - Generates the session config at run time into a temp dir (absolute binary
     path, cleaned up on exit) — see component 2.
   - `--check`: headless checks through the Inspector CLI over real stdio
     JSON-RPC; exits non-zero with the server's stderr on any failure:
     (a) `tools/list` returns exactly the 13 expected tool names;
     (b) `hash` (sha256 of `abc`) returns the known digest, `isError` false;
     (c) `reclaim_space` with an unknown ID against an empty temp root reports it
     `skipped` and moves/deletes nothing (proves the gate path over the wire).
   - `./run` header comment and help `sed` range updated for the new command.
   - The Inspector version lives in one variable at the top of the script.

2. **Session config (generated, not committed)** — `{"mcpServers":{"reclaim":{"command":"<abs path to target/debug/reclaim-mcp>"}}}`,
   written by the script to a temp file and passed as `--config`. Generated so
   the absolute path is never stale or machine-specific. Verified working with
   Inspector 2.8.0.

3. **`docs/mcp-examples.md`** — copy-paste arguments per tool group, taken from
   the real arg structs (`ScanArgs`, `ProposeArgs`, `ReclaimArgs` in
   `reclaim-mcp/src/reclaim_tools.rs` and the `tools/*.rs` inputs):
   - Pure tools (`json_*`, `encode`/`decode`/`hash`, `time_*`): safe to run freely.
   - Disk tools in order `scan_disk` → `propose_reclaim` → `reclaim_space`.
   - Two expected-refusal calls so the gate is visible over the real protocol:
     an unknown ID, and a Protected item's ID copied from a `scan_disk` result.
     Both come back `skipped` with a reason. (The `$HOME` boundary is enforced
     per item at delete time; `root` itself is not restricted for scan/reclaim,
     by design — see `reclaim-core/src/scan.rs` — so a "root outside HOME"
     refusal is NOT demonstrable and is not claimed.)
   - Notes the CLI quirk: `--tool-arg` values are JSON-parsed, so string
     arguments that look like JSON or numbers must be quoted, e.g.
     `--tool-arg 'json="{\"a\":1}"'`, `--tool-arg 'value="0"'`. The web UI form
     is schema-driven and does not need this.

4. **`docs/mcp.md`** — add an "Interactive testing" section pointing at
   `./run inspect`; replace the stale "Quick smoke test" (empty `initialize`
   params, no `initialized` notification) with the working handshake already in
   `scripts/mcp.sh --smoke`.

## Safety and product guarantees

- `reclaim_space` invoked from the Inspector is a real call. Behavior is
  unchanged: Trash by default, safety gate re-validates every ID, staged-autonomy
  policy applies. Docs and examples state this and lead with propose/dry-run.
- The `reclaim-mcp` binary is untouched: zero network, zero telemetry. The only
  network use is `npx` fetching the Inspector at dev time; documented as such.
- The Inspector is launched with its default localhost binding; the script does
  not add flags that expose it on other interfaces.

## Testing

- `./run inspect --check` is the automated verification for the script/config:
  it must pass (13 tools) on a clean checkout with Node + cargo present.
- One manual pass: launch the web UI, confirm it connects, call `json_validate`
  and one expected-refusal disk call, confirm the refusal surfaces.
- Failure modes covered: missing `node`/`npx` → `die` with a clear message;
  Inspector download failure → the npx error is surfaced, script exits non-zero.

## Resolved during implementation planning

- Config schema and binary-path injection: generated temp file (above).
- `--check` asserts names, not just the count.
