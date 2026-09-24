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
