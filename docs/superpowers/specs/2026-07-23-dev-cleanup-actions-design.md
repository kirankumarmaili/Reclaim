# Dev cleanup actions: npm, Gradle, Docker prune, git worktree prune

**Status:** Draft — to revisit before implementation.
**Date:** 2026-07-23
**Depends on:** `Prd.md` (§5 principles, §6 risk model, §10 schema, §11 safety).

## Summary

Extend Reclaim to reclaim four new classes of developer disk cruft:

1. **npm / yarn / pnpm caches** — user-global cache directories.
2. **Gradle caches** — `~/.gradle` build caches, wrapper dists, daemon logs.
3. **Docker reclaimable data** — build cache, dangling/unused images, stopped
   containers (and volumes surfaced as **Protected**, never deletable).
4. **git worktree prune** — remove stale worktree administrative entries in
   discovered repos under the scan root.

The first two fit today's model exactly: they are paths you move to Trash. The
last two do not — they run a tool (`docker`, `git`) instead of deleting a path.
The heart of this spec is the **minimal core change that lets a tool-invoking
action pass through the same safety gate** without weakening any invariant.

## Motivation

Reclaim already classifies caches by risk and gates every delete. Developer
machines accumulate their heaviest cruft in exactly these four places. Measured
on the author's machine, `docker system df` alone reported **1.29 GB** of
reclaimable images and **3.39 GB** of volumes; Gradle and npm caches are
routinely multi-GB. These are the highest-value targets not yet covered.

## Non-goals

- Walking all of `$HOME` to discover repos. Discovery is scoped to the caller's
  scan root (see "Project discovery").
- Per-project `node_modules/` and `build/` deletion. Deferred; this spec covers
  the four named cleanups only.
- Docker image *compaction* / `Docker.raw` shrinking. Out of scope.
- Any settings file or configured "dev roots" list. Discovery uses the existing
  scan root, no new config.
- Running arbitrary caller-supplied commands. The core only ever runs
  detector-authored, allowlisted argv.

## Design principles preserved

Every core invariant (`CLAUDE.md` "Non-negotiable invariants", PRD §5/§11) must
still hold. The two that this feature stresses, and how they are kept:

- **#1 Backend decides, never the UI.** The request still sends only an item
  `id`. For tool actions, the core re-derives the exact argv from the detector —
  the caller can no more forge a command than it can forge a risk class today.
- **#2 Reversible by default.** Tool actions are irreversible (you cannot Trash a
  `docker system prune`). They are therefore treated like today's
  `Mode::Permanent`: **opt-in per item, never auto-selected, never part of
  "Select safe."** The default flow stays Trash-first.
- **#3 HOME boundary, no sudo.** `git worktree prune` runs with `cwd` set to a
  repo that must canonicalize inside `$HOME`. `docker` has no path; it is
  constrained instead by a fixed argv allowlist. No path escapes home; nothing
  escalates privilege.
- **#4 Ambiguity resolves to Protected.** Docker volumes = real data =
  Protected. Stopped containers and unused tagged images = Review (re-pull/state
  cost). Only build cache and dangling images = Safe.
- **#5 A failed action never aborts the batch.** A failed `docker`/`git`
  invocation is captured as a `Skip`, exactly like a failed delete today.
- **#6 Local and silent.** `docker`/`git` run locally against the local daemon /
  local repo. No network call is made by Reclaim itself. (`docker system prune`
  does not pull; unused-image removal only deletes.)

## Core model change: two kinds of item

Today an `Item` *is a path you delete*. We add one field describing **what
reclaiming the item does**:

```rust
/// What "reclaiming" this item performs. Defaults to DeletePath so every
/// existing detector and the §10 schema stay backward-compatible.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    /// Today's behavior: move the item's `path` to Trash, or permanently delete.
    DeletePath,
    /// Run a fixed, detector-authored command. Not reversible.
    RunTool {
        /// The exact argv the core will execute. Built by the detector, never
        /// taken from the request. argv[0] is validated against an allowlist.
        argv: Vec<String>,
        /// Working directory for the command. For git worktree prune this is the
        /// repo; it must canonicalize inside $HOME. `None` for daemon tools
        /// (docker) that take no cwd-relative path.
        cwd: Option<PathBuf>,
    },
}
```

`Item` gains `#[serde(default)] pub action: Action` where `Default` is
`DeletePath`. Existing detectors are untouched; the field is additive and the
§10 JSON schema gains an optional `action` object (schema version bump, see
"Schema").

### Why an enum on `Item`, not a parallel type

The scan output, the treemap, the risk classes, the side panel, and the MCP
tools are all built around a flat list of `Item`. A parallel "action" list would
fork every consumer. One `Item` list with a discriminated `action` keeps the
schema, the UI selection model, and the safety gate single-source. The UI shows
a tool item the same way — name, bytes, risk color, rationale — and reclaim
still just echoes `id`s back.

## Safety gate change

`validate_against_home` gets a second branch, keyed on `item.action`. **Check 1
(risk re-derived, Protected refused) is unchanged and runs first for every item,
both kinds** — the Protected refusal is never bypassed.

```
validate(item):
  if item.risk == Protected            -> Denial::Protected      # unchanged, both kinds
  match item.action:
    DeletePath:                          # exactly today's checks 2 & 3
      path must exist                  -> else Denial::Missing
      canonical(path) within $HOME     -> else Denial::OutsideHome / Unresolvable
    RunTool { argv, cwd }:
      argv[0] in ALLOWLIST             -> else Denial::DisallowedCommand   # new
      every argv element non-empty, no shell metachars                    # new
      if cwd.is_some():
        canonical(cwd) within $HOME    -> else Denial::OutsideHome
```

- `ALLOWLIST` is a fixed set: `{ "docker", "git" }` with a **fixed permitted
  subcommand shape per binary** (`docker system prune …`, `docker image prune
  …`, `docker container prune …`; `git worktree prune …`). The allowlist lives
  in `safety.rs`, not in the detector, so the gate — not the detector — is the
  authority on what may run.
- New `Denial::DisallowedCommand` variant with a reason string.
- The argv is validated for emptiness and shell metacharacters even though we
  never pass it through a shell (`Command::new(argv[0]).args(&argv[1..])`,
  no `sh -c`). Defense in depth.

### Executor change

The `Executor` trait gains one method so tool execution is testable without
touching the real daemon/repo, mirroring the existing `to_trash`/`permanent`
split:

```rust
trait Executor {
    fn to_trash(&self, path: &Path) -> io::Result<()>;       // unchanged
    fn permanent(&self, path: &Path) -> io::Result<()>;      // unchanged
    fn run_tool(&self, argv: &[String], cwd: Option<&Path>) -> io::Result<()>;  // new
}
```

`SystemExecutor::run_tool` runs `Command`, returns `Err` on non-zero exit (which
becomes a `Skip`, per invariant #5). The test `SpyExecutor` records the argv, so
gate tests can assert "a Protected/ disallowed action never reaches the
executor" the same way path tests do today.

### reclaim dispatch

In `reclaim_with`, after the gate passes, dispatch on `item.action`:

- `DeletePath` → `to_trash` / `permanent` by `target.mode` (today's code).
- `RunTool` → `run_tool(argv, cwd)`. **`target.mode` is ignored for tool items;
  they are always irreversible.** The `freed_bytes` credited is the item's
  scanned `bytes` (best estimate; see "Docker byte accounting").

## The four detectors

All four live in `reclaim-core/src/detectors/`, register in `catalogue()`, and
are filtered by scan root exactly like existing detectors.

### 1. npm / yarn / pnpm (`node_caches.rs`) — DeletePath

| Item | Path | Risk | Why |
|---|---|---|---|
| npm cache | `~/.npm/_cacache` | **Safe** | Pure content-addressable cache; npm refetches. |
| yarn cache | `~/Library/Caches/Yarn` or `~/.cache/yarn` | **Safe** | Regenerates on next install. |
| pnpm store | `~/.local/share/pnpm/store` (or `pnpm store path`) | **Review** | Global content-addressable store shared by all projects; deleting forces re-download of everything. Reclaimable but costly. |

Only `~/.npm/_cacache` is deleted, never `~/.npm` (which holds config).

### 2. Gradle (`gradle.rs`) — DeletePath

| Item | Path | Risk | Why |
|---|---|---|---|
| Gradle build cache | `~/.gradle/caches/build-cache-1` | **Safe** | Rebuildable output cache. |
| Gradle wrapper dists | `~/.gradle/wrapper/dists` | **Review** | Downloaded Gradle distributions; re-downloaded on next build (network cost). |
| Gradle daemon logs | `~/.gradle/daemon` | **Safe** | Logs/registry; regenerated. |

Never touches `~/.gradle/caches/modules-2` metadata beyond the build cache, and
never `~/.gradle/gradle.properties` (config, Protected/ignored).

### 3. Docker (`docker_prune.rs`) — RunTool + Protected transparency

Extends beyond the existing orphaned-Docker-Desktop detector (which stays as-is
for the whole-VM-bundle case). This new detector reads `docker system df
--format json` **only when a Docker daemon is reachable** and produces one item
per reclaimable category:

| Item | Action | Risk | Command |
|---|---|---|---|
| Docker build cache | RunTool | **Safe** | `docker builder prune -f` |
| Dangling images | RunTool | **Safe** | `docker image prune -f` |
| Unused (tagged) images | RunTool | **Review** | `docker image prune -a -f` — re-pull cost |
| Stopped containers | RunTool | **Review** | `docker container prune -f` — may hold state |
| Local volumes | *(none)* | **Protected** | Listed for transparency at their real size; **no prune action ever attached.** This is real data the product exists to protect. |

- `bytes` per item comes from the `Reclaimable` field of `docker system df`.
- If no daemon is reachable, the detector emits nothing (not an error).
- `cwd: None` for all Docker items (daemon tool, no path).
- Volumes appear as a Protected item so the user *sees* the 3.39 GB and
  understands Reclaim will not delete it — consistent with PRD transparency.

### 4. git worktree (`git_worktrees.rs`) — RunTool

Runs only when the scan root contains git repositories (bounded discovery).

- **Discovery:** walk the scan root to a shallow bounded depth looking for `.git`
  directories, collecting repos. This is the only new walk; it is bounded and
  only runs when the root is a dev directory the user pointed at (pointing at
  `~/Library` finds none, preserving the <5 s budget, FR-4).
- For each repo with **stale/prunable worktree entries** (detected via `git
  worktree list --porcelain` showing `prunable`), emit one item:

| Item | Action | Risk | Command | cwd |
|---|---|---|---|---|
| Stale worktrees in `<repo>` | RunTool | **Safe** | `git worktree prune -v` | the repo |

- `bytes` for this item is the size of the stale `.git/worktrees/<name>`
  administrative directories (small — kilobytes to low MB). **Honesty note in
  rationale:** git worktree prune reclaims the *admin entries*, not the
  worktree working directories themselves; those are separate and not deleted by
  this action. The rationale states this so the user isn't surprised by the
  small byte figure.

## Project discovery

Chosen approach: **use the existing scan root.** No new config, no new settings
UI, no always-on walk.

- `scan ~/Library` → global caches (npm/gradle/docker) light up; git worktree
  detector finds no repos. Identical cost to today.
- `scan ~/Documents/GitHub` → git worktree detector finds repos under that root;
  global-cache detectors still emit their fixed paths but are filtered out by
  `run_all`'s `starts_with(root)` unless the root contains them.

This keeps the <5 s budget: the user opts into repo discovery by choosing where
to point the scan.

## Schema (PRD §10 contract)

The scan-output JSON is a versioned contract shared by UI and MCP. Changes:

- `Item` gains an optional `action` object. Absent ⇒ `{ "kind": "delete_path" }`
  (the default), so existing consumers that ignore it still work for path items.
- Tool items serialize as
  `"action": { "kind": "run_tool", "argv": [...], "cwd": "…"|null }`.
- **Bump the schema version** and note it in the README/`Prd.md` §10 changelog.
  MCP `reclaim_space` and the UI must both learn that an item may be a tool
  action (irreversible, `mode` ignored, never in "Select safe").

## MCP surface

No new tools. The existing `scan_disk` / `propose_reclaim` / `reclaim_space`
already operate on `id`s and pass through the gate. Changes:

- `propose_reclaim`'s "Select safe" set continues to include only
  `auto_selectable` (Safe **and** `DeletePath`) items — **tool actions are
  excluded from auto-selection even when Safe**, because they are irreversible.
  They can be reclaimed only by explicit id.
- The staged-autonomy policy gate treats `RunTool` items as requiring at least
  the same confirmation stage as `Permanent` deletes.

## Testing (hard bar: zero false-deletion of Protected)

Per `CLAUDE.md` "When changing safety-critical code", the safety change must be
proven by tests, not reviewer attention:

1. **Protected tool item is refused** — a `RunTool` item classified Protected
   never reaches `run_tool` (Spy asserts zero invocations). Mirrors the existing
   `trash_is_the_default_and_protected_is_refused`.
2. **Docker volumes never get an action** — detector test: volume items have
   `action == DeletePath`? No — they are Protected *and* carry no prune command;
   assert no volume item is ever `RunTool`.
3. **Disallowed command is refused** — a forged/hypothetical `RunTool` with
   `argv[0] = "rm"` (or any non-allowlisted binary) → `Denial::DisallowedCommand`,
   never executed.
4. **git worktree cwd outside home is refused** — a `RunTool` with a `cwd` that
   canonicalizes outside `$HOME` → `Denial::OutsideHome`.
5. **Tool actions are never auto-selected** — "Select safe" over a set including
   a Safe `RunTool` item excludes it.
6. **Failed tool run is a Skip, not fatal** — a `run_tool` returning `Err` with
   another Safe item in the batch: the other item still succeeds (invariant #5).
7. **`mode` is ignored for tool items** — a `RunTool` target sent with
   `Mode::Trash` still executes the tool (never attempts Trash) and is reported
   as irreversible.

## Docker byte accounting

Bytes are `u64`, never floats (project convention). `docker system df --format
json` returns human strings ("1.294GB") and exact `Reclaimable` byte-ish values
depending on format; use `docker system df --format '{{json .}}'` per type and
parse the byte counts, falling back to `0` (item still listed) if parsing fails.
The `freed_bytes` credited after a successful prune is the scanned estimate — an
estimate is acceptable here because the exact freed amount is only knowable from
a post-prune `df`, and invariant accuracy (never over-crediting a Protected
item) is unaffected.

## Open questions to revisit

1. **Docker unused-tagged-images as Review vs. split** — should `docker image
   prune -a` be one Review item, or split "dangling" (Safe) from "unused tagged"
   (Review)? Current design splits them into two items with different commands.
   Confirm the two-command split is worth the extra `df` parsing.
2. **git worktree discovery depth** — what bounded depth is right for `.git`
   discovery under a dev root, and do we cap the number of repos to preserve the
   scan budget?
3. **Byte estimate honesty in UI** — how prominently should the UI mark tool
   items as "estimated, irreversible" so the drain animation / freed total
   doesn't imply the same precision as a path delete?
4. **pnpm store risk** — Safe or Review? Drafted as Review (shared global store);
   confirm.
