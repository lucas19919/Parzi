// Themed tooltips for controls without a visible label:
//   <button use:tooltip={"Close tab"}><Icon name="close" /></button>
// The text names the control (aria-label, unless it has its own name) and
// lives in a role="tooltip" element linked to it, shown after a short
// hover or on keyboard focus. Use it instead of `title`, whose native popup
// ignores the theme and is invisible to keyboard users.

const DELAY = 450;
// Moving between controls within this window shows the next tip at once.
const WARM = 400;

let seq = 0;
let openTip: { el: HTMLElement; hide: () => void } | null = null;
let closedAt = 0;
let listening = false;

// One listener for all tips: anchors move on scroll, the tip is fixed.
function hideOpen() {
  openTip?.hide();
}

function place(tip: HTMLElement, anchor: HTMLElement) {
  const r = anchor.getBoundingClientRect();
  const t = tip.getBoundingClientRect();
  const left = Math.max(6, Math.min(r.left + r.width / 2 - t.width / 2, window.innerWidth - t.width - 6));
  const below = r.bottom + 6;
  const top = below + t.height <= window.innerHeight - 6 ? below : Math.max(6, r.top - t.height - 6);
  tip.style.left = `${Math.round(left)}px`;
  tip.style.top = `${Math.round(top)}px`;
}

export function tooltip(node: HTMLElement, text: string | null | undefined) {
  const tip = document.createElement("div");
  tip.id = `tip-${++seq}`;
  tip.className = "tip";
  tip.setAttribute("role", "tooltip");
  tip.hidden = true;
  document.body.appendChild(tip);
  node.setAttribute("data-ui-tooltip", tip.id);

  let label = "";
  let timer = 0;
  // aria-label we set ourselves (vs. one the component owns).
  let ownLabel = false;

  function apply(next: string | null | undefined) {
    label = (next ?? "").trim();
    tip.textContent = label;
    const named = (!ownLabel && node.hasAttribute("aria-label")) || /[\p{L}\p{N}]/u.test(node.textContent ?? "");
    if (!named && label) {
      node.setAttribute("aria-label", label);
      ownLabel = true;
    } else if (named && label) {
      // The control has its own name: the tip adds to it.
      node.setAttribute("aria-describedby", tip.id);
    }
    if (!label) hide();
  }

  function show() {
    clearTimeout(timer);
    if (!label || !node.isConnected) return;
    if (openTip && openTip.el !== tip) openTip.hide();
    tip.hidden = false;
    place(tip, node);
    openTip = { el: tip, hide };
    if (!listening) {
      listening = true;
      window.addEventListener("scroll", hideOpen, true);
    }
  }

  function hide() {
    clearTimeout(timer);
    if (tip.hidden) return;
    tip.hidden = true;
    if (openTip?.el === tip) {
      openTip = null;
      closedAt = performance.now();
    }
  }

  function onEnter() {
    clearTimeout(timer);
    if (performance.now() - closedAt < WARM || openTip) show();
    else timer = window.setTimeout(show, DELAY);
  }

  function onFocus() {
    if (node.matches(":focus-visible")) show();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") hide();
  }

  node.addEventListener("pointerenter", onEnter);
  node.addEventListener("pointerleave", hide);
  node.addEventListener("pointerdown", hide);
  node.addEventListener("focus", onFocus);
  node.addEventListener("blur", hide);
  node.addEventListener("keydown", onKey);
  apply(text);

  return {
    update(next: string | null | undefined) {
      apply(next);
      if (!tip.hidden) place(tip, node);
    },
    destroy() {
      hide();
      node.removeEventListener("pointerenter", onEnter);
      node.removeEventListener("pointerleave", hide);
      node.removeEventListener("pointerdown", hide);
      node.removeEventListener("focus", onFocus);
      node.removeEventListener("blur", hide);
      node.removeEventListener("keydown", onKey);
      tip.remove();
    },
  };
}
