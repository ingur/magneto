// Sort modes + labels: the sort vocabulary the chrome (SortButton) and nav
// bind to.

export type SortMode = "added" | "name-asc" | "name-desc" | "size-desc" | "size-asc" | "status";

// Labels are kept short and roughly equal-width; the icon next to each
// row carries the direction, so the label just says what's being sorted.
export const sortLabels: Record<SortMode, string> = {
  added: "Date added",
  "name-asc": "Name A → Z",
  "name-desc": "Name Z → A",
  "size-desc": "Size large",
  "size-asc": "Size small",
  status: "Status",
};
