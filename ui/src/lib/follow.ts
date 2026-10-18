// Keeps a scrolling thread at its bottom while you are there: whatever
// grows under it (a streamed reply, a picture loading, the composer getting
// taller) stays in view. Scrolling up lets go; coming back down holds again.
// `toBottom(true)` glides there, following the bottom as it moves.

export interface Follower {
  attach: (node: HTMLElement) => { destroy: () => void };
  /** Goes to the bottom and holds there. */
  toBottom: (smooth?: boolean) => void;
  /** Whether it holds the bottom now. */
  holding: () => boolean;
}

const NEAR = 24;
const GLIDE_MS = 360;

export function follower(onChange?: (holding: boolean) => void): Follower {
  let el: HTMLElement | null = null;
  let hold = true;
  let lastTop = 0;
  let glide = 0;
  const reduced = typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches;

  const gap = () => (el ? el.scrollHeight - el.scrollTop - el.clientHeight : 0);
  const set = (h: boolean) => {
    if (h === hold) return;
    hold = h;
    onChange?.(h);
  };
  const pin = () => {
    if (!el) return;
    el.scrollTop = el.scrollHeight;
    lastTop = el.scrollTop;
  };

  function toBottom(smooth = false) {
    set(true);
    if (!el) return;
    cancelAnimationFrame(glide);
    glide = 0;
    if (!smooth || reduced || gap() < 2) {
      pin();
      return;
    }
    const from = el.scrollTop;
    const start = performance.now();
    const step = (now: number) => {
      if (!el) return;
      const t = Math.min(1, (now - start) / GLIDE_MS);
      const ease = 1 - Math.pow(1 - t, 3);
      // The bottom may move while gliding: aim at where it is now.
      const target = el.scrollHeight - el.clientHeight;
      el.scrollTop = from + (target - from) * ease;
      lastTop = el.scrollTop;
      glide = t < 1 ? requestAnimationFrame(step) : 0;
    };
    glide = requestAnimationFrame(step);
  }

  function attach(node: HTMLElement) {
    el = node;
    lastTop = node.scrollTop;
    const onScroll = () => {
      if (glide) return;
      const top = node.scrollTop;
      if (top < lastTop - 1 && gap() > NEAR) set(false);
      else if (gap() <= NEAR) set(true);
      lastTop = top;
    };
    // A wheel or key up while gliding is the person taking over.
    const takeOver = (e: Event) => {
      if (!glide) return;
      if (e instanceof WheelEvent && e.deltaY >= 0) return;
      cancelAnimationFrame(glide);
      glide = 0;
    };
    const grew = () => {
      if (hold && !glide) pin();
    };
    const ro = new ResizeObserver(grew);
    ro.observe(node);
    const watchContent = () => {
      for (const child of Array.from(node.children)) ro.observe(child);
    };
    watchContent();
    // The thread's column may be replaced (another session): watch the new one.
    const mo = new MutationObserver(watchContent);
    mo.observe(node, { childList: true });
    node.addEventListener("scroll", onScroll, { passive: true });
    node.addEventListener("wheel", takeOver, { passive: true });
    node.addEventListener("keydown", takeOver);
    return {
      destroy() {
        ro.disconnect();
        mo.disconnect();
        node.removeEventListener("scroll", onScroll);
        node.removeEventListener("wheel", takeOver);
        node.removeEventListener("keydown", takeOver);
        cancelAnimationFrame(glide);
        if (el === node) el = null;
      },
    };
  }

  return { attach, toBottom, holding: () => hold };
}
