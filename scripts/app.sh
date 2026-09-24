#!/usr/bin/env bash
# The real thing: native desktop app (Tauri + Vite + live reclaim-core).
# First run compiles the Tauri toolchain and takes a few minutes.
source "$(dirname "${BASH_SOURCE[0]}")/_common.sh"

need pnpm
log "pnpm tauri dev (native window, live scan/reclaim)"
exec pnpm tauri dev "$@"
