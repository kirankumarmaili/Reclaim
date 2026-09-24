#!/usr/bin/env bash
# Shared helpers for Reclaim run scripts. Sourced, not executed.
set -euo pipefail

# Repo root = parent of this scripts/ dir, resolved regardless of CWD.
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

log()  { printf '\033[1;36m▶ %s\033[0m\n' "$*" >&2; }
die()  { printf '\033[1;31m✗ %s\033[0m\n' "$*" >&2; exit 1; }

need() {
  command -v "$1" >/dev/null 2>&1 || die "'$1' not found on PATH — see README Prerequisites."
}
