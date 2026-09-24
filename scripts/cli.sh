#!/usr/bin/env bash
# Headless CLI over the same core. Passes all args straight to the `reclaim` bin.
# Examples:
#   ./run cli scan ~/Library --pretty
#   ./run cli reclaim ~/Library aerials jetbrains-caches
#   ./run cli safe ~/Library
# Whatever you pass, the core re-validates risk + the $HOME boundary.
source "$(dirname "${BASH_SOURCE[0]}")/_common.sh"

need cargo
if [[ $# -eq 0 ]]; then
  log "no args — defaulting to: scan \$HOME/Library --pretty"
  set -- scan "$HOME/Library" --pretty
fi
exec cargo run -q -p reclaim-core --bin reclaim -- "$@"
