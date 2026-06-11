// Shared width preset for setting controls. Inputs / dropdowns / file
// pickers all draw from the same scale so a column of mixed controls
// reads as a vertical column rather than a ragged edge.
//
//   sm: short text or compact dropdowns (max-w-48 / 192px)
//   md: middling values (max-w-72 / 288px)
//   lg: paths, comma lists, command-line args (max-w-96 / 384px)
//
// Components pick a sensible default (Input=sm, Dropdown=sm, FilePicker=lg)
// and callers override per-row when intent calls for it.

export type Size = "sm" | "md" | "lg";

export const widths: Record<Size, string> = {
  sm: "max-w-48",
  md: "max-w-72",
  lg: "max-w-96",
};
