
export interface PreviewMeta {
  name: string;
  title: string;
  variants: { n: number; label: string; blurb: string }[];
}

export const PREVIEW_META: PreviewMeta[] = [
  {
    name: "omnibar",
    title: "Omnibar",
    variants: [
      { n: 1, label: "Agent", blurb: "Talking to the agent." },
      { n: 2, label: "Web", blurb: "Searching the web or opening an address." },
    ],
  },
];

const COLORS = new Set(["bg", "text", "accent", "panel"]);
const LENGTHS = new Set(["radius", "space"]);
const HEX = /^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/;
const PX = /^[0-9]{1,2}px$/;

export function previewNames(): string {
  return PREVIEW_META.map((m) => m.name).join(", ");
}

export function tokenStyle(raw: unknown): string {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return "";
  const set: Record<string, string> = {};
  for (const [key, value] of Object.entries(raw as Record<string, unknown>)) {
    const name = key.replace(/^--/, "");
    if (typeof value !== "string") continue;
    const v = value.trim();
    if (COLORS.has(name) && HEX.test(v)) set[name] = v;
    else if (LENGTHS.has(name) && PX.test(v)) set[name] = v;
  }
  if (!Object.keys(set).length) return "";
  const derived: string[] = [];
  if ((set.bg || set.text) && !set.panel) derived.push("--panel:color-mix(in srgb, var(--text) 6%, var(--bg))");
  if (set.bg || set.text || set.panel) {
    derived.push(
      "--muted:color-mix(in srgb, var(--text) 78%, var(--bg))",
      "--line:color-mix(in srgb, var(--text) 12%, var(--bg))",
      "--faint:color-mix(in srgb, var(--muted) 62%, var(--bg))",
      "--hover:color-mix(in srgb, var(--text) 6%, transparent)",
      "--selected:color-mix(in srgb, var(--text) 10%, transparent)",
    );
  }
  if (set.space) derived.push("--gap:calc(var(--space) * 2)", "--radius-sm:calc(var(--space))");
  if (set.radius) derived.push("--radius-lg:var(--radius)");
  const explicit = Object.entries(set).map(([k, v]) => `--${k}:${v}`);
  return [...derived, ...explicit].join(";");
}

export interface ParsedPreview {
  component: string;
  variant: number;
  extra: Record<string, unknown>;
  style: string;
}

export function parsePreview(raw: string): ParsedPreview | null {
  let p: { component?: unknown; variant?: unknown; tokens?: unknown } & Record<string, unknown>;
  try {
    p = JSON.parse(raw);
  } catch {
    return null;
  }
  if (!p || typeof p.component !== "string") return null;
  const meta = PREVIEW_META.find((m) => m.name === p.component);
  if (!meta) return null;
  if (typeof p.variant === "boolean") return null;
  const n = Number(p.variant);
  if (!Number.isInteger(n) || !meta.variants.some((v) => v.n === n)) return null;
  const { component: _c, variant: _v, tokens, ...extra } = p;
  return { component: meta.name, variant: n, extra, style: tokenStyle(tokens) };
}

export function previewProblem(raw: string): string {
  let p: { component?: unknown; variant?: unknown };
  try {
    p = JSON.parse(raw);
  } catch {
    return `not JSON: "${raw.slice(0, 120)}"`;
  }
  const meta = typeof p?.component === "string" ? PREVIEW_META.find((m) => m.name === p.component) : undefined;
  if (!meta) return `unknown component "${String(p?.component).slice(0, 40)}". Known: ${previewNames()}`;
  const ns = meta.variants.map((v) => v.n);
  return `unknown variant ${String(p?.variant).slice(0, 40)} for ${meta.name}. Valid: ${ns.join(", ")}`;
}
