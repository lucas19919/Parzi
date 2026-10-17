import Omnibar from "./Omnibar.svelte";
import type { ComponentType } from "svelte";

export interface PreviewVariant {
  n: number;
  label: string;
  blurb: string;
}

export interface PreviewEntry {
  /** Stable name agents put in {"component": "..."}. */
  name: string;
  title: string;
  component: ComponentType;
  variants: PreviewVariant[];
  /** Demo defaults for a variant; artifact props merge over these. */
  props: (variant: number, extra: Record<string, unknown>) => Record<string, unknown>;
}

function omnibarProps(variant: number, extra: Record<string, unknown>) {
  return {
    input: "",
    model: "",
    effort: "medium",
    permission: "supervised",
    mode: variant === 2 ? "search" : "build",
    attachments: [],
    folder: "C:/demo/parzi",
    branch: "main",
    hero: true,
    project: { slug: "ui-polish", title: "UI Polish", tokens: 1200 },
    ...extra,
  };
}

// One entry per previewable component. Adding a component means one entry
// here — the gallery and ArtifactCard pick it up with no other changes.
const ENTRIES: PreviewEntry[] = [
  {
    name: "omnibar",
    title: "Omnibar",
    component: Omnibar as unknown as ComponentType,
    variants: [
      { n: 1, label: "Agent", blurb: "Talking to the agent." },
      { n: 2, label: "Web", blurb: "Searching the web or opening an address." },
    ],
    props: omnibarProps,
  },
];

export const PREVIEWS: Record<string, PreviewEntry> = Object.fromEntries(
  ENTRIES.map((e) => [e.name, e]),
);

export function previewNames(): string {
  return ENTRIES.map((e) => e.name).join(", ");
}

export function previewSpecOf(raw: string): { entry: PreviewEntry; variant: number; extra: Record<string, unknown> } | null {
  let p: { component?: unknown; variant?: unknown } & Record<string, unknown>;
  try {
    p = JSON.parse(raw);
  } catch {
    return null;
  }
  if (!p || typeof p.component !== "string") return null;
  const entry = PREVIEWS[p.component];
  if (!entry) return null;
  // Strict: unknown variants error, never silently render something else.
  if (typeof p.variant === "boolean") return null;
  const n = Number(p.variant);
  if (!Number.isInteger(n)) return null;
  const ns = entry.variants.map((v) => v.n);
  if (!ns.includes(n)) return null;
  const { component: _c, variant: _v, ...extra } = p;
  return { entry, variant: n, extra };
}

// Human-readable reason a preview spec failed, for the inline error card.
export function previewProblem(raw: string): string {
  let p: { component?: unknown; variant?: unknown };
  try {
    p = JSON.parse(raw);
  } catch {
    return `not JSON: "${raw.slice(0, 120)}"`;
  }
  const entry = typeof p?.component === "string" ? PREVIEWS[p.component] : undefined;
  if (!entry) return `unknown component "${String(p?.component).slice(0, 40)}". Known: ${previewNames()}`;
  const ns = entry.variants.map((v) => v.n);
  return `unknown variant ${String(p?.variant).slice(0, 40)} for ${entry.name}. Valid: ${ns.join(", ")}`;
}
