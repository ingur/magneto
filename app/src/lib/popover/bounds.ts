// Resolve a clamping rect for popover positioning. Walks up from the
// given element looking for an ancestor matching the source attr/selector,
// or falls back to the viewport.
//
// Examples:
//   getBounds(triggerEl, 'data-menu-bounds')        // closest [data-menu-bounds]
//   getBounds(triggerEl, '[role="dialog"]')          // closest [role="dialog"]
//   getBounds(null)                                  // viewport
//
// `source` accepts either a bare attribute name (no brackets, common case)
// or a full CSS selector. The function picks based on whether the value
// contains selector punctuation.

export function getBounds(from: Element | null, source?: string | (() => DOMRect)): DOMRect {
  if (typeof source === "function") return source();
  if (from && source) {
    const selector = /[\[\].#:>+~ ]/.test(source) ? source : `[${source}]`;
    const el = from.closest(selector);
    if (el) return el.getBoundingClientRect();
  }
  return new DOMRect(0, 0, window.innerWidth, window.innerHeight);
}
