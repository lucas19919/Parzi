/**
 * Move a node to `document.body` while it lives. A `position: fixed` menu
 * positioned in viewport coordinates is only placed right when no ancestor
 * has a `transform`, `filter` or `backdrop-filter`, because any of those
 * becomes its containing block. The composer sits in a transformed slot and
 * a blurred card, so its menus opened offset or off-screen until they were
 * portaled out.
 */
export function portal(node: HTMLElement) {
  document.body.appendChild(node);
  return {
    destroy() {
      node.remove();
    },
  };
}
