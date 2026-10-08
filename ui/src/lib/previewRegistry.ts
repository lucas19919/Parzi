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
    permission: "full",
    mode: "build",
    attachments: [],
    folder: "C:\\demo\\parzi",
    branch: "main",
    hero: true,
    project: { slug: "ui-polish", title: "UI Polish", tokens: 1200 },
    ...extra,
    variant,
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
      { n: 1, label: "Current", blurb: "Baseline. Full labels, full controls." },
      { n: 2, label: "Edition A · Priority row", blurb: "Icon-only modes, project-first with accent, one row." },
      { n: 3, label: "Edition B · Two-line calm", blurb: "Project on its own full-width row, controls below." },
      { n: 4, label: "Edition C · Compact", blurb: "Tighter pills, everything kept, nothing hidden." },
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
  const ns = entry.variants.map((v) => v.n);
  const variant = ns.includes(Number(p.variant)) ? Number(p.variant) : ns[0];
  const { component: _c, variant: _v, ...extra } = p;
  return { entry, variant, extra };
}
