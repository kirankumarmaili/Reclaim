// Squarified treemap layout (Bruls, Huizing & van Wijk, 2000). Lays out values
// into a rectangle while keeping each tile's aspect ratio close to 1, so blocks
// read as comparable areas rather than thin slivers. Area ∝ value (PRD FR-6).

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Tile<T> extends Rect {
  datum: T;
}

interface Sized<T> {
  value: number;
  datum: T;
}

export function squarify<T>(items: Sized<T>[], bounds: Rect): Tile<T>[] {
  const total = items.reduce((s, it) => s + it.value, 0);
  if (total <= 0 || items.length === 0) return [];

  // Normalize values to fill the bounds' area exactly.
  const scale = (bounds.w * bounds.h) / total;
  const work = items
    .map((it) => ({ area: it.value * scale, datum: it.datum }))
    .sort((a, b) => b.area - a.area);

  const tiles: Tile<T>[] = [];
  layout(work, { ...bounds }, tiles);
  return tiles;
}

type Cell<T> = { area: number; datum: T };

function layout<T>(items: Cell<T>[], rect: Rect, out: Tile<T>[]): void {
  if (items.length === 0) return;

  let row: Cell<T>[] = [];
  let i = 0;

  while (i < items.length) {
    const next = items[i];
    const shorter = Math.min(rect.w, rect.h);
    const withNext = [...row, next];

    if (row.length === 0 || worst(withNext, shorter) <= worst(row, shorter)) {
      row = withNext;
      i += 1;
    } else {
      placeRow(row, rect, out);
      rect = remaining(row, rect);
      row = [];
    }
  }
  if (row.length > 0) placeRow(row, rect, out);
}

function sum<T>(row: Cell<T>[]): number {
  return row.reduce((s, c) => s + c.area, 0);
}

// Worst (largest) aspect ratio in a row laid along the shorter side of length w.
function worst<T>(row: Cell<T>[], w: number): number {
  const s = sum(row);
  if (s === 0) return Infinity;
  let max = -Infinity;
  let min = Infinity;
  for (const c of row) {
    max = Math.max(max, c.area);
    min = Math.min(min, c.area);
  }
  const w2 = w * w;
  const s2 = s * s;
  return Math.max((w2 * max) / s2, s2 / (w2 * min));
}

// Place a completed row along the shorter edge of `rect`.
function placeRow<T>(row: Cell<T>[], rect: Rect, out: Tile<T>[]): void {
  const s = sum(row);
  const horizontal = rect.w >= rect.h;
  if (horizontal) {
    const rowW = s / rect.h; // thickness along x
    let y = rect.y;
    for (const c of row) {
      const h = c.area / rowW;
      out.push({ x: rect.x, y, w: rowW, h, datum: c.datum });
      y += h;
    }
  } else {
    const rowH = s / rect.w; // thickness along y
    let x = rect.x;
    for (const c of row) {
      const w = c.area / rowH;
      out.push({ x, y: rect.y, w, h: rowH, datum: c.datum });
      x += w;
    }
  }
}

// The sub-rectangle left after a row is placed.
function remaining<T>(row: Cell<T>[], rect: Rect): Rect {
  const s = sum(row);
  const horizontal = rect.w >= rect.h;
  if (horizontal) {
    const rowW = s / rect.h;
    return { x: rect.x + rowW, y: rect.y, w: rect.w - rowW, h: rect.h };
  }
  const rowH = s / rect.w;
  return { x: rect.x, y: rect.y + rowH, w: rect.w, h: rect.h - rowH };
}
