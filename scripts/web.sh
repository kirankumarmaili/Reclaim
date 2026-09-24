#!/usr/bin/env bash
# UI-only in a browser on bundled mock data (no native build, never touches disk).
# Open http://localhost:1420 — the header shows "demo data".
source "$(dirname "${BASH_SOURCE[0]}")/_common.sh"

need pnpm
log "pnpm dev (Vite, mock data) → http://localhost:1420"
exec pnpm dev "$@"
