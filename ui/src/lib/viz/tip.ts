
export interface TipRow {
  value: string;
  label: string;
  color?: string;
}

let tip: HTMLDivElement | null = null;
let shownAt = 0;
let listening = false;

function onScroll() {
  if (performance.now() - shownAt > 150) hideTip();
}

function el(): HTMLDivElement {
  if (tip?.isConnected) return tip;
  tip = document.createElement("div");
  tip.className = "tip viz-tip";
  tip.setAttribute("role", "tooltip");
  tip.hidden = true;
  document.body.appendChild(tip);
  if (!listening) window.addEventListener("scroll", onScroll, true);
  listening = true;
  return tip;
}

export function showTip(anchor: { left: number; top: number; right: number; bottom: number }, head: string, rows: TipRow[]) {
  const t = el();
  t.replaceChildren();
  if (head) {
    const h = document.createElement("div");
    h.className = "viz-tip-head";
    h.textContent = head;
    t.appendChild(h);
  }
  for (const r of rows) {
    const line = document.createElement("div");
    line.className = "viz-tip-row";
    if (r.color) {
      const key = document.createElement("i");
      key.style.background = r.color;
      line.appendChild(key);
    }
    const v = document.createElement("b");
    v.textContent = r.value;
    line.appendChild(v);
    if (r.label) {
      const l = document.createElement("span");
      l.textContent = r.label;
      line.appendChild(l);
    }
    t.appendChild(line);
  }
  t.hidden = false;
  shownAt = performance.now();
  const box = t.getBoundingClientRect();
  const gap = 8;
  const mid = (anchor.left + anchor.right) / 2;
  const left = Math.max(gap, Math.min(mid - box.width / 2, window.innerWidth - box.width - gap));
  const above = anchor.top - box.height - gap;
  const top = above >= gap ? above : Math.min(anchor.bottom + gap, window.innerHeight - box.height - gap);
  t.style.left = `${Math.round(left)}px`;
  t.style.top = `${Math.round(top)}px`;
}

export function hideTip() {
  if (tip) tip.hidden = true;
}
