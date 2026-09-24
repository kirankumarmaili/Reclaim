#!/usr/bin/env bash
# The safety net: workspace tests + clippy + frontend typecheck.
# Pass extra args through to cargo test, e.g. ./run test safety -- --nocapture
source "$(dirname "${BASH_SOURCE[0]}")/_common.sh"

need cargo
need pnpm

log "cargo test (core + mcp)"
cargo test "$@"

log "cargo clippy --all-targets -D warnings"
cargo clippy --all-targets -- -D warnings

log "pnpm build (tsc typecheck + vite build)"
pnpm build

log "All checks passed."
