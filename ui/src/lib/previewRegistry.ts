import Omnibar from "./Omnibar.svelte";
import type { ComponentType } from "svelte";
import { parsePreview, PREVIEW_META, previewNames, previewProblem, type PreviewMeta } from "./previewSpec";

export type { PreviewMeta };
export { previewNames, previewProblem };

export interface PreviewEntry extends PreviewMeta {
  component: ComponentType;
  props: (variant: number, extra: Record<string, unknown>) => Record<string, unknown>;
}

function omnibarProps(variant: number, extra: Record<string, unknown>) {
  return {
    input: "",
    model: "",
    effort: "medium",
    permission: "full",
    mode: variant === 2 ? "search" : "build",
    attachments: [],
    folder: "C:/demo/parzi",
    branch: "main",
    hero: true,
    project: { slug: "ui-polish", title: "UI Polish", tokens: 1200 },
    ...extra,
  };
}

const BUILT: Record<string, Pick<PreviewEntry, "component" | "props">> = {
  omnibar: { component: Omnibar as unknown as ComponentType, props: omnibarProps },
};

const ENTRIES: PreviewEntry[] = PREVIEW_META.flatMap((meta) => {
  const built = BUILT[meta.name];
  return built ? [{ ...meta, ...built }] : [];
});

export const PREVIEWS: Record<string, PreviewEntry> = Object.fromEntries(
  ENTRIES.map((e) => [e.name, e]),
);

export function previewSpecOf(raw: string): { entry: PreviewEntry; variant: number; extra: Record<string, unknown>; style: string } | null {
  const parsed = parsePreview(raw);
  if (!parsed) return null;
  const entry = PREVIEWS[parsed.component];
  if (!entry) return null;
  return { entry, variant: parsed.variant, extra: parsed.extra, style: parsed.style };
}
