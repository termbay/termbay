// Where things are in the office (task C2): desks spread by the number of
// agents, a corridor with the door, and a coffee corner. Pure geometry, in
// CSS pixels, so it is tested without WebGL.

export interface Point {
  readonly x: number;
  readonly y: number;
}

export interface Size {
  readonly width: number;
  readonly height: number;
}

export interface Rect extends Point, Size {}

/** Room a desk and its character need at full scale. */
export const CELL = { width: 84, height: 76 } as const;
/** Characters and desks never shrink below this. */
export const MIN_SCALE = 0.4;
/** Corridor along the bottom, with the door on the left and coffee on the right. */
export const CORRIDOR_HEIGHT = 56;
export const COFFEE_WIDTH = 64;
/** From a desk's center to its seat, at full scale. */
export const SEAT_OFFSET = 4;
/** From a desk's center to its front edge, at full scale (drawing order). */
export const DESK_FRONT = 14;
/** Margin around the desk area. */
const PAD = 6;

export interface OfficeLayout {
  readonly size: Size;
  /** Scale of desks and characters (1 at full size). */
  readonly scale: number;
  /** Where the desks are. */
  readonly floor: Rect;
  readonly corridor: Rect;
  /** Where characters come in and leave. */
  readonly door: Point;
  readonly coffee: Rect;
  /** One area per session, in session order. */
  readonly zones: readonly Rect[];
  /** Desk centers, by session and then by agent index. */
  readonly desks: readonly (readonly Point[])[];
  /** Where each character sits, right below its desk. */
  readonly seats: readonly (readonly Point[])[];
}

/** Columns for `n` desks in a `width` x `height` area, keeping cells close to CELL's shape. */
export function columnsFor(n: number, width: number, height: number): number {
  if (n <= 1) return 1;
  const aspect = width / CELL.width / Math.max(1e-6, height / CELL.height);
  return Math.max(1, Math.min(n, Math.round(Math.sqrt(n * aspect))));
}

/** Splits `rect` into `n` equal parts along its longer side. */
function split(rect: Rect, n: number): Rect[] {
  if (n <= 0) return [];
  const vertical = rect.height >= rect.width;
  return Array.from({ length: n }, (_, i) =>
    vertical
      ? { x: rect.x, y: rect.y + (rect.height / n) * i, width: rect.width, height: rect.height / n }
      : { x: rect.x + (rect.width / n) * i, y: rect.y, width: rect.width / n, height: rect.height },
  );
}

/**
 * The office for a panel of `size`, with `counts[i]` agents in session `i`.
 * Desk `j` of session `i` is the j-th agent in the session's order (main
 * first), so a desk does not change owner when others arrive.
 */
export function layoutOffice(size: Size, counts: readonly number[]): OfficeLayout {
  const width = Math.max(0, size.width);
  const height = Math.max(0, size.height);
  const corridorHeight = Math.min(CORRIDOR_HEIGHT, height * 0.3);
  const corridor: Rect = { x: 0, y: height - corridorHeight, width, height: corridorHeight };
  const coffeeWidth = Math.min(COFFEE_WIDTH, width * 0.3);
  const coffee: Rect = { x: width - coffeeWidth, y: corridor.y, width: coffeeWidth, height: corridorHeight };
  const door: Point = { x: 0, y: corridor.y + corridorHeight / 2 };
  const floor: Rect = {
    x: PAD,
    y: PAD,
    width: Math.max(0, width - 2 * PAD),
    height: Math.max(0, corridor.y - 2 * PAD),
  };

  const zones = split(floor, counts.length);
  let scale = 1;
  const desks: Point[][] = [];
  const seats: Point[][] = [];
  zones.forEach((zone, i) => {
    const n = Math.max(0, counts[i] ?? 0);
    const cols = columnsFor(n, zone.width, zone.height);
    const rows = Math.max(1, Math.ceil(n / cols));
    const cellW = zone.width / cols;
    const cellH = zone.height / rows;
    if (n > 0) scale = Math.min(scale, cellW / CELL.width, cellH / CELL.height);
    const row: Point[] = [];
    for (let j = 0; j < n; j++) {
      const col = j % cols;
      const r = Math.floor(j / cols);
      row.push({ x: zone.x + cellW * (col + 0.5), y: zone.y + cellH * (r + 0.5) });
    }
    desks.push(row);
  });
  scale = Math.max(MIN_SCALE, scale);
  // The seat is behind the desk, a little below its center and above its
  // front edge (DESK_FRONT), so the character faces the viewer and the desk
  // covers its legs.
  const seatOffset = SEAT_OFFSET * scale;
  for (const row of desks) seats.push(row.map((d) => ({ x: d.x, y: d.y + seatOffset })));

  return { size: { width, height }, scale, floor, corridor, door, coffee, zones, desks, seats };
}

/** Spots per row in the coffee corner before a new row starts. */
const SPOTS_PER_ROW = 4;

/**
 * Where the i-th free agent stands for coffee: in a queue that grows from the
 * coffee corner toward the door, in up to two rows inside the corridor.
 */
export function breakSpot(layout: OfficeLayout, index: number): Point {
  const step = 18 * layout.scale;
  const { corridor, coffee } = layout;
  const col = index % SPOTS_PER_ROW;
  const row = Math.floor(index / SPOTS_PER_ROW);
  const x = Math.max(corridor.x + step, coffee.x - step * (col + 0.5) - step * 0.5 * (row % 2));
  const lanes = [0.62, 0.32];
  const y = corridor.y + corridor.height * (lanes[row % 2] ?? 0.5);
  return { x, y };
}

/** Whether `p` is in the corridor (where walking is free). */
export function inCorridor(layout: OfficeLayout, p: Point): boolean {
  return p.y >= layout.corridor.y - 0.5;
}

/**
 * Path from `from` to `to`: through the middle of the corridor, as people
 * walk around desks, unless both ends are already in the corridor or `direct`
 * is set (a desk that only moved a little after a new arrival).
 */
export function route(layout: OfficeLayout, from: Point, to: Point, direct = false): Point[] {
  const same = (a: Point, b: Point) => Math.abs(a.x - b.x) < 0.5 && Math.abs(a.y - b.y) < 0.5;
  if (same(from, to)) return [];
  if (direct || (inCorridor(layout, from) && inCorridor(layout, to))) return [to];
  const lane = layout.corridor.y + layout.corridor.height / 2;
  const points: Point[] = [{ x: from.x, y: lane }, { x: to.x, y: lane }, to];
  const path: Point[] = [];
  let last = from;
  for (const p of points) {
    if (!same(p, last)) path.push(p);
    last = p;
  }
  return path;
}
