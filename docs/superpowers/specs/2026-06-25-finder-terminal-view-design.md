# Finder + Terminal View, with Dry-Run Impact Assessment

**Date:** 2026-06-25
**Status:** Approved design — ready for implementation plan

## Problem

Reclaim's only graphical surface today is the Tauri treemap app. The `reclaim`
CLI exists but emits **raw JSON only** — there is no human-readable "view," and
no way to start an analysis from where developers actually keep their folders:
the Finder. Users also need to *assess* what a reclaim would do — how much space
it recovers and what it costs — **before** deleting anything.

This design adds two new read-only front-ends over the existing engine and a
truthful impact-assessment path. It changes **no detector and no gate logic**.

## Goals

1. Right-click a folder in Finder → see Reclaim's analysis in a Terminal.
2. A human-readable, color-coded terminal view (not raw JSON).
3. A truthful "what would deleting do" impact assessment, reusing the real
   safety gate so the preview can never diverge from an actual reclaim.

## Non-goals

- No interactive TUI (no input loop / in-place selection). View is static.
- No deletion from the new surfaces. Reclaim stays the existing explicit
  `reclaim safe` / `reclaim reclaim` commands.
- No Finder Sync Extension (no Swift/Xcode/app-bundle/signing).
- No change to the §10 scan JSON schema, the detectors, or the gate's logic.

## Invariants preserved

- **Backend decides what's deletable** (§1): the preview re-derives live risk and
  runs the same `safety::validate_against_home` gate; the view never decides.
- **Reversible by default** (§2): preview/scan delete nothing; suggested command
  is `reclaim safe` (Trash).
- **HOME boundary, no sudo** (§3): unchanged — gate still enforces it.
- **Wrappers carry no business logic**: the Finder Quick Action only hands a path
  to the CLI.
- **Local and silent** (§6): no network; all new code is local rendering + glue.

---

## Component 1 — CLI human view renderer

**Where:** `reclaim-core/src/bin/reclaim.rs` (rendering only; library untouched).

**Behavior**
- Header: `root`, total reclaimable = sum of `Safe` + `Review` bytes (Protected
  is inert / transparency-only, never counted), plus disk free context.
- Group `items` by `risk` in order **SAFE → REVIEW → PROTECTED**; within each
  group sort by `bytes` desc. Show top-N per group (default 10) with a
  `… +N more (X GB)` rollup so large cache lists stay readable.
- Each line: `<size>  <name>` (fall back to `path` if `name` empty). Color is the
  three risk tokens only — mint `#3FB950` / amber `#D29922` / steel `#6E7B8B`
  ("color is information"; no decorative color).
- Footer: suggested next command (`reclaim safe <root>` when any Safe exists);
  PROTECTED shown as `(inert)`.

**JSON vs. view selection (preserves the schema contract)**
- Default = **human table when stdout is a TTY**; **piped / redirected (non-TTY)
  → JSON**, so `reclaim scan | jq` and existing scripts keep working.
- `--json` forces JSON to a TTY; existing `--pretty` keeps pretty JSON.
- MCP and the Tauri UI link the *crate* directly (not this binary), so none of
  this affects them. The §10 JSON schema does not change.

**Implementation notes:** reuse the existing `human()` size formatter; emit raw
ANSI color, suppressed when non-TTY or `NO_COLOR` is set. No new crate.

### Example output

```
Reclaim — ~/Library/Caches   (4.2 GB reclaimable)

SAFE        3.1 GB
  2.0 GB  com.apple.Safari/...
  1.1 GB  Homebrew/downloads
REVIEW      900 MB
  900 MB  JetBrains/IntelliJ
PROTECTED   220 MB  (inert)

Run: reclaim safe ~/Library/Caches
```

---

## Component 2 — Impact assessment (truthful dry-run)

**Key insight:** `reclaim::reclaim_with(authoritative, targets, home, exec)`
already takes a pluggable `Executor`. A dry run is the **same code path** with an
executor that records instead of deletes — so the preview is guaranteed
consistent with a real reclaim.

**Core addition (`reclaim-core/src/reclaim.rs`):**
- `struct NoopExecutor;` implementing `Executor` — `to_trash` / `permanent`
  record nothing to the filesystem and return `Ok(())`.
- `pub fn dry_run(root: &Path, targets: &[ReclaimTarget]) -> io::Result<ReclaimResult>`
  = re-scan, then `reclaim_with(&items, targets, &safety::home_dir(), &NoopExecutor)`.
- Returned `ReclaimResult` is reused as-is: `moved_to_trash` / `permanently_deleted`
  mean "**would** be"; `skipped` carries real `Denial` reasons (Protected,
  OutsideHome, Missing); `freed_bytes` is the projected recovery.

**Required test (CLAUDE.md gate rule):** a dry run over a Protected item reports it
skipped-as-Protected, `deleted_nothing()` is true, and the NoopExecutor is never
asked to delete it (e.g. a spy variant counting calls).

**Two display layers**
1. **Per-item cost** (already in scan data): show `rationale`
   (e.g. "regenerates on next build", "re-download required") and a marker from
   the `reversible` flag — `↩ reversible` vs `⚠ permanent-only`.
2. **Aggregate projection** (dry_run + `DiskInfo`):
   ```
   Reclaiming SAFE: +3.1 GB  →  free 18.0 GB → 21.1 GB  (40% → 47% of disk)
   12 items would move to Trash · 2 skipped (Protected, inert)
   REVIEW (opt-in): +900 MB more if selected
   ```
   free-after = `disk.free_bytes + freed_bytes`; Protected never counted as
   recoverable.

**Surfaces**
- `reclaim scan <root>` (the view): shows layers 1 + 2. Read-only.
- `reclaim preview <root> [--include-review]`: runs `dry_run` for the Safe set
  (plus Review when `--include-review`) and prints the would-trash / would-skip
  (+ why) breakdown — the literal impact assessment before committing.
- Actual deletion stays the separate explicit `reclaim safe` / `reclaim reclaim`.

---

## Component 3 — Finder Quick Action (entry point)

**What:** an Automator Quick Action `Analyze with Reclaim.workflow` that receives
**folders in Finder** and appears under right-click → **Quick Actions**. No Xcode,
signing, or app bundle.

**Launcher** (workflow "Run Shell Script", input *as arguments*):
```sh
for f in "$@"; do
  osascript -e "tell application \"Terminal\" to do script \
    \"$RECLAIM_BIN scan '$f'; echo; echo '[reclaim preview]'; $RECLAIM_BIN preview '$f'\""
done
```
Opens Terminal (so the colored view + projection stay on screen) and runs the
**read-only** `scan` + `preview`. It never deletes; the suggested `reclaim safe …`
is printed for the user to run deliberately. `$RECLAIM_BIN` resolves to the
installed `reclaim` binary.

**Shipping (versioned, not hand-built):**
- `.workflow` source lives in the repo under `finder/`.
- `scripts/install-finder-quick-action.sh` generates the bundle
  (`Contents/document.wflow` plist + `Info.plist`) into `~/Library/Services/` and
  points it at the release binary. Surfaced via a README step / `make` target.

**Boundary:** pure glue — only hands a path to the CLI. All risk logic and the
gate stay in the core (wrapper-has-no-business-logic invariant).

---

## Testing

- **Core:** `dry_run` over a Protected item → skipped-as-Protected,
  `deleted_nothing()`, NoopExecutor never asked to delete (spy). `dry_run`
  `freed_bytes` equals sum of would-trash item bytes.
- **CLI rendering:** unit-test the renderer over a fixed `ScanResult` →
  grouping order, top-N rollup, reclaimable total excludes Protected, projection
  math (`free + freed`).
- **TTY routing:** piped output is valid JSON (schema contract); explicit
  `--json` forces JSON.
- **Finder:** the generator produces a loadable `.workflow`; launcher invokes the
  binary with the selected path (smoke-level; the heavy logic is core-tested).

## Components & boundaries

| Unit | Does | Depends on | Public interface |
|---|---|---|---|
| view renderer | `ScanResult` → colored table + projection | `scan`, `dry_run`, `DiskInfo` | stdout text |
| `dry_run` / `NoopExecutor` | run gate without deleting | `reclaim_with`, `safety` | `fn dry_run(root, targets) -> ReclaimResult` |
| `preview` subcommand | print would-trash/would-skip | `dry_run`, renderer | CLI arg parse |
| Quick Action + installer | Finder → Terminal launcher | the `reclaim` binary | `.workflow` + install script |
