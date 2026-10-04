import { api, type Theme } from "./api";

const GENERIC_FAMILIES = new Set([
  "system-ui", "sans-serif", "serif", "monospace", "ui-monospace",
  "ui-sans-serif", "ui-serif", "ui-rounded", "cursive", "fantasy", "inherit",
]);

export function cssFontList(s: string): string {
  const parts = s
    .split(",")
    .map((p) => p.trim().replace(/^["']|["']$/g, "").trim())
    .filter(Boolean)
    .map((p) => (GENERIC_FAMILIES.has(p.toLowerCase()) ? p : `"${p.replace(/["\\;{}]/g, "")}"`));
  return parts.length ? parts.join(", ") : "inherit";
}

export function isHex(s: string): boolean {
  return /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(s.trim());
}

export function normalizeHex(s: string): string {
  const h = s.trim();
  if (h.length === 4) return ("#" + h[1] + h[1] + h[2] + h[2] + h[3] + h[3]).toUpperCase();
  return h.toUpperCase();
}

const clamp01 = (v: number) => Math.min(1, Math.max(0, v));

let probe: CanvasRenderingContext2D | null = null;

export function resolveColor(token: string, fallback: string): string {
  try {
    const el = document.createElement("span");
    el.style.color = `var(${token}, ${fallback})`;
    document.body.appendChild(el);
    const computed = getComputedStyle(el).color;
    el.remove();
    probe ??= document.createElement("canvas").getContext("2d", { willReadFrequently: true });
    if (!probe) return computed || fallback;
    probe.clearRect(0, 0, 1, 1);
    probe.fillStyle = computed;
    probe.fillRect(0, 0, 1, 1);
    const [r, g, b, a] = probe.getImageData(0, 0, 1, 1).data;
    return `rgba(${r}, ${g}, ${b}, ${(a / 255).toFixed(3)})`;
  } catch {
    return fallback;
  }
}

export function themeVars(t: Theme): Record<string, string> {
  return {
    "--font": cssFontList(t.font.family),
    "--font-size": `${t.font.size}px`,
    "--mono": cssFontList(t.font.mono),
    "--mono-size": `${t.font.mono_size}px`,
    "--bg": t.colors.stage,
    "--accent": t.colors.accent,
    "--text": t.colors.text,
    "--muted": t.colors.text_dim,
    "--bg-dim": String(clamp01(t.background.dim)),
    "--vignette": String(clamp01(t.background.vignette)),
    "--bg-blur": `${t.background.blur}px`,
  };
}

export function previewTheme(t: Theme) {
  const s = document.documentElement.style;
  for (const [k, v] of Object.entries(themeVars(t))) s.setProperty(k, v);
}

const THEME_VARS = ["--font", "--font-size", "--mono", "--mono-size", "--bg", "--accent", "--text", "--muted", "--bg-dim", "--vignette", "--bg-blur"];

export function clearPreview() {
  const s = document.documentElement.style;
  for (const n of THEME_VARS) s.removeProperty(n);
}

export function applyThemeCss(css: string) {
  document.querySelectorAll("style[data-parzi]").forEach((s) => s.remove());
  const style = document.createElement("style");
  style.setAttribute("data-parzi", "1");
  style.textContent = css;
  document.head.appendChild(style);
  clearPreview();
}

export async function refreshTheme() {
  applyThemeCss(await api.getThemeCss());
}

export async function refreshBackground() {
  const url = await api.backgroundUrl();
  document.dispatchEvent(new CustomEvent("parzi:bg", { detail: url }));
}

export function titleCase(slug: string): string {
  return slug
    .split(/[-_\s]+/)
    .filter(Boolean)
    .map((w) => w[0].toUpperCase() + w.slice(1))
    .join(" ");
}
