import { describe, expect, it } from "vitest";

import { bandRect, bandSelection, edgeVelocity, intersects } from "./drag-select";

describe("bandRect", () => {
  it("spans two points regardless of drag direction", () => {
    const down = bandRect(10, 20, 30, 60);
    expect(down).toEqual({ left: 10, top: 20, width: 20, height: 40 });
    const up = bandRect(30, 60, 10, 20);
    expect(up).toEqual(down);
  });

  it("collapses to zero size on a click in place", () => {
    expect(bandRect(5, 5, 5, 5)).toEqual({ left: 5, top: 5, width: 0, height: 0 });
  });
});

describe("intersects", () => {
  const band = { left: 0, top: 0, width: 100, height: 50 };

  it("overlapping rects intersect", () => {
    expect(intersects(band, { left: 50, top: 25, width: 100, height: 50 })).toBe(true);
    expect(intersects(band, { left: -10, top: -10, width: 20, height: 20 })).toBe(true);
  });

  it("a row inside the band intersects", () => {
    expect(intersects(band, { left: 10, top: 10, width: 10, height: 10 })).toBe(true);
  });

  it("touching edges do not select (strict overlap)", () => {
    expect(intersects(band, { left: 100, top: 0, width: 50, height: 50 })).toBe(false);
    expect(intersects(band, { left: 0, top: 50, width: 100, height: 50 })).toBe(false);
  });

  it("disjoint rects do not intersect", () => {
    expect(intersects(band, { left: 200, top: 200, width: 10, height: 10 })).toBe(false);
  });
});

describe("bandSelection", () => {
  const base = new Set(["a", "b"]);

  it("plain drag replaces the selection with the banded rows", () => {
    expect(bandSelection(base, ["c", "d"], false)).toEqual(new Set(["c", "d"]));
  });

  it("plain drag over nothing clears the selection", () => {
    expect(bandSelection(base, [], false)).toEqual(new Set());
  });

  it("additive drag unions banded rows with the drag-start selection", () => {
    expect(bandSelection(base, ["b", "c"], true)).toEqual(new Set(["a", "b", "c"]));
  });

  it("additive drag over nothing keeps the drag-start selection", () => {
    expect(bandSelection(base, [], true)).toEqual(new Set(["a", "b"]));
  });
});

describe("edgeVelocity", () => {
  // Viewport from 100 to 500, default edge 24 / maxStep 16.
  const min = 100;
  const max = 500;

  it("is zero away from both edges", () => {
    expect(edgeVelocity(300, min, max)).toBe(0);
    expect(edgeVelocity(min + 24, min, max)).toBe(0);
    expect(edgeVelocity(max - 24, min, max)).toBe(0);
  });

  it("ramps up toward the top edge and scrolls up", () => {
    expect(edgeVelocity(min + 12, min, max)).toBe(-8);
    expect(edgeVelocity(min, min, max)).toBe(-16);
  });

  it("ramps up toward the bottom edge and scrolls down", () => {
    expect(edgeVelocity(max - 12, min, max)).toBe(8);
    expect(edgeVelocity(max, min, max)).toBe(16);
  });

  it("clamps to maxStep when the pointer is past the edge", () => {
    expect(edgeVelocity(min - 200, min, max)).toBe(-16);
    expect(edgeVelocity(max + 200, min, max)).toBe(16);
  });
});
