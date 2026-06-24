import { useLayoutEffect, useRef, useState } from "react";
import { squarify } from "../squarify";
import type { Item } from "../types";
import { formatBytes, isSelectable } from "../types";

interface Props {
  items: Item[];
  selected: Set<string>;
  draining: Set<string>;
  hasSelection: boolean;
  onToggle: (item: Item) => void;
  onHover: (item: Item | null, e?: React.MouseEvent) => void;
}

/** Squarified treemap. Block area ∝ bytes, color = risk class (PRD FR-6).
 *  Protected blocks are inert — striped and non-interactive. */
export function Treemap({ items, selected, draining, hasSelection, onToggle, onHover }: Props) {
  const wrap = useRef<HTMLDivElement>(null);
  const [box, setBox] = useState({ w: 0, h: 0 });

  useLayoutEffect(() => {
    const el = wrap.current;
    if (!el) return;
    const ro = new ResizeObserver(([entry]) => {
      const { width, height } = entry.contentRect;
      setBox({ w: width, h: height });
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const tiles =
    box.w > 0 && items.length > 0
      ? squarify(
          items.map((it) => ({ value: it.bytes, datum: it })),
          { x: 0, y: 0, w: box.w, h: box.h },
        )
      : [];

  return (
    <div className="treemap-wrap" ref={wrap}>
      <div className="treemap">
        {tiles.map(({ x, y, w, h, datum }) => {
          const sel = selected.has(datum.id);
          const tiny = w < 54 || h < 30;
          const classes = [
            "block",
            datum.risk,
            sel ? "selected" : "",
            draining.has(datum.id) ? "draining" : "",
            hasSelection && !sel && isSelectable(datum) ? "dim" : "",
            tiny ? "tiny" : "",
          ]
            .filter(Boolean)
            .join(" ");

          return (
            <div
              key={datum.id}
              className={classes}
              style={{ left: x, top: y, width: w, height: h }}
              onClick={() => onToggle(datum)}
              onMouseMove={(e) => onHover(datum, e)}
              onMouseLeave={() => onHover(null)}
              role={isSelectable(datum) ? "button" : "img"}
              aria-pressed={isSelectable(datum) ? sel : undefined}
              aria-label={`${datum.name}, ${formatBytes(datum.bytes)}, ${datum.risk}`}
              tabIndex={isSelectable(datum) ? 0 : -1}
              onKeyDown={(e) => {
                if (isSelectable(datum) && (e.key === "Enter" || e.key === " ")) {
                  e.preventDefault();
                  onToggle(datum);
                }
              }}
            >
              <span className="b-name">{datum.name}</span>
              <span className="b-size mono">{formatBytes(datum.bytes)}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
