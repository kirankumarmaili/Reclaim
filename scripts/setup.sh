#!/usr/bin/env bash
# Install frontend deps (also fetches the Tauri CLI) and warm the Rust build.
source "$(dirname "${BASH_SOURCE[0]}")/_common.sh"

need pnpm
need cargo

log "pnpm install (frontend deps + Tauri CLI)"
pnpm install

log "cargo fetch (Rust deps)"
cargo fetch

log "Setup complete. Try: ./run app   |   ./run web   |   ./run cli scan ~/Library"
