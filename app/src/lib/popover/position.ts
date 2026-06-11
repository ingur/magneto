// Popover positioning helpers.
//
// All popovers in the app share the same core problem: given a trigger
// (a DOM rect or a point) and a bounding region (the panel the popover
// must stay inside), pick which edges to anchor by based on which side
// has more room.
//
// Three patterns:
//
//   * positionDropdown: trigger-rect vertical pick. Returns top OR
//     bottom plus a maxHeight clamp. Horizontal is the caller's job
//     (typically: left = rect.left, width = rect.width).
//
//   * positionAnchored: trigger-rect vertical AND horizontal pick.
//     Returns {top|bottom, left|right} aligned to the trigger's edges.
//     Used for buttons that anchor a popup to one of their corners.
//
//   * positionAtPoint: point-based vertical AND horizontal pick. Used
//     for cursor / right-click invocations where there's no trigger
//     element, just an (x, y).
//
// The vertical/horizontal pick is the same logic in both axes: prefer
// the "natural" direction (below / right) when it fits or has more
// room than the alternative, otherwise flip.
//
// Anchoring detail (load-bearing): the FLIPPED side anchors by the
// opposite CSS edge (bottom instead of top, right instead of left). If
// you anchor with `top = trigger.top - GAP - estHeight`, a popover that
// renders SHORTER than the estimate leaves dead space below it (it's
// positioned as if it were the full height). Anchoring by the opposite
// edge sidesteps this entirely: the popover's bottom edge stays at
// (trigger.top - GAP) regardless of how tall the actual content is.

export type PopoverEdges = {
  top: number | null;
  bottom: number | null;
  left: number | null;
  right: number | null;
  maxHeight?: number;
};

const DEFAULT_GAP = 4;

// Single-axis flip: prefer the "natural" direction (below/right) when it
// fits the estimated size or has more room than the alternative; flip
// otherwise. Returns which side was picked + the available room there.
// Shared by all three position helpers; vertical and horizontal axes
// reduce to the same decision.
function pickPlacement(opts: { spaceNatural: number; spaceFlipped: number; est: number }): {
  useNatural: boolean;
  space: number;
} {
  const useNatural = opts.spaceNatural >= opts.est || opts.spaceNatural >= opts.spaceFlipped;
  return { useNatural, space: useNatural ? opts.spaceNatural : opts.spaceFlipped };
}

export function positionDropdown(
  triggerRect: DOMRect,
  bounds: DOMRect,
  opts?: { gap?: number; edge?: number; preferredHeight?: number },
): { top: number | null; bottom: number | null; maxHeight: number } {
  const gap = opts?.gap ?? DEFAULT_GAP;
  const edge = opts?.edge ?? 16;
  const preferred = opts?.preferredHeight ?? 240;

  // Dropdown reserves an extra `edge` margin against the bound: keeps
  // the menu from kissing the panel border. So the room calculations
  // subtract gap + edge, not just gap (anchored/atPoint use only gap).
  const { useNatural, space } = pickPlacement({
    spaceNatural: bounds.bottom - triggerRect.bottom - gap - edge,
    spaceFlipped: triggerRect.top - bounds.top - gap - edge,
    est: preferred,
  });

  if (useNatural) {
    return {
      top: triggerRect.bottom + gap,
      bottom: null,
      maxHeight: Math.max(0, Math.min(preferred, space)),
    };
  }
  return {
    top: null,
    bottom: window.innerHeight - triggerRect.top + gap,
    maxHeight: Math.max(0, Math.min(preferred, space)),
  };
}

export function positionAnchored(
  triggerRect: DOMRect,
  bounds: DOMRect,
  est: { height: number; width: number },
  opts?: { gap?: number },
): PopoverEdges {
  const gap = opts?.gap ?? DEFAULT_GAP;

  // Vertical: below if it fits or has more room.
  const v = pickPlacement({
    spaceNatural: bounds.bottom - triggerRect.bottom - gap,
    spaceFlipped: triggerRect.top - bounds.top - gap,
    est: est.height,
  });
  const top = v.useNatural ? triggerRect.bottom + gap : null;
  const bottom = v.useNatural ? null : window.innerHeight - triggerRect.top + gap;

  // Horizontal: align to the trigger's right edge by default (menu's
  // right edge sits at the trigger's right edge). Flip to left if there's
  // not enough room on the left for the menu to extend that direction.
  // Asymmetric (there's no "spaceFlipped" comparison, just a fits check),
  // so this stays inline rather than going through pickPlacement.
  let left: number | null = null;
  let right: number | null = null;
  const spaceLeftOfRightEdge = triggerRect.right - bounds.left;
  if (spaceLeftOfRightEdge >= est.width) {
    right = window.innerWidth - triggerRect.right;
  } else {
    left = triggerRect.left;
  }

  return { top, bottom, left, right };
}

export function positionAtPoint(
  x: number,
  y: number,
  bounds: DOMRect,
  est: { height: number; width: number },
  opts?: { gap?: number },
): PopoverEdges {
  const gap = opts?.gap ?? 0;

  const v = pickPlacement({
    spaceNatural: bounds.bottom - y,
    spaceFlipped: y - bounds.top,
    est: est.height,
  });
  const top = v.useNatural ? y + gap : null;
  const bottom = v.useNatural ? null : window.innerHeight - y + gap;

  const h = pickPlacement({
    spaceNatural: bounds.right - x,
    spaceFlipped: x - bounds.left,
    est: est.width,
  });
  const left = h.useNatural ? x + gap : null;
  const right = h.useNatural ? null : window.innerWidth - x + gap;

  return { top, bottom, left, right };
}
