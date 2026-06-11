// Pure geometry + semantics for the rubber-band drag selection. The gesture
// controller (drag-select.svelte.ts) owns DOM and pointer state; everything
// here is plain math so it can be unit-tested directly.

export interface BandRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** Normalized rect spanning two corner points (any drag direction). */
export function bandRect(ax: number, ay: number, bx: number, by: number): BandRect {
  return {
    left: Math.min(ax, bx),
    top: Math.min(ay, by),
    width: Math.abs(ax - bx),
    height: Math.abs(ay - by),
  };
}

/** Strict overlap: a band merely touching a row's edge doesn't select it. */
export function intersects(a: BandRect, b: BandRect): boolean {
  return (
    a.left < b.left + b.width &&
    b.left < a.left + a.width &&
    a.top < b.top + b.height &&
    b.top < a.top + a.height
  );
}

/** Next selection while banding: a plain drag replaces the selection with the
 *  banded rows; ctrl/cmd-drag adds them to the selection from drag start. */
export function bandSelection(
  base: ReadonlySet<string>,
  banded: readonly string[],
  additive: boolean,
): Set<string> {
  const next = additive ? new Set(base) : new Set<string>();
  for (const id of banded) next.add(id);
  return next;
}

/** Auto-scroll velocity (px per frame) while the pointer is within `edge` px
 *  of a viewport edge, or past it. Ramps linearly from 0 at the threshold to
 *  `maxStep` at (or beyond) the edge itself; 0 anywhere in the middle. */
export function edgeVelocity(
  pos: number,
  min: number,
  max: number,
  edge = 24,
  maxStep = 16,
): number {
  if (pos < min + edge) return -Math.min(1, (min + edge - pos) / edge) * maxStep;
  if (pos > max - edge) return Math.min(1, (pos - (max - edge)) / edge) * maxStep;
  return 0;
}
