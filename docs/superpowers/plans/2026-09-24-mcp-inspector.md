# MCP Inspector Testing Interface Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `./run inspect` so the developer can test `reclaim-mcp` interactively (Postman-style) in the official MCP Inspector, plus a headless `--check` that verifies the server over real stdio JSON-RPC.

**Architecture:** A bash script builds `reclaim-mcp`, writes a throwaway Inspector session config with the absolute binary path, and launches the pinned Inspector (`npx`) in web mode, or in CLI mode for `--check`. No Rust changes. Docs gain copy-paste examples and a corrected smoke test.

**Tech Stack:** bash (macOS bash 3.2 compatible), Node 26 / npx, `@modelcontextprotocol/inspector@2.8.0`, cargo.

**Spec:** `docs/superpowers/specs/2026-09-24-mcp-inspector-design.md`

## Global Constraints

- Inspector pinned to **2.8.0**, overridable only via the `INSPECTOR_VERSION` env var; the default lives in one place at the top of `scripts/inspect.sh`.
- **No Rust, tool-schema, safety-gate, or policy changes.** `reclaim-mcp` is not modified.
- The `reclaim-mcp` binary makes **zero network calls**; only `npx` downloads the Inspector, at dev time, and the docs say so.
- Inspector mode flags (`--web` / `--cli`) must come **before** other options.
- Scripts follow `scripts/_common.sh` style (`source` it, use `need`, `log`, `die`); bash 3.2 compatible (no `mapfile`, no associative arrays).
- Session config is generated into a temp dir and removed on exit; nothing machine-specific is committed.
- Docs must not claim a "root outside `$HOME`" refusal: the boundary is enforced per item at delete time; `root` is unrestricted for scan/reclaim by design.
- Inspector CLI `--tool-arg` values are JSON-parsed; string args that look like JSON/numbers must be quoted (`'json="{\"a\":1}"'`, `'value="0"'`).
- Commit messages end with `Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>`. Commit only the files each task lists.

## Review Focus

- Inspector can't be fetched / bad version → script exits non-zero and shows npx's stderr (Task 1 negative test).
- Run from a directory other than the repo root → still works (Task 1 test).
- Temp dir is removed after `--check`, pass or fail (Task 1 test).
- Server's stderr banner (`reclaim-mcp 0.1.0 starting on stdio`) must never corrupt the JSON parsed from stdout (Task 1: stderr goes to a file).
- A doc example that silently doesn't work as written (JSON-looking string args) → every example command is executed in Task 2.

---

### Task 1: `scripts/inspect.sh`, `./run inspect`, and `--check`

**Files:**
- Create: `scripts/inspect.sh` (mode 755)
- Modify: `run` (header comment lines 2-11, both `sed -n '2,11p'` ranges, and the `case` block)

**Interfaces:**
- Consumes: `scripts/_common.sh` (`REPO_ROOT`, `log`, `die`, `need`).
- Produces: `./run inspect` (web UI) and `./run inspect --check` (exit 0 = all checks pass). Env override `INSPECTOR_VERSION`.

- [ ] **Step 1: Write the script (the `--check` mode is the test)**

Create `scripts/inspect.sh`:

```bash
#!/usr/bin/env bash
# Interactive MCP testing via the official MCP Inspector (dev-only).
#   ./run inspect            Inspector web UI with the `reclaim` server preloaded
#   ./run inspect --check    headless checks over real stdio JSON-RPC (exit != 0 on failure)
# The reclaim binary makes no network calls; only `npx` fetches the Inspector.
source "$(dirname "${BASH_SOURCE[0]}")/_common.sh"

INSPECTOR_VERSION="${INSPECTOR_VERSION:-2.8.0}"
INSPECTOR_PKG="@modelcontextprotocol/inspector@${INSPECTOR_VERSION}"
EXPECTED_TOOLS="decode,encode,hash,json_compare,json_minify,json_prettify,json_validate,propose_reclaim,reclaim_space,scan_disk,time_convert,time_diff,time_now"

need node
need npx
need cargo

log "building reclaim-mcp"
cargo build -q -p reclaim-mcp
BIN="$REPO_ROOT/target/debug/reclaim-mcp"
[[ -x "$BIN" ]] || die "expected server binary at $BIN"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
trap 'exit 130' INT TERM

# Session config with the absolute binary path (never committed; see spec).
CONFIG="$WORK/inspector.config.json"
node -e 'console.log(JSON.stringify({mcpServers:{reclaim:{command:process.argv[1]}}}))' "$BIN" > "$CONFIG"

# Inspector CLI against the server. JSON result on stdout; server/npx noise goes
# to a file so it can never corrupt the JSON, and is shown only on failure.
inspector_cli() {
  npx -y "$INSPECTOR_PKG" --cli --config "$CONFIG" --server reclaim "$@" 2>"$WORK/stderr" \
    || { cat "$WORK/stderr" >&2; die "inspector CLI failed: $*"; }
}

# assert <description> <result-json> <js expression over j (raw result) and
# t (parsed content[0].text, or null)>
assert() {
  local desc="$1" json="$2" expr="$3"
  printf '%s' "$json" | node -e '
    let s = "";
    process.stdin.on("data", d => s += d).on("end", () => {
      const j = JSON.parse(s);
      let t = null;
      try { t = JSON.parse(j.content[0].text); } catch (_) {}
      process.exit(new Function("j", "t", "return (" + process.argv[1] + ")")(j, t) ? 0 : 1);
    });' "$expr" || die "check failed: $desc"
  log "ok: $desc"
}

run_checks() {
  local out

  out="$(inspector_cli --method tools/list)"
  assert "tools/list returns the 13 expected tools" "$out" \
    "j.tools.map(x => x.name).sort().join(',') === '$EXPECTED_TOOLS'"

  out="$(inspector_cli --method tools/call --tool-name hash --tool-arg input=abc algo=sha256)"
  assert "hash sha256(abc) is correct" "$out" \
    "j.isError === false && t.hex === 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad'"

  mkdir "$WORK/empty"
  out="$(inspector_cli --method tools/call --tool-name reclaim_space \
    --tool-arg 'ids=["no-such-id"]' "root=$WORK/empty")"
  assert "reclaim_space skips an unknown id and deletes nothing" "$out" \
    "j.isError === false && t.moved_to_trash.length === 0 && t.permanently_deleted.length === 0 && t.skipped.length === 1 && t.skipped[0].id === 'no-such-id'"

  log "all checks passed"
}

case "${1:-}" in
  --check) run_checks ;;
  "")
    log "Inspector $INSPECTOR_VERSION → reclaim server (Ctrl-C to stop)"
    npx -y "$INSPECTOR_PKG" --web --config "$CONFIG" --server reclaim
    ;;
  *) die "usage: ./run inspect [--check]" ;;
esac
```

Then: `chmod +x scripts/inspect.sh`

- [ ] **Step 2: Wire it into `./run`**

In `run`, insert after the `#   ./run mcp  [--smoke]   MCP server on stdio` line:

```
#   ./run inspect [--check]  Test MCP tools in the Inspector UI (or headless check)
```

Change both `sed -n '2,11p'` to `sed -n '2,12p'` (the help block is now one line longer). Add to the `case`, after the `mcp)` line:

```bash
  inspect) exec "$DIR/inspect.sh" "$@" ;;
```

- [ ] **Step 3: Run the checks — expect PASS**

Run: `./run inspect --check`
Expected: four `ok:`/`all checks passed` lines, exit 0. If `tools/list` fails, print `"$out"` and compare with `EXPECTED_TOOLS`.

- [ ] **Step 4: Negative tests — expect FAIL / correct behavior**

Run each and confirm:

```bash
# Bad Inspector version → non-zero exit, npx error visible
INSPECTOR_VERSION=0.0.0-nope ./run inspect --check; echo "exit=$?"
# Expected: npm error text, then "✗ inspector CLI failed: ...", exit=1

# Unknown flag → usage error
./run inspect --bogus; echo "exit=$?"
# Expected: "✗ usage: ./run inspect [--check]", exit=1

# From another directory
(cd /tmp && /Users/kirankumar/Documents/GitHub/Reclaim/run inspect --check); echo "exit=$?"
# Expected: all checks pass, exit=0

# Temp dir cleanup: no leftover config after a run
ls "${TMPDIR:-/tmp}"/tmp.*/inspector.config.json 2>/dev/null; echo "leftover-check-done"
# Expected: no files listed before "leftover-check-done"
```

- [ ] **Step 5: Confirm help output**

Run: `./run help`
Expected: the list now includes the `inspect [--check]` line and still ends cleanly.

- [ ] **Step 6: Commit**

```bash
git add scripts/inspect.sh run
git commit -m "feat(mcp): add ./run inspect (MCP Inspector UI + headless --check)

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>"
```

---

### Task 2: Docs — examples and corrected smoke test

**Files:**
- Create: `docs/mcp-examples.md`
- Modify: `docs/mcp.md` (add "Interactive testing" section before `## Build`; replace the `## Quick smoke test` section)

**Interfaces:**
- Consumes: `./run inspect` and its `--check` from Task 1; tool arg names from `reclaim-mcp/src/reclaim_tools.rs` and `tools/*.rs`.
- Produces: none.

- [ ] **Step 1: Write `docs/mcp-examples.md`**

````markdown
# MCP tool examples

Copy-paste requests for `reclaim-mcp`. Two ways to send them:

- **Web UI (recommended):** `./run inspect`, pick a tool, fill the form, **Run**.
  The form is generated from each tool's schema; the raw JSON-RPC is shown too.
- **Headless CLI:** the same Inspector without a browser:

  ```bash
  cargo build -p reclaim-mcp
  echo '{"mcpServers":{"reclaim":{"command":"'"$PWD"'/target/debug/reclaim-mcp"}}}' > /tmp/reclaim-mcp.json
  npx -y @modelcontextprotocol/inspector@2.8.0 --cli --config /tmp/reclaim-mcp.json \
    --server reclaim --method tools/call --tool-name hash --tool-arg input=abc algo=sha256
  ```

> **CLI quirk:** `--tool-arg` values are JSON-parsed. A string that looks like JSON
> or a number must be quoted as a JSON string, e.g. `'json="{\"a\":1}"'` or
> `'value="0"'`. Without that you get `invalid type: map, expected a string`.
> The web UI does not have this problem.

The Inspector is a dev-time tool: `npx` downloads it on first use. The
`reclaim-mcp` binary itself makes no network calls.

## Pure utilities (safe to run freely)

| Tool | Arguments | Expect |
|---|---|---|
| `hash` | `input=abc`, `algo=sha256` | `hex` = `ba7816bf…0015ad` |
| `hash` | `input=abc`, `algo=md5` | `hex` = `900150983cd24fb0d6963f7d28e17f72` |
| `encode` | `input=hello`, `scheme=base64` | `output` = `aGVsbG8=` |
| `decode` | `input=aGVsbG8=`, `scheme=base64` | `text` = `hello`, `hex` = `68656c6c6f` |
| `json_prettify` | `json={"a":1,"b":[true,null]}` | indented JSON in `pretty` |
| `json_validate` | `json={"a":` | `valid: false`, `error.line` 1, `error.column` 5 |
| `json_compare` | `left={"a":1}`, `right={"a":2}` | `equal: false`, one `changed` diff at `/a` |
| `time_convert` | `value=0`, `from=epoch_s`, `to_tz=UTC` | `rfc3339` = `1970-01-01T00:00:00+00:00` |
| `time_diff` | `a=0`, `b=3600` | `seconds` = 3600, `human` = `0d 1h 0m 0s` |
| `time_now` | `tz=UTC` | current time in RFC3339 |

## Disk tools

These act on your real filesystem. Go in this order.

1. **`scan_disk`** (read-only). Optional `root` (defaults to `~/Library`). Returns
   items with `id`, `path`, `bytes`, `risk` (`safe` / `review` / `protected`) and a
   `rationale`.
2. **`propose_reclaim`** (read-only). Optional `root`, `policy` = `shadow`
   (default) / `assisted` / `auto_safe`. Only Safe-class items are ever proposed.
3. **`reclaim_space`** (**changes your disk**). `ids` (required), optional
   `root`, `mode` = `trash` (default, reversible) or `permanent` (irreversible;
   opt-in). Passing a real Safe/Review id moves it to the Trash. Start with the
   refusal examples below.

### See the safety gate refuse

`reclaim_space` reports refused ids in a `skipped` list (the call itself still
succeeds); nothing is deleted for a refused id.

- **Unknown id:** `ids=["does-not-exist"]` → `skipped[0].reason` =
  `skipped: no longer present in scan (already gone or never existed)`;
  `moved_to_trash` and `permanently_deleted` are empty.
- **Protected item:** run `scan_disk`, copy the `id` of any item whose `risk` is
  `protected`, then call `reclaim_space` with `ids=["<that id>"]`. It comes back in
  `skipped` with a reason explaining the denial; nothing is moved.

The `$HOME` boundary is enforced per item at delete time (`root` itself is not
restricted for scanning), so an item outside `$HOME` can never be deleted either.
````

- [ ] **Step 2: Add "Interactive testing" and fix the smoke test in `docs/mcp.md`**

Insert before `## Build`:

````markdown
## Interactive testing

```bash
./run inspect            # opens the MCP Inspector with the reclaim server preloaded
./run inspect --check    # headless: verifies tool list + a pure call + the gate over real stdio
```

Pick a tool, fill the schema-generated form, send it, and read the response and
the raw JSON-RPC — like an API client. Copy-paste requests are in
[`mcp-examples.md`](mcp-examples.md). Note `reclaim_space` is a real call: it
still defaults to Trash and still goes through the safety gate.
````

Replace the whole `## Quick smoke test` section with:

````markdown
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
````

- [ ] **Step 3: Execute every example — none may be wrong as written**

Using the CLI form from the doc (config at `/tmp/reclaim-mcp.json`, server built), run each row of the pure-utilities table with the quoting from the CLI quirk note and confirm the "Expect" column matches. Run `./run mcp --smoke` and confirm two JSON responses (one with `serverInfo`, one with 13 tools). Run the unknown-id example and confirm the exact `reason` text. Fix the doc, not the expectation, if any differ.

Run: `./run mcp --smoke 2>/dev/null | head -c 400`
Expected: JSON-RPC `initialize` result containing `reclaim-mcp`.

- [ ] **Step 4: Commit**

```bash
git add docs/mcp-examples.md docs/mcp.md
git commit -m "docs(mcp): add Inspector examples, interactive testing section, fix smoke test

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>"
```

---

### Task 3: End-to-end verification of the web UI (no code changes)

**Files:** none.

**Interfaces:** Consumes Task 1's `./run inspect`.

- [ ] **Step 1: Launch the web UI in the background and capture output**

```bash
./run inspect > /tmp/inspect-web.log 2>&1 &
echo $! > /tmp/inspect-web.pid
sleep 15; cat /tmp/inspect-web.log
```
Expected: the Inspector prints a localhost URL (with an auth token if v2 uses one).

- [ ] **Step 2: Confirm it serves and is bound to localhost only**

```bash
URL=$(grep -Eo 'http://[^ ]+' /tmp/inspect-web.log | head -1)
echo "$URL"; curl -s -o /dev/null -w '%{http_code}\n' "$URL"
lsof -nP -iTCP -sTCP:LISTEN -a -p "$(cat /tmp/inspect-web.pid)" 2>/dev/null | head
```
Expected: `200`; the listener is on `127.0.0.1` / `localhost`, not `*`. If it binds to all interfaces, stop and report — the spec forbids exposing it.

- [ ] **Step 3: Hand off the interactive pass to the user**

Ask the user to open the URL, confirm the `reclaim` server shows 13 tools, run `json_validate` with `{"a":`, and run `reclaim_space` with `ids=["does-not-exist"]` to see `skipped`. This is the one thing automation can't confirm (that the form UI renders and works).

- [ ] **Step 4: Clean up**

```bash
kill "$(cat /tmp/inspect-web.pid)"; sleep 1
rm -f /tmp/inspect-web.log /tmp/inspect-web.pid
```
Expected: process gone; no stray `reclaim-mcp` process (`pgrep reclaim-mcp` prints nothing).
