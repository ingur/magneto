import { describe, expect, it } from "vitest";

import type { Row } from "./projection";
import { sortRows } from "./sort";

function row(over: Partial<Row>): Row {
  return {
    id: "t:x",
    kind: "torrent",
    name: "x",
    state: "complete",
    progress: 1,
    size: 0,
    enterable: true,
    persisted: "none",
    shared: "none",
    mixed: false,
    ...over,
  };
}

const names = (rows: Row[]) => rows.map((r) => r.name);

describe("sortRows", () => {
  const rows = [
    row({ name: "Bravo", size: 200, state: "complete", addedAt: "2026-01-02T00:00:00Z" }),
    row({ name: "alpha", size: 50, state: "downloading", addedAt: "2026-01-03T00:00:00Z" }),
    row({ name: "Charlie", size: 100, state: "paused", addedAt: "2026-01-01T00:00:00Z" }),
  ];

  it("sorts by name ascending and descending (locale-aware)", () => {
    expect(names(sortRows(rows, "name-asc"))).toEqual(["alpha", "Bravo", "Charlie"]);
    expect(names(sortRows(rows, "name-desc"))).toEqual(["Charlie", "Bravo", "alpha"]);
  });

  it("sorts by size", () => {
    expect(names(sortRows(rows, "size-desc"))).toEqual(["Bravo", "Charlie", "alpha"]);
    expect(names(sortRows(rows, "size-asc"))).toEqual(["alpha", "Charlie", "Bravo"]);
  });

  it("sorts by status (downloading before paused before complete)", () => {
    expect(names(sortRows(rows, "status"))).toEqual(["alpha", "Charlie", "Bravo"]);
  });

  it("sorts 'added' newest-first when rows carry added_at", () => {
    expect(names(sortRows(rows, "added"))).toEqual(["alpha", "Bravo", "Charlie"]);
  });

  it("leaves 'added' order untouched for rows without added_at (files)", () => {
    const files = [row({ name: "b", addedAt: undefined }), row({ name: "a", addedAt: undefined })];
    expect(names(sortRows(files, "added"))).toEqual(["b", "a"]);
  });

  it("sorts 'added' with undated rows last instead of abandoning the sort", () => {
    // A just-added torrent before its added_at resolves must not disable the
    // whole list's recency sort; dated rows still order newest-first.
    const mixed = [
      row({ name: "old", addedAt: "2026-01-01T00:00:00Z" }),
      row({ name: "undated", addedAt: undefined }),
      row({ name: "new", addedAt: "2026-01-03T00:00:00Z" }),
    ];
    expect(names(sortRows(mixed, "added"))).toEqual(["new", "old", "undated"]);
  });
});
