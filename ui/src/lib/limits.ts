import type { UsageWindow } from "./api";

/** The plan limits that apply to `model` ("antigravity/gemini-4-neon"): every
 *  window, less those scoped to other models (Opus-only, Claude/GPT-only). */
export function limitsFor(windows: UsageWindow[], model: string): UsageWindow[] {
  const id = (model.split("/").slice(1).join("/") || "").toLowerCase();
  return windows.filter((w) => !w.models?.length || (!!id && w.models.some((word) => id.includes(word))));
}

/** The plan limits an agent's own models are under: a window scoped to
 *  models it doesn't offer (Antigravity's Claude/GPT quota while it lists
 *  Gemini only) is left out. With no model list yet, all of them. */
export function offeredLimits(windows: UsageWindow[], models: { id: string }[]): UsageWindow[] {
  if (!models.length) return windows;
  return windows.filter((w) => !w.models?.length || models.some((m) => w.models!.some((word) => m.id.toLowerCase().includes(word))));
}

/** The window the composer's ring shows: the session (5-hour) one when there
 *  is one, else the shortest the plan has (Grok's weekly credits). */
export function ringWindow(windows: UsageWindow[]): UsageWindow | null {
  return windows.find((w) => w.label === "Session") ?? windows[0] ?? null;
}
