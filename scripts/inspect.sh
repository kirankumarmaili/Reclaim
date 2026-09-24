#!/usr/bin/env bash
# Interactive MCP testing via the official MCP Inspector (dev-only).
#   ./run inspect            Inspector web UI with the `reclaim` server preloaded
#   ./run inspect --check    headless checks over real stdio JSON-RPC (exit != 0 on failure)
# The reclaim binary makes no network calls; only `npx` fetches the Inspector.
source "$(dirname "${BASH_SOURCE[0]}")/_common.sh"

INSPECTOR_VERSION="${INSPECTOR_VERSION:-2.8.0}"
INSPECTOR_PKG="@modelcontextprotocol/inspector@${INSPECTOR_VERSION}"
# Writable server list for the web UI (kept out of the repo; may hold remote-server
# headers). Set INSPECTOR_CATALOG=~/.mcp-inspector/mcp.json to share the Inspector's default.
CATALOG="${INSPECTOR_CATALOG:-$HOME/.mcp-inspector/reclaim-catalog.json}"
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

# Read-only session config for --check (absolute binary path; never committed).
CONFIG="$WORK/inspector.config.json"
node -e 'console.log(JSON.stringify({mcpServers:{reclaim:{command:process.argv[1]}}}))' "$BIN" > "$CONFIG"

# The web UI needs a *writable* catalog (--config is read-only: adds get a 403), so
# users can add their own local (stdio) or remote (http/sse) servers. Ensure
# `reclaim` is present and points at this build; every other entry is preserved.
# A corrupt catalog aborts instead of being overwritten.
seed_catalog() {
  node -e '
    const fs = require("fs"), path = require("path");
    const [file, bin] = process.argv.slice(1);
    let c = {};
    try { c = JSON.parse(fs.readFileSync(file, "utf8")); }
    catch (e) { if (e.code !== "ENOENT") throw e; }
    c.mcpServers = Object.assign({}, c.mcpServers);
    c.mcpServers.reclaim = Object.assign({}, c.mcpServers.reclaim, { command: bin });
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, JSON.stringify(c, null, 2) + "\n");
  ' "$1" "$BIN" || die "could not update Inspector catalog $1 (is it valid JSON?)"
}

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
    });' "$expr" || {
      printf 'result was: %s\n' "$(printf '%s' "$json" | head -c 2000)" >&2
      if [[ -s "$WORK/stderr" ]]; then printf 'server/npx stderr:\n' >&2; cat "$WORK/stderr" >&2; fi
      die "check failed: $desc"
    }
  log "ok: $desc"
}

run_checks() {
  local out

  out="$(inspector_cli --method tools/list)"
  log "server reports: $(printf '%s' "$out" | node -e 'let s = ""; process.stdin.on("data", d => s += d).on("end", () => console.log(JSON.parse(s).tools.map(x => x.name).sort().join(",")))')"
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

  # The web UI uses a writable catalog: seeding must keep user-added servers and
  # repoint `reclaim` at this build (the stale path below would fail to start).
  printf '%s' '{"mcpServers":{"other":{"type":"streamable-http","url":"https://example.invalid/mcp"},"reclaim":{"command":"/stale/path"}}}' > "$WORK/catalog.json"
  seed_catalog "$WORK/catalog.json"
  out="$(npx -y "$INSPECTOR_PKG" --cli --catalog "$WORK/catalog.json" --server reclaim --method tools/list 2>"$WORK/stderr")" \
    || { cat "$WORK/stderr" >&2; die "inspector CLI failed via catalog"; }
  assert "seeded catalog starts reclaim (stale path repointed)" "$out" "j.tools.length === 13"
  node -e 'const c = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")); process.exit(c.mcpServers.other && c.mcpServers.other.url === "https://example.invalid/mcp" ? 0 : 1)' \
    "$WORK/catalog.json" || die "check failed: seeding dropped a user-added server"
  log "ok: seeding kept the user-added server"

  log "all checks passed"
}

case "${1:-}" in
  --check) run_checks ;;
  "")
    seed_catalog "$CATALOG"
    log "Inspector $INSPECTOR_VERSION — servers saved in $CATALOG (add local/remote ones in the UI)"
    log "Ctrl-C to stop"
    npx -y "$INSPECTOR_PKG" --web --catalog "$CATALOG"
    ;;
  *) die "usage: ./run inspect [--check]" ;;
esac
