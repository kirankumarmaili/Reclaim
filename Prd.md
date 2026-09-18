# Reclaim — Product Requirements Document

**A disk-space visualizer and recovery tool for developer machines.**

| | |
|---|---|
| Version | 1.0 |
| Status | **M0 (MVP) implemented** — M1–M3 planned (see §13) |
| Last updated | 2026-06-24 |
| Platforms (v1) | macOS (Apple Silicon + Intel) |
| Distribution | Tauri desktop app; MCP server wrapper |

---

## 1. Summary

Reclaim is a local desktop tool that shows a developer exactly where their disk space went and lets them recover it safely with a click. Unlike generic disk visualizers, Reclaim understands *developer cruft* — orphaned container VMs, old IDE versions, build caches, derived data — and classifies every item by how safe it is to delete. The interface is a treemap where block size is bytes and block color is **risk**, paired with a one-click reclaim flow that moves selected items to the Trash by default (reversible) before any permanent deletion.

The same scanning-and-deletion engine is exposed as an MCP server, so an agent (e.g. Brhaspati) can propose and execute cleanups under the same safety guarantees.

---

## 2. Problem

Developer Macs fill up fast, and the space is consumed by artifacts that general-purpose tools don't recognize or can't safely judge:

- **Generic visualizers** (DaisyDisk, GrandPerspective) show *where* space went but have no opinion on what's safe to remove. They treat a 29 GB orphaned Docker VM and a 29 GB Postgres volume identically, leaving the risky judgment entirely to the user.
- **Manual cleanup** via `du`/`rm` is error-prone and not repeatable. It requires knowing that `~/Library/Containers/com.docker.docker` is orphaned, that old `JetBrains/IntelliJIdea2024.*` folders are stale, that `vm_bundles` rebuild but Postgres volumes don't.
- **macOS "Manage Storage"** is coarse, ignores developer-specific bloat, and offers no transparency into what it deletes.

The result: developers either hoard space they could reclaim, or delete something they needed (a live database volume, an IDE config). What's missing is a tool that combines a clear visual of consumption with **risk-aware, reversible recovery** tuned for a developer's filesystem.

---

## 3. Goals and non-goals

### Goals
- Show disk consumption visually, with block color encoding deletion risk.
- Detect developer-specific reclaimable space using purpose-built detectors.
- Make recovery safe by default: reversible (Trash) first, permanent only on explicit opt-in.
- Never offer real data (databases, current configs, documents) for deletion.
- Be fully local — no network calls, no telemetry, no accounts.
- Expose the engine over MCP so agents can drive it under the same guarantees.

### Non-goals (v1)
- Not a backup tool, file manager, or duplicate finder.
- Not a system optimizer / "cleaner" that touches system internals or requires kernel extensions.
- No cloud sync, multi-machine dashboards, or team features.
- No automatic deletion without a human or an explicit policy (see §9 staged autonomy).

---

## 4. Target users

| Persona | Need |
|---|---|
| **Primary — the working developer** (e.g. polyglot dev with Docker/OrbStack, JetBrains, VS Code, multiple language toolchains) | "I'm out of space and I don't want to spend an hour figuring out what's safe to delete." |
| **Secondary — the careful developer** | Wants transparency and reversibility; won't run a "cleaner" they don't trust. Reclaim's risk model and Trash-first deletion are aimed squarely here. |
| **Tertiary — the agent operator** | Runs Brhaspati or similar; wants disk hygiene as an automatable, policy-gated task rather than a manual chore. |

---

## 5. Product principles

1. **Diagnose before you delete.** The default state is a read-only scan. Nothing is removed without an explicit action.
2. **Color is information, not decoration.** Every visual element encodes risk or size — the treemap is a risk map, not a mood board.
3. **The backend decides what's deletable, never the UI.** Risk class and path boundaries are re-validated server-side on every delete; a compromised or buggy front end cannot delete protected data.
4. **Reversible by default.** Reclaim moves to Trash first. Permanent deletion is a deliberate, separate choice.
5. **Local and silent.** No data leaves the machine. No analytics. The tool earns trust by asking for nothing.

---

## 6. Core concept: the risk model

Every scanned item is assigned exactly one risk class. This classification is the heart of the product.

| Class | Meaning | UI color | Selectable? | Auto-select eligible? |
|---|---|---|---|---|
| **Safe** | Regenerates automatically with no user-visible loss (caches, derived data, downloaded media, orphaned VMs). | Mint | Yes | Yes (via "Select safe") |
| **Review** | Reclaimable, but with a cost — re-download, rebuild time, or possibly-wanted local state. | Amber | Yes | No — user opts in per item |
| **Protected** | Real data or actively-in-use config (container volumes, current IDE, Documents, system). | Steel | No | Never |

A detector may downgrade an item to Protected based on live signals (e.g. a Docker VM is only "Safe / orphaned" if Docker Desktop is absent *and* not running *and* not the active context). Ambiguity always resolves toward the more protective class.

---

## 7. Functional requirements

### 7.1 Scan engine
- **FR-1** Scan a configurable root (default `~/Library`; v1.1 adds full `$HOME` and whole-disk).
- **FR-2** Compute true on-disk size per node, accounting for sparse files (e.g. OrbStack `data.img`).
- **FR-3** Run the detector catalogue (§8) to assign risk class and human-readable rationale to each item.
- **FR-4** Complete a `~/Library` scan in under ~5 seconds on a typical SSD via parallel directory walking.
- **FR-5** Return results as a stable JSON schema (§10), consumable by both the UI and the MCP layer.

### 7.2 Visualization
- **FR-6** Render a squarified treemap of the scanned root; block area ∝ bytes, block color = risk class.
- **FR-7** Show capacity context: total disk, current free, and projected free after the current selection.
- **FR-8** Hover reveals name, size, risk, and rationale. Click toggles selection (Protected blocks are inert).
- **FR-9** On reclaim, animate selected blocks draining out and the freed space flowing into the capacity meter.

### 7.3 Recovery flow
- **FR-10** A side panel lists reclaimable items grouped by detector, each with checkbox, size, risk badge, and rationale.
- **FR-11** "Select safe" bulk-selects all Safe-class items. Review items are never bulk-selected.
- **FR-12** Reclaim action shows total to be freed and requires a single confirm.
- **FR-13** **Default deletion = move to Trash** (reversible). A clearly-labeled advanced toggle enables permanent deletion (e.g. for items too large for Trash to be useful, like a 29 GB VM).
- **FR-14** Backend re-validates every target's risk class and path boundary before acting; rejects anything Protected or outside the permitted root.
- **FR-15** Report actual bytes freed; reconcile against estimate and surface any items that failed (e.g. in use).

### 7.4 Transparency & history
- **FR-16** Show a per-session log of what was removed, where it went (Trash vs permanent), and bytes freed.
- **FR-17** (v1.1) Persist scan history locally to chart consumption over time and show "what grew since last scan."

---

## 8. Detector catalogue

Detectors encode the domain knowledge. Each defines: target paths, risk class, and the live conditions that confirm or downgrade the class.

| Detector | Target | Default risk | Detection / downgrade logic |
|---|---|---|---|
| **Orphaned Docker Desktop** | `~/Library/Containers/com.docker.docker` | Safe | Safe only if `/Applications/Docker.app` absent **and** no `Docker Desktop` process **and** active docker context ≠ `desktop-linux`. Otherwise Protected. |
| **JetBrains old versions** | `~/Library/Application Support/JetBrains/<Product><Version>` | Review | Group by product; keep newest *N* (default 1) → Protected; older → Review. Index caches under `~/Library/Caches/JetBrains` → Safe. |
| **Electron app caches** | `Cache`, `Code Cache`, `GPUCache`, `DawnCache`, `CachedData`, `Crashpad` under each app's Application Support | Safe | Size threshold (≥5 MB) to surface; app data folders themselves are not targeted. |
| **Aerial wallpapers** | `~/Library/Application Support/com.apple.wallpaper/aerials` | Safe | Re-fetched on demand by macOS. |
| **Xcode derived data / simulators** | `Developer/Xcode/DerivedData`, `Developer/CoreSimulator/Caches`, `Developer/CoreSimulator/Devices` | Review | Rebuilds; flagged Review because rebuild cost is non-trivial (recompiling, re-warming simulators, losing installed app data). `Devices` has no per-device split, so it downgrades to Protected whenever any simulator is currently booted. |
| **Language build caches** | `~/.m2/repository`, `~/.gradle/caches`, `~/Library/Caches/pip`, `~/.cache/yarn` | Review | Re-downloads; opt-in only. |
| **Local sandbox bundles** | `~/Library/Application Support/Claude/vm_bundles` | Review | Rebuilt when next used. |
| **Container volumes** | OrbStack / Docker volumes | **Protected** | Always Protected — real data. Listed for transparency, never selectable. |
| **iOS device backups** | `~/Library/Application Support/MobileSync/Backup` | **Protected** (report-only) | Too risky to auto-offer; surfaced with a "review manually" note. |
| **node_modules** | project trees under `$HOME` | Report-only | Reported as aggregate; per-project deletion only, never bulk. |

New detectors are additive and declarative — adding one must not require touching the UI or the safety core.

---

## 9. Architecture

```
┌──────────────────────────────┐        ┌──────────────────────────────┐
│  Reclaim UI (webview)        │        │  Brhaspati / other agent     │
│  React treemap + reclaim flow│        │  (proposes cleanups)         │
└──────────────┬───────────────┘        └──────────────┬───────────────┘
               │ scan() / reclaim()                     │ MCP tools
               ▼                                        ▼
        ┌───────────────────────────────────────────────────────┐
        │  Reclaim Core (Rust)  — the single source of truth     │
        │  • parallel filesystem walk + true-size accounting     │
        │  • detector catalogue → risk classification            │
        │  • SAFETY GATE: re-validate risk class + path boundary │
        │  • execute: move-to-Trash (default) | permanent        │
        └───────────────────────────────────────────────────────┘
```

- **Packaging: Tauri** (Rust core + system webview). Chosen over Electron for a ~10 MB bundle vs ~150 MB, and because the Rust core walks the filesystem far faster than shelling out to `du`. The React app from the prototype is the webview front end largely unchanged.
- **Two commands** bridge UI ↔ core: `scan(root) → ScanResult` and `reclaim(ids, mode) → ReclaimResult`.
- **Safety gate lives in the core, not the UI.** The UI sends item IDs; the core independently re-derives each item's current risk class and path, and refuses anything Protected or outside the permitted root. This is the **Confidence Gateway** pattern: the deterministic core validates every probabilistic/UI-originated proposal.
- **MCP wrapper** exposes the same core as tools (`scan_disk`, `propose_reclaim`, `reclaim_space`) so Brhaspati can drive it. Agent proposals pass through the identical safety gate — the agent can never widen what's deletable.

### Staged autonomy (agent path)
Mirrors the Brhaspati rollout ladder: **shadow** (agent reports what it *would* reclaim) → **assisted** (agent proposes, human approves) → **auto-safe** (policy auto-reclaims Safe-class only, Review/Protected always require approval). Permanent deletion is never automated.

---

## 10. Data model (scan output schema)

```json
{
  "root": "/Users/<user>/Library",
  "scanned_at": "2026-06-23T10:00:00Z",
  "disk": { "total_bytes": 536870912000, "free_bytes": 94489280512 },
  "root_total_bytes": 169600741376,
  "items": [
    {
      "id": "docker",
      "name": "Docker Desktop",
      "path": "/Users/<user>/Library/Containers/com.docker.docker",
      "bytes": 31138512896,
      "risk": "safe",
      "detector": "orphaned-docker-desktop",
      "rationale": "No Docker Desktop app or process found; active context is OrbStack.",
      "reversible": false
    }
  ]
}
```

`ReclaimResult` returns `{ requested_ids, freed_bytes, moved_to_trash[], permanently_deleted[], skipped[] }` with a reason for each skip.

---

## 11. Safety, permissions & privacy

- **HOME boundary:** the core refuses to operate on any path outside the user's home directory. No `sudo`, ever.
- **Risk re-validation:** deletion targets are re-classified at execution time, not trusted from the request.
- **Reversible default:** move-to-Trash unless the user explicitly chooses permanent for a specific item.
- **macOS Full Disk Access (TCC):** some paths require the user to grant Full Disk Access. The app detects this and guides the user to System Settings rather than failing silently — it never attempts to bypass TCC.
- **Privacy:** no network access, no telemetry, no accounts. Scan results never leave the machine. This is a stated, testable guarantee (the shipped binary makes no outbound connections).

---

## 12. Non-functional requirements

| Area | Requirement |
|---|---|
| Performance | `~/Library` scan < 5 s on SSD; whole-`$HOME` scan < 20 s (v1.1). |
| Footprint | App bundle < 15 MB; idle memory < 80 MB. |
| Reliability | A failed delete on one item never aborts the batch; failures are reported, not swallowed. |
| Accessibility | Full keyboard navigation, visible focus, `prefers-reduced-motion` respected. |
| Trust | Zero false-deletion of Protected items is a hard requirement, not a target. |

---

## 13. Milestones

| Milestone | Status | Scope |
|---|---|---|
| **M0 — MVP** | ✅ Done | `~/Library` scan, core detectors (Docker, JetBrains, Electron caches, aerials, Xcode, container volumes), treemap UI, Trash-first reclaim + permanent opt-in, safety gate, CLI, macOS only. |
| **M1 — Depth** | Planned | Full detector catalogue, whole-`$HOME` and whole-disk scan, scan history + "what grew," permanent-delete path for large items. |
| **M2 — Agent** | Planned | MCP server, Brhaspati integration, staged autonomy (shadow → assisted → auto-safe), policy config. |
| **M3 — Reach** | Planned | Linux dev-box support, headless CI-runner cleanup mode (reuses the CLI/core). |

---

## 14. Success metrics

- **Median GB reclaimed per session** (primary value signal).
- **Time-to-first-insight** — scan completion latency.
- **Protected-data incidents = 0** (hard correctness bar).
- **Reclaim-without-regret** — proportion of reclaimed bytes *not* restored from Trash within 7 days (proxy for good risk classification).
- **Repeat use** — sessions per machine per month (hygiene becomes a habit).

---

## 15. Risks & mitigations

| Risk | Mitigation |
|---|---|
| Misclassifying real data as Safe | Conservative detectors; ambiguity resolves to Protected; Trash-first deletion gives a recovery window. |
| Detector drift as tools change (new Docker/IDE paths) | Declarative detector definitions, versioned; easy to add/adjust without touching core or UI. |
| User grants Full Disk Access reluctantly | Clear in-app rationale; degrade gracefully to the paths already accessible. |
| Agent over-reach via MCP | Identical safety gate for agent and UI; Safe-only automation; permanent delete never automated. |

---

## 16. Open questions

1. Default value of *N* for "keep newest JetBrains versions" — 1, or 2 for a fallback?
2. Should whole-disk scan be v1 or deferred to M1? (Trades first-run completeness against scan latency and TCC friction.)
3. Trash has practical size limits for very large items — should permanent be the *suggested* default specifically for items above a threshold (e.g. >10 GB), while staying opt-in?
4. Scheduled/background scans — useful, or does it push the tool toward the "cleaner" category users distrust?

---

## 17. Out of scope (explicit)

Duplicate detection, photo/media libraries, cloud-storage offloading, system-cache surgery requiring elevated privileges, and any feature requiring a network connection or account.
