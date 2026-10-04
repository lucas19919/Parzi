import { setOverlay } from "./overlay";

let seq = 0;

export function placeAbove(anchor: HTMLElement, width: number, alignRight = false): string {
  const r = anchor.getBoundingClientRect();
  const left = Math.max(8, Math.min(alignRight ? r.right - width : r.left, window.innerWidth - width - 8));
  const above = r.top - 46;
  const below = window.innerHeight - r.bottom - 8;
  if (above >= 300 || above >= below) {
    const maxH = Math.max(180, Math.min(above, 560));
    return `left:${Math.round(left)}px;bottom:${Math.round(window.innerHeight - r.top + 8)}px;max-height:${Math.round(maxH)}px;`;
  }
  return `left:${Math.round(left)}px;top:${Math.round(r.bottom + 8)}px;max-height:${Math.round(Math.max(180, below))}px;`;
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

  window.addEventListener("pointerdown", onPointer, true);
  window.addEventListener("keydown", onKey);
  window.addEventListener("resize", onResize);

  return {
    update(next: PopoverOptions) {
      current = next;
    },
    destroy() {
      window.removeEventListener("pointerdown", onPointer, true);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("resize", onResize);
      setOverlay(key, false);
      node.remove();
    },
  };
}
