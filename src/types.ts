// Mirrors the reclaim-core JSON contract (Prd.md §10). Keep in sync with the
// Rust types in reclaim-core/src/lib.rs — this is a shared schema, not a guess.

export type Risk = "safe" | "review" | "protected";

export interface Item {
  id: string;
  name: string;
  path: string;
  bytes: number;
  risk: Risk;
  detector: string;
  rationale: string;
  reversible: boolean;
}

export interface DiskInfo {
  total_bytes: number;
  free_bytes: number;
}

export interface ScanResult {
  root: string;
  scanned_at: string;
  disk: DiskInfo;
  root_total_bytes: number;
  items: Item[];
}

export type Mode = "trash" | "permanent";

export interface ReclaimTarget {
  id: string;
  mode: Mode;
}

export interface Disposition {
  id: string;
  path: string;
  bytes: number;
}

export interface Skip {
  id: string;
  reason: string;
}

export interface ReclaimResult {
  requested_ids: string[];
  freed_bytes: number;
  moved_to_trash: Disposition[];
  permanently_deleted: Disposition[];
  skipped: Skip[];
}

// ---- presentation helpers ----------------------------------------------------

export const RISK_ORDER: Record<Risk, number> = { safe: 0, review: 1, protected: 2 };

export const RISK_LABEL: Record<Risk, string> = {
  safe: "Safe",
  review: "Review",
  protected: "Protected",
};

export function isSelectable(item: Item): boolean {
  return item.risk !== "protected";
}

/** Binary byte formatting — sizes are always shown in the developer's units. */
export function formatBytes(bytes: number, digits = 1): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB", "PB"];
  let v = bytes / 1024;
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u += 1;
  }
  return `${v.toFixed(digits)} ${units[u]}`;
}
