const DURATION = 300;
const EASING = "cubic-bezier(0.16, 1, 0.3, 1)"; // --ease-out

const still = () => typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches;

function play(el: HTMLElement, id: string, from: Keyframe, to: Keyframe) {
  el.animate([from, to], { id, duration: DURATION, easing: EASING });
}

function stop(el: HTMLElement, id: string) {
  for (const a of el.getAnimations()) if (a.id === id) a.cancel();
}

/**
 * For layout that cannot animate on its own, like moving from bottom to top.
 * Take `before` from getBoundingClientRect ahead of the change, call this after
 * it, and the element travels from where it was to where it now is.
 */
export function glideTo(el: HTMLElement, before: DOMRect) {
  if (still()) return;
  stop(el, "glide-to");
  const dy = before.top - el.getBoundingClientRect().top;
  if (Math.abs(dy) < 1) return;
  play(el, "glide-to", { translate: `0 ${dy}px` }, { translate: "0 0" });
}

/** The same for a height that changes because its content was swapped. */
export function glideHeight(el: HTMLElement, before: DOMRect) {
  if (still()) return;
  stop(el, "glide-height");
  const now = el.getBoundingClientRect().height;
  if (Math.abs(now - before.height) < 1) return;
  play(el, "glide-height", { height: `${before.height}px`, overflow: "hidden" }, { height: `${now}px`, overflow: "hidden" });
}
