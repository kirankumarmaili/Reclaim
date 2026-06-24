import { useCallback, useEffect, useMemo, useState } from "react";
import { isDesktop, reclaim as apiReclaim, scan as apiScan } from "./api";
import { CapacityGauge } from "./components/CapacityGauge";
import { ReclaimPanel } from "./components/ReclaimPanel";
import { Treemap } from "./components/Treemap";
import type { Item, ReclaimResult, ScanResult } from "./types";
import { formatBytes, isSelectable, RISK_LABEL } from "./types";

type Hover = { item: Item; x: number; y: number } | null;

export default function App() {
  const [scan, setScan] = useState<ScanResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [permanent, setPermanent] = useState(false);
  const [draining, setDraining] = useState<Set<string>>(new Set());
  const [hover, setHover] = useState<Hover>(null);
  const [busy, setBusy] = useState(false);
  const [receipt, setReceipt] = useState<ReclaimResult | null>(null);

  const runScan = useCallback(async () => {
    setLoading(true);
    setSelected(new Set());
    setReceipt(null);
    try {
      setScan(await apiScan());
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    runScan();
  }, [runScan]);

  // Auto-dismiss the reclaim receipt after it has been read.
  useEffect(() => {
    if (!receipt) return;
    const t = setTimeout(() => setReceipt(null), 6000);
    return () => clearTimeout(t);
  }, [receipt]);

  const items = scan?.items ?? [];
  const selectedItems = useMemo(
    () => items.filter((i) => selected.has(i.id)),
    [items, selected],
  );
  const selectedBytes = selectedItems.reduce((s, i) => s + i.bytes, 0);
  const safeItems = items.filter((i) => i.risk === "safe");

  const toggle = useCallback((item: Item) => {
    if (!isSelectable(item)) return; // the gate is server-side too, but don't even offer it
    setSelected((prev) => {
      const next = new Set(prev);
      next.has(item.id) ? next.delete(item.id) : next.add(item.id);
      return next;
    });
  }, []);

  const selectSafe = useCallback(() => {
    // "Select safe" bulk-selects Safe only; Review is never bulk-selected (FR-11).
    setSelected(new Set(safeItems.map((i) => i.id)));
  }, [safeItems]);

  const onHover = useCallback((item: Item | null, e?: React.MouseEvent) => {
    if (!item || !e) return setHover(null);
    setHover({ item, x: e.clientX, y: e.clientY });
  }, []);

  const doReclaim = useCallback(async () => {
    if (selectedItems.length === 0 || busy) return;
    const ids = selectedItems.map((i) => i.id);
    setBusy(true);
    // Animate the blocks draining before the data updates.
    setDraining(new Set(ids));
    const mode = permanent ? "permanent" : "trash";
    const targets = ids.map((id) => ({ id, mode } as const));

    const result = await apiReclaim(targets);

    // Let the drain animation breathe, then reconcile against what the core
    // reports actually happened (only removed ids leave the model).
    const removed = new Set([
      ...result.moved_to_trash.map((d) => d.id),
      ...result.permanently_deleted.map((d) => d.id),
    ]);
    setTimeout(() => {
      setScan((prev) =>
        prev
          ? {
              ...prev,
              disk: { ...prev.disk, free_bytes: prev.disk.free_bytes + result.freed_bytes },
              items: prev.items.filter((i) => !removed.has(i.id)),
            }
          : prev,
      );
      setSelected(new Set());
      setDraining(new Set());
      setReceipt(result);
      setBusy(false);
    }, 480);
  }, [selectedItems, permanent, busy]);

  return (
    <div className="app">
      <header className="header">
        <span className="wordmark">RECLAIM<span className="dot">.</span></span>
        <span className="root mono">{scan?.root ?? "~/Library"}</span>
        <span className="spacer" />
        <span className="status">
          {loading
            ? "scanning…"
            : isDesktop()
              ? `${items.length} items`
              : `${items.length} items · demo data`}
        </span>
        <button className="btn ghost" onClick={runScan} disabled={loading || busy}>
          ⟳ Rescan
        </button>
      </header>

      <CapacityGauge
        totalBytes={scan?.disk.total_bytes ?? 0}
        freeBytes={scan?.disk.free_bytes ?? 0}
        selectedBytes={selectedBytes}
      />

      <main className="main">
        <div style={{ position: "relative", margin: 0, display: "contents" }}>
          {loading ? (
            <div className="treemap-wrap">
              <div className="center"><span className="pulse">reading filesystem…</span></div>
            </div>
          ) : items.length === 0 ? (
            <div className="treemap-wrap">
              <div className="center">Nothing reclaimable found. Your disk is tidy.</div>
            </div>
          ) : (
            <Treemap
              items={items}
              selected={selected}
              draining={draining}
              hasSelection={selected.size > 0}
              onToggle={toggle}
              onHover={onHover}
            />
          )}
        </div>

        <ReclaimPanel
          items={items}
          selected={selected}
          permanent={permanent}
          onToggle={toggle}
          onSelectSafe={selectSafe}
          safeCount={safeItems.length}
        />
      </main>

      <footer className="actions">
        <span className="summary">
          {selectedItems.length > 0 ? (
            <>
              <strong>{selectedItems.length}</strong> selected ·{" "}
              <strong>{formatBytes(selectedBytes)}</strong> to free
            </>
          ) : (
            "Select Safe items, or pick individual Review items to reclaim."
          )}
        </span>
        <span className="spacer" />
        <label className="perm-toggle" title="Permanent deletion skips the Trash and cannot be undone.">
          <input
            type="checkbox"
            checked={permanent}
            onChange={(e) => setPermanent(e.target.checked)}
          />
          Delete permanently
        </label>
        <button
          className={`btn primary ${permanent ? "danger" : ""}`}
          onClick={doReclaim}
          disabled={selectedItems.length === 0 || busy}
        >
          {busy
            ? "Reclaiming…"
            : permanent
              ? `Delete ${formatBytes(selectedBytes)} permanently`
              : `Reclaim ${formatBytes(selectedBytes)} → Trash`}
        </button>
      </footer>

      {hover && (
        <div
          className="tooltip"
          style={{
            left: Math.min(hover.x + 14, window.innerWidth - 340),
            top: Math.min(hover.y + 14, window.innerHeight - 160),
          }}
        >
          <div className="tt-name">{hover.item.name}</div>
          <div className="tt-meta">
            <span className="mono">{formatBytes(hover.item.bytes)}</span>
            <span className={`tt-badge ${hover.item.risk}`}>{RISK_LABEL[hover.item.risk]}</span>
          </div>
          <div className="tt-why">{hover.item.rationale}</div>
          <div className="tt-path mono">{hover.item.path}</div>
        </div>
      )}

      {receipt && (
        <div className="toast" role="status">
          Freed <span className="freed">{formatBytes(receipt.freed_bytes)}</span>
          {receipt.moved_to_trash.length > 0 && ` · ${receipt.moved_to_trash.length} to Trash`}
          {receipt.permanently_deleted.length > 0 && ` · ${receipt.permanently_deleted.length} deleted`}
          {receipt.skipped.length > 0 && (
            <span className="skips"> · {receipt.skipped.length} skipped</span>
          )}
        </div>
      )}
    </div>
  );
}
