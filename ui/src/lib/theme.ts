// Theme plumbing shared by App (startup) and the Appearance page.
//
// Two layers: a <style data-parzi> tag carries the authoritative CSS from
// Rust (theme.toml vars + user.css); inline `--parzi-*` properties on <html>
// carry a live preview while a control is being dragged. Applying the
// authoritative CSS always clears the preview, so a stale inline value can
// never outlive the setting it previewed.
import { api, type Theme } from "./api";

const GENERIC_FAMILIES = new Set([
  "system-ui", "sans-serif", "serif", "monospace", "ui-monospace",
  "ui-sans-serif", "ui-serif", "ui-rounded", "cursive", "fantasy", "inherit",
]);

/** Mirror of `theme::css_font_list` in Rust: quote names, keep generics bare. */
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

/** #abc → #AABBCC; passes 6-digit values through upper-cased. */
export function normalizeHex(s: string): string {
  const h = s.trim();
  if (h.length === 4) return ("#" + h[1] + h[1] + h[2] + h[2] + h[3] + h[3]).toUpperCase();
  return h.toUpperCase();
}

/** Mirror of `theme::accent_ink`: text colour that reads on the accent. */
export function accentInk(hex: string): string {
  if (!isHex(hex)) return "#0B0D12";
  const h = normalizeHex(hex).slice(1);
  const v = parseInt(h, 16);
  const lin = (c: number) => {
    c /= 255;
    return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
  };
  const l = 0.2126 * lin(v >> 16) + 0.7152 * lin((v >> 8) & 0xff) + 0.0722 * lin(v & 0xff);
  return l > 0.179 ? "#0B0D12" : "#FFFFFF";
}

const pct = (v: number) => `${Math.round(Math.min(1, Math.max(0, v)) * 100)}%`;

/** The same inputs Rust emits, computed client-side for instant previews. */
export function themeVars(t: Theme): Record<string, string> {
  return {
    "--parzi-font": cssFontList(t.font.family),
    "--parzi-font-size": `${t.font.size}px`,
    "--parzi-mono": cssFontList(t.font.mono),
    "--parzi-mono-size": `${t.font.mono_size}px`,
    "--parzi-sidebar": t.colors.sidebar,
    "--parzi-stage": t.colors.stage,
    "--parzi-bar": t.colors.bar,
    "--parzi-border": t.colors.border,
    "--parzi-accent": t.colors.accent,
    "--parzi-accent-ink": accentInk(t.colors.accent),
    "--parzi-text": t.colors.text,
    "--parzi-text-dim": t.colors.text_dim,
    "--parzi-bg-dim": String(t.background.dim),
    "--parzi-bg-dim-pct": pct(t.background.dim),
    "--parzi-vignette": String(t.background.vignette),
    "--parzi-bg-blur": `${t.background.blur}px`,
    "--parzi-glass-opacity": String(t.glass.opacity),
    "--parzi-glass-opacity-pct": pct(t.glass.opacity),
    "--parzi-glass-radius": `${t.glass.radius}px`,
    "--parzi-glass-blur": `${t.glass.blur_px}px`,
    "--parzi-glass-shadow": t.glass.shadow ? "1" : "0",
  };
}

/** Live preview: inline vars on <html> win over the stylesheet until cleared. */
export function previewTheme(t: Theme) {
  const s = document.documentElement.style;
  for (const [k, v] of Object.entries(themeVars(t))) s.setProperty(k, v);
}

export function clearPreview() {
  const s = document.documentElement.style;
  const names: string[] = [];
  for (let i = 0; i < s.length; i++) {
    const n = s[i];
    if (n.startsWith("--parzi-")) names.push(n);
  }
  for (const n of names) s.removeProperty(n);
}

/** Swap in the authoritative stylesheet (vars + user.css) and drop previews. */
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

/** Tell the shell the wallpaper changed (App listens for `parzi:bg`). */
export async function refreshBackground() {
  const url = await api.backgroundUrl();
  document.dispatchEvent(new CustomEvent("parzi:bg", { detail: url }));
}

/** "rose-pine" → "Rose Pine". */
export function titleCase(slug: string): string {
  return slug
    .split(/[-_\s]+/)
    .filter(Boolean)
    .map((w) => w[0].toUpperCase() + w.slice(1))
    .join(" ");
}
