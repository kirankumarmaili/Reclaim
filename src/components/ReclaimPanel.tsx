import type { Item, Risk } from "../types";
import { formatBytes, isSelectable, RISK_LABEL } from "../types";

interface Props {
  items: Item[];
  selected: Set<string>;
  permanent: boolean;
  onToggle: (item: Item) => void;
  onSelectSafe: () => void;
  safeCount: number;
}

const GROUP_ORDER: Risk[] = ["safe", "review", "protected"];

/** Reclaimable items grouped by risk, each with checkbox, size, and rationale
 *  (PRD FR-10). Protected rows are shown locked for transparency. */
export function ReclaimPanel({ items, selected, permanent, onToggle, onSelectSafe, safeCount }: Props) {
  const groups = GROUP_ORDER.map((risk) => ({
    risk,
    rows: items.filter((i) => i.risk === risk),
  })).filter((g) => g.rows.length > 0);

  return (
    <aside className="panel" aria-label="Reclaimable items">
      <div className="panel-head">
        <div className="title">Reclaimable</div>
        <button
          className="btn select-safe"
          onClick={onSelectSafe}
          disabled={safeCount === 0}
          title="Select every Safe-class item. Review items are never bulk-selected."
        >
          Select safe ({safeCount})
        </button>
      </div>

      <div className="groups">
        {groups.map((g) => {
          const groupBytes = g.rows.reduce((s, i) => s + i.bytes, 0);
          return (
            <div key={g.risk}>
              <div className="group-head">
                <span className={`rdot ${g.risk}`} />
                {RISK_LABEL[g.risk]}
                <span className="gcount">{g.rows.length}</span>
                {g.risk === "protected" && <span className="gcount">· locked</span>}
                <span className="gsize mono">{formatBytes(groupBytes)}</span>
              </div>

              {g.rows.map((item) => {
                const on = selected.has(item.id);
                const locked = !isSelectable(item);
                return (
                  <div
                    key={item.id}
                    className={`row ${on ? "on" : ""} ${locked ? "locked" : ""}`}
                    onClick={() => !locked && onToggle(item)}
                    role={locked ? undefined : "checkbox"}
                    aria-checked={locked ? undefined : on}
                    aria-disabled={locked || undefined}
                    tabIndex={locked ? -1 : 0}
                    onKeyDown={(e) => {
                      if (!locked && (e.key === "Enter" || e.key === " ")) {
                        e.preventDefault();
                        onToggle(item);
                      }
                    }}
                  >
                    <span className="check">{on ? "✓" : locked ? "" : ""}</span>
                    <span>
                      <span className="r-name">{item.name}</span>
                      <span className="r-detector mono">{item.detector}</span>
                      <span className="r-why">{item.rationale}</span>
                    </span>
                    <span className="r-size">
                      {formatBytes(item.bytes)}
                      {on && permanent && <span className="perm">permanent</span>}
                    </span>
                  </div>
                );
              })}
            </div>
          );
        })}
      </div>
    </aside>
  );
}
