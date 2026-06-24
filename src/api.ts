// The bridge to reclaim-core. In the Tauri webview, scan/reclaim call the Rust
// commands over IPC. In a plain browser (`pnpm dev`), there is no backend, so we
// fall back to bundled mock data — the UI is fully explorable without building
// the desktop app. The mock NEVER deletes anything.

import { mockScan } from "./mock";
import type { ReclaimResult, ReclaimTarget, ScanResult } from "./types";

// Tauri v2 injects this when `app.withGlobalTauri = true` (see tauri.conf.json).
interface TauriGlobal {
  core: { invoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T> };
}
function tauri(): TauriGlobal | null {
  const w = window as unknown as { __TAURI__?: TauriGlobal };
  return w.__TAURI__ ?? null;
}

export const isDesktop = (): boolean => tauri() !== null;

export async function scan(root?: string): Promise<ScanResult> {
  const t = tauri();
  if (t) return t.core.invoke<ScanResult>("scan", { root: root ?? null });
  // Browser dev: simulate a touch of latency so loading states are visible.
  await new Promise((r) => setTimeout(r, 350));
  return mockScan();
}

export async function reclaim(targets: ReclaimTarget[]): Promise<ReclaimResult> {
  const t = tauri();
  if (t) return t.core.invoke<ReclaimResult>("reclaim", { targets });
  // Browser dev: report success without touching disk.
  await new Promise((r) => setTimeout(r, 600));
  const freed = mockScan().items.filter((i) => targets.some((x) => x.id === i.id));
  return {
    requested_ids: targets.map((x) => x.id),
    freed_bytes: freed.reduce((s, i) => s + i.bytes, 0),
    moved_to_trash: targets
      .filter((x) => x.mode === "trash")
      .map((x) => freed.find((i) => i.id === x.id)!)
      .filter(Boolean)
      .map((i) => ({ id: i.id, path: i.path, bytes: i.bytes })),
    permanently_deleted: targets
      .filter((x) => x.mode === "permanent")
      .map((x) => freed.find((i) => i.id === x.id)!)
      .filter(Boolean)
      .map((i) => ({ id: i.id, path: i.path, bytes: i.bytes })),
    skipped: [],
  };
}
