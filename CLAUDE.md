# Reclaim

A local, risk-aware disk-space visualizer and recovery tool for developer Macs.
See `Prd.md` for the full product spec — it is the source of truth for behavior.

## What this is

Reclaim scans a developer's filesystem, classifies every reclaimable item by **how
safe it is to delete**, and lets the user recover space safely (Trash-first,
reversible). The same engine is exposed over MCP so an agent can drive it under the
identical safety guarantees.

The product's whole value is **correct risk classification + a safety gate that the
UI cannot bypass**. Treat those two things as load-bearing.

## Architecture (single source of truth = the Rust core)

```
src/ (React webview)  ─┐
                       ├─ scan() / reclaim() ─▶  reclaim-core (Rust)  ─▶  filesystem
MCP wrapper / agent  ─┘                          • parallel walk + true on-disk size
                                                 • detector catalogue → risk class
                                                 • SAFETY GATE (re-validate every delete)
                                                 • execute: Trash (default) | permanent
```

| Path | Role |
|---|---|
| `reclaim-core/` | Rust library + CLI. The engine. All risk logic and the safety gate live here. |
| `src-tauri/` | Thin Tauri v2 wrapper. Exposes `scan`/`reclaim` commands. **No business logic.** |
| `src/` | React + TS treemap UI. Renders; never decides what is deletable. |
| `reclaim-core/src/detectors/` | The declarative detector catalogue (the domain knowledge). |

## Non-negotiable invariants

These come straight from the PRD's product principles (§5) and safety section (§11).
A change that violates one of these is a bug, not a tradeoff:

1. **The backend decides what's deletable, never the UI.** The UI/agent sends item
   IDs; the core re-derives each item's current risk class and path at execution
   time and refuses anything Protected or outside `$HOME`. Never trust the request's
   claimed risk class.
2. **Reversible by default.** `reclaim` moves to Trash unless the caller explicitly
   passes permanent mode for a specific item. Default is never permanent.
3. **HOME boundary, no sudo.** The core refuses any path outside the user's home dir.
   There is no code path that escalates privileges.
4. **Ambiguity resolves to Protected.** A detector that cannot confirm "Safe" must
   classify up to Review or Protected, never down.
5. **A failed delete never aborts the batch.** Failures are collected and reported,
   not swallowed and not fatal.
6. **Local and silent.** Zero network calls, zero telemetry. Adding any outbound
   connection breaks a stated product guarantee.

## Risk model

Every item has exactly one class:

| Class | Color token | Selectable | Bulk "Select safe" |
|---|---|---|---|
| `safe` | mint `#3FB950` | yes | yes |
| `review` | amber `#D29922` | yes | no (opt-in per item) |
| `protected` | steel `#6E7B8B` | no (inert) | never |

## Conventions

- **Money/size math:** bytes are `u64`; never use floats for byte accounting.
- **Detectors are additive and declarative.** Adding one must not require touching
  the UI or the safety core — implement the `Detector` trait and register it.
- **Schema stability:** the scan-output JSON (`Prd.md` §10) is a contract shared by
  the UI and MCP. Changing it is a breaking change; version it.
- **Color is information.** In the UI, saturated color is reserved for the three risk
  classes. Don't introduce decorative color.

## Common commands

```bash
# Rust core
cargo test                         # unit tests (the safety gate has the most)
cargo run -p reclaim-core --bin reclaim -- scan ~/Library   # CLI scan, prints JSON
cargo run -p reclaim-core --bin reclaim -- scan ~/Library --pretty

# Frontend
pnpm install
pnpm dev                           # Vite dev server (uses mock data if core absent)
pnpm tauri dev                     # full desktop app (Rust + webview)
```

## When changing safety-critical code

Anything under `reclaim-core/src/safety.rs`, `reclaim.rs`, or a detector's risk logic:
add or update the tests that prove a Protected item can never be deleted and that the
HOME boundary holds. "Zero false-deletion of Protected items" is a hard correctness
bar (PRD §12), so it must be enforced by a test, not by reviewer attention.
