import { api, type Theme } from "./api";

const GENERIC_FAMILIES = new Set([
  "system-ui", "sans-serif", "serif", "monospace", "ui-monospace",
  "ui-sans-serif", "ui-serif", "ui-rounded", "cursive", "fantasy", "inherit",
]);

function cssFontList(s: string): string {
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

/** Matches DITHER_MAX_OPACITY in the engine's theme.rs. */
const DITHER_MAX_OPACITY = 0.22;

function themeVars(t: Theme): Record<string, string> {
  return {
    "--font": cssFontList(t.font.family),
    "--font-size": `${t.font.size}px`,
    "--mono": cssFontList(t.font.mono),
    "--mono-size": `${t.font.mono_size}px`,
    "--bg": t.colors.stage,
    "--accent": t.colors.accent,
    "--text": t.colors.text,
    "--panel": t.colors.panel?.trim() ? t.colors.panel : "color-mix(in srgb, var(--text) 6%, var(--bg))",
    "--bg-height": `${Math.round(t.background.height * 100)}%`,
    "--bg-y": `${Math.round(t.background.y * 100)}%`,
    "--art-fill": t.background.fill ?? "band",
    "--art-cards": t.background.cards ?? "clear",
    // As the engine writes them: strength times the most it ever shows.
    "--dither": (Math.min(1, Math.max(0, t.background.dither ?? 0.25)) * DITHER_MAX_OPACITY).toFixed(3),
    "--dither-size": `${Math.min(6, Math.max(1, Math.round(t.background.dither_size ?? 1)))}px`,
  };
}

/** The wallpaper's fill and the cards' look arrive as theme variables; as
 *  attributes on the page, plain CSS can style by them. */
function syncArt() {
  const root = document.documentElement;
  const css = getComputedStyle(root);
  const fill = css.getPropertyValue("--art-fill").trim();
  const cards = css.getPropertyValue("--art-cards").trim();
  root.dataset.artFill = fill === "full" ? "full" : "band";
  root.dataset.artCards = cards === "glass" || cards === "solid" ? cards : "clear";
  root.dataset.tone = isLight(css.getPropertyValue("--bg")) ? "light" : "dark";
}

/** Whether a colour is light, by its relative luminance. Any CSS colour the
 *  browser understands works: the canvas turns it into RGB. */
function isLight(colour: string): boolean {
  const c = document.createElement("canvas").getContext("2d");
  if (!c || !colour.trim()) return false;
  c.fillStyle = "#000";
  c.fillStyle = colour.trim();
  c.fillRect(0, 0, 1, 1);
  const [r, g, b] = c.getImageData(0, 0, 1, 1).data;
  const lin = (v: number) => {
    const x = v / 255;
    return x <= 0.04045 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b) > 0.4;
}

export function previewTheme(t: Theme) {
  const s = document.documentElement.style;
  for (const [k, v] of Object.entries(themeVars(t))) s.setProperty(k, v);
  syncArt();
}

const THEME_VARS = ["--font", "--font-size", "--mono", "--mono-size", "--bg", "--accent", "--text", "--muted", "--panel", "--bg-height", "--bg-y", "--art-fill", "--art-cards", "--dither", "--dither-size"];

function clearPreview() {
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
  syncArt();
}

export async function refreshBackground() {
  const url = await api.backgroundUrl();
  document.dispatchEvent(new CustomEvent("parzi:bg", { detail: url }));
}

