import { setOverlay } from "./overlay";

let seq = 0;

/** Where a menu opens from its button. Inside a [data-pop-frame] (the
 *  composer), every menu opens from the frame's edge rather than its own
 *  button's: all on one line, above the composer whenever there is room for
 *  a usable list, and kept within its width. Each lines up with its button:
 *  by its left edge, or its right edge for a button in the right half. */
export function placeAbove(anchor: HTMLElement | null, width: number, alignRight?: boolean): string {
  if (!anchor || !anchor.isConnected) {
    return `left:${Math.max(8, Math.round((window.innerWidth - width) / 2))}px;top:64px;max-height:${Math.round(window.innerHeight - 128)}px;`;
  }
  const r = anchor.getBoundingClientRect();
  const frame = anchor.closest<HTMLElement>("[data-pop-frame]")?.getBoundingClientRect();
  const edge = frame ?? r;
  const right = alignRight ?? (frame ? r.left + r.width / 2 > frame.left + frame.width / 2 : false);
  let left = right ? r.right - width : r.left;
  if (frame) left = Math.max(frame.left, Math.min(left, frame.right - width));
  left = Math.max(8, Math.min(left, window.innerWidth - width - 8));
  // Room between the frame and the title bar (46px) or the window's foot,
  // less the 6px gap.
  const above = edge.top - 46 - 6;
  const below = window.innerHeight - edge.bottom - 8 - 6;
  if (above >= 180 || above >= below) {
    const maxH = Math.min(Math.max(above, 120), 560);
    return `left:${Math.round(left)}px;bottom:${Math.round(window.innerHeight - edge.top + 6)}px;max-height:${Math.round(maxH)}px;`;
  }
  return `left:${Math.round(left)}px;top:${Math.round(edge.bottom + 6)}px;max-height:${Math.round(Math.max(180, below))}px;`;
}

interface PopoverOptions {
  anchor: HTMLElement | null;
  close: () => void;
}

export function popover(node: HTMLElement, opts: PopoverOptions) {
  let current = opts;
  const key = `popover-${++seq}`;
  node.classList.add("pop");
  document.body.appendChild(node);
  setOverlay(key, true);

  function onPointer(e: PointerEvent) {
    const t = e.target as Node;
    if (node.contains(t) || current.anchor?.contains(t)) return;
    current.close();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") current.close();
  }

  function onResize() {
    current.close();
  }

  const opened = performance.now();

  function onScroll(e: Event) {
    if (e.target instanceof Node && node.contains(e.target)) return;
    if (performance.now() - opened < 150) return;
    current.close();
  }

  window.addEventListener("pointerdown", onPointer, true);
  window.addEventListener("keydown", onKey);
  window.addEventListener("resize", onResize);
  window.addEventListener("scroll", onScroll, true);

  return {
    update(next: PopoverOptions) {
      current = next;
    },
    destroy() {
      window.removeEventListener("pointerdown", onPointer, true);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("resize", onResize);
      window.removeEventListener("scroll", onScroll, true);
      setOverlay(key, false);
      node.remove();
    },
  };
}

export function portal(node: HTMLElement) {
  const key = `portal-${++seq}`;
  document.body.appendChild(node);
  setOverlay(key, true);
  return {
    destroy() {
      setOverlay(key, false);
      node.remove();
    },
  };
}
