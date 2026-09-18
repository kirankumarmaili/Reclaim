#!/usr/bin/env bash
# Release builds. Default builds everything; pass a target to narrow.
#   ./run build         → core CLI + mcp (release) + frontend
#   ./run build cli     → just the reclaim CLI  → ./target/release/reclaim
#   ./run build mcp     → just the MCP server   → ./target/release/reclaim-mcp
#   ./run build app     → bundle the distributable macOS .app
#   ./run build web     → production frontend bundle → ./dist
source "$(dirname "${BASH_SOURCE[0]}")/_common.sh"

target="${1:-all}"

build_cli() { need cargo; log "release: reclaim CLI";  cargo build --release -p reclaim-core --bin reclaim; log "→ ./target/release/reclaim"; }
build_mcp() { need cargo; log "release: reclaim-mcp";  cargo build --release -p reclaim-mcp;               log "→ ./target/release/reclaim-mcp"; }
build_web() { need pnpm;  log "frontend production bundle"; pnpm build; log "→ ./dist"; }
build_app() { need pnpm;  log "bundle distributable .app"; pnpm tauri build; }

case "$target" in
  cli) build_cli ;;
  mcp) build_mcp ;;
  web) build_web ;;
  app) build_app ;;
  all) build_cli; build_mcp; build_web ;;
  *)   die "unknown build target '$target' (cli|mcp|web|app|all)" ;;
esac

log "Build complete."
