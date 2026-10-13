// Home's cards scroll with no scrollbar, so the edges say what is there:
// a top fade once scrolled, a deeper bottom fade and a "More below" hint
// while there is more to see.

export type ScrollEdges = {
  // Height of the fade at the top edge, px: none until scrolled.
  top: number;
  // Height of the fade at the bottom edge, px: deeper while more is below.
  bottom: number;
  // How far is left to scroll, px.
  below: number;
  // The hint's opacity: it fades out over the last stretch, gone at the end.
  hint: number;
};

const TOP_FADE = 16;
// The scroller's bottom padding: the fade it keeps at the end.
const END_FADE = 32;
const EXTRA_FADE = 40;
const HINT_FADE = 96;
// Less than this left below counts as the end.
const END = 8;

export function scrollEdges(scrollTop: number, scrollHeight: number, clientHeight: number): ScrollEdges {
  const top = Math.max(0, scrollTop);
  const below = Math.max(0, scrollHeight - clientHeight - top);
  return {
    top: Math.min(top, TOP_FADE),
    bottom: END_FADE + Math.min(below, EXTRA_FADE),
    below,
    hint: below < END ? 0 : Math.min(1, below / HINT_FADE),
  };
}

// Where to scroll to bring the next row of cards to the top: the first row
// starting below the current top. `tops` are the cards' offsets in the
// scrolled content, `pad` the scroller's top padding, `max` the furthest it
// scrolls. A row that would leave only a sliver below goes to the end.
// Null past the last row.
export function nextRowTop(tops: number[], scrollTop: number, pad: number, max: number): number | null {
  const next = [...tops].sort((a, b) => a - b).find((t) => t - pad > scrollTop + 1);
  if (next === undefined) return null;
  return max - (next - pad) < END_FADE ? max : next - pad;
}

// Calls `update` with the scroller's edges on scroll and whenever it or its
// content changes size.
export function watchEdges(node: HTMLElement, update: (edges: ScrollEdges) => void) {
  let cb = update;
  const measure = () => cb(scrollEdges(node.scrollTop, node.scrollHeight, node.clientHeight));
  const ro = new ResizeObserver(measure);
  ro.observe(node);
  for (const child of node.children) ro.observe(child);
  node.addEventListener("scroll", measure, { passive: true });
  measure();
  return {
    update(next: (edges: ScrollEdges) => void) {
      cb = next;
    },
    destroy() {
      ro.disconnect();
      node.removeEventListener("scroll", measure);
    },
  };
}
