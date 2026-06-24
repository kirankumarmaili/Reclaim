import { formatBytes } from "../types";

interface Props {
  totalBytes: number;
  freeBytes: number;
  /** Bytes about to be reclaimed by the current selection. */
  selectedBytes: number;
}

/** The signature element: a disk gauge whose mint band grows as you select,
 *  with the projected free space called out. Used | Reclaimable | Free. */
export function CapacityGauge({ totalBytes, freeBytes, selectedBytes }: Props) {
  const total = Math.max(totalBytes, 1);
  const usedBytes = Math.max(total - freeBytes, 0);
  // The reclaim band is carved out of the used portion.
  const reclaimBytes = Math.min(selectedBytes, usedBytes);
  const settledUsed = usedBytes - reclaimBytes;

  const pct = (b: number) => `${(b / total) * 100}%`;
  const projectedFree = freeBytes + reclaimBytes;
  const hasSelection = selectedBytes > 0;

  return (
    <section className="gauge" aria-label="Disk capacity">
      <div className="gauge-head">
        <span className="label">Capacity</span>
        <span className="free mono">
          {formatBytes(freeBytes, 1).split(" ")[0]}
          <span className="unit"> {formatBytes(freeBytes, 1).split(" ")[1]} free</span>
        </span>
        <span className={`projected mono ${hasSelection ? "" : "idle"}`}>
          {hasSelection
            ? `→ ${formatBytes(projectedFree)} after reclaim`
            : `of ${formatBytes(total)}`}
        </span>
      </div>

      <div
        className="bar"
        role="meter"
        aria-valuemin={0}
        aria-valuemax={total}
        aria-valuenow={projectedFree}
        aria-label={`${formatBytes(projectedFree)} free after reclaiming ${formatBytes(selectedBytes)}`}
      >
        <div className="seg used" style={{ width: pct(settledUsed) }} />
        <div className="seg reclaim" style={{ width: pct(reclaimBytes) }} />
        <div className="seg free" style={{ width: pct(projectedFree) }} />
        {/* tick at the original free boundary, so growth past it is legible */}
        <div className="tick" style={{ left: pct(usedBytes) }} />
      </div>

      <div className="gauge-legend">
        <span><span className="swatch" style={{ background: "#252c36" }} />Used {formatBytes(settledUsed)}</span>
        {hasSelection && (
          <span><span className="swatch" style={{ background: "var(--safe)" }} />Reclaiming {formatBytes(reclaimBytes)}</span>
        )}
        <span><span className="swatch" style={{ background: "var(--line)", border: "1px solid var(--line-strong)" }} />Free {formatBytes(projectedFree)}</span>
      </div>
    </section>
  );
}
