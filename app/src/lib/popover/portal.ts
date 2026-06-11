// Svelte action that re-parents a node to <body> on mount and removes it
// on destroy. Used by popovers (menus, dropdowns) so they escape any
// containing-block constraint (backdrop-filter, transform, contain) on
// their ancestors and can position fixed against the viewport.
//
// Owning the destroy here is load-bearing: an $effect-based portal
// would leak the element because Svelte's destroy traverses the template
// tree and never finds the re-parented node. With a use:action the
// destroy callback is attached to THIS element and runs on every unmount
// path (esc, click-outside, option-click, cascading close).

export function portal(node: HTMLElement) {
  document.body.appendChild(node);
  return {
    destroy() {
      node.remove();
    },
  };
}
