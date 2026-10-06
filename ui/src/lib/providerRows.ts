import type { ProviderStatus } from "./api";

export const PROVIDER_ORDER = ["claude", "codex", "opencode", "grok", "antigravity", "cursor"];

const PROVIDER_NAME: Record<string, string> = {
  claude: "Claude",
  codex: "Codex",
  opencode: "OpenCode",
  grok: "Grok",
  antigravity: "Antigravity",
  cursor: "Cursor",
};

export const nameOf = (id: string): string => PROVIDER_NAME[id] ?? id;

export interface PickRow {
  provider: string;
  usable: boolean;
  label: string;
  value: string;
  note?: string;
}

export function prettyModel(id: string): string {
  const words = (id.split("/").pop() ?? id).split(/[-_\s]+/).filter(Boolean);
  const out: string[] = [];
  for (const w of words) {
    const prev = out[out.length - 1];
    if (/^\d{1,2}$/.test(w) && prev && /^\d+(\.\d+)*$/.test(prev)) out[out.length - 1] = `${prev}.${w}`;
    else if (/^gpt$/i.test(w)) out.push("GPT");
    else out.push(w[0].toUpperCase() + w.slice(1));
  }
  return out.join(" ") || id;
}

export const modelLabel = (m: { id: string; name: string }): string =>
  m.name && m.name !== m.id ? m.name : prettyModel(m.id);

const sourceOf = (id: string): string => id.split("/").slice(0, -1).join("/");

export const AUTO_ROW: PickRow = { provider: "auto", usable: true, label: "Smart Auto", value: "auto" };

export const isUsable = (p: ProviderStatus | undefined): boolean =>
  !!p && (p.state === "ready" || p.state === "unchecked");

function rowsOf(p: ProviderStatus): PickRow[] {
  const usable = isUsable(p);
  if (!p.models.length) {
    return [{ provider: p.provider, usable, label: `${nameOf(p.provider)} (its default model)`, value: p.provider }];
  }
  return p.models.map((m) => ({
    provider: p.provider,
    usable,
    label: modelLabel(m),
    value: `${p.provider}/${m.id}`,
    note: sourceOf(m.id),
  }));
}

export const allRows = (board: ProviderStatus[]): PickRow[] => board.flatMap(rowsOf);

export function stateLabel(p: ProviderStatus): string {
  switch (p.state) {
    case "ready":
      return p.account || "Ready";
    case "unchecked":
      return "Installed";
    case "signed_out":
      return "Not signed in";
    case "not_installed":
      return "Not installed";
    case "disabled":
      return "Off";
    case "error":
      return "Check failed";
  }
}

export function shownOf(value: string, board: ProviderStatus[]): { provider: string; name: string } {
  if (!value || value === "auto") return { provider: "auto", name: "Smart Auto" };
  const [p, ...rest] = value.split("/");
  const id = rest.join("/");
  const m = board.find((b) => b.provider === p)?.models.find((x) => x.id === id);
  if (m) return { provider: p, name: modelLabel(m) };
  return { provider: p, name: id ? prettyModel(id) : nameOf(p) };
}

const PILL = ["low", "medium", "high", "extra", "ultra"];

export const DEFAULT_EFFORTS = PILL;

export function effortsFor(value: string, board: ProviderStatus[]): string[] {
  if (!value || value === "auto") return PILL;
  const [p, ...rest] = value.split("/");
  const models = board.find((b) => b.provider === p)?.models ?? [];
  const id = rest.join("/");
  const m = models.find((x) => x.id === id) ?? models.find((x) => x.is_default);
  const list = m?.efforts ?? [];
  return list.length ? list : PILL;
}

export function fitEffort(current: string, efforts: string[]): string {
  if (!efforts.length || efforts.includes(current)) return current;
  if (efforts.includes("medium")) return "medium";
  return efforts[Math.floor(efforts.length / 2)];
}

const EFFORT_HINT: Record<string, string> = {
  minimal: "Barely thinks",
  low: "Fastest",
  medium: "Balanced",
  high: "Thinks longer",
  xhigh: "Longer still",
  extra: "Longer still",
  max: "As long as it takes",
  ultra: "As long as it takes",
};

export const effortLabel = (e: string): string =>
  e === "xhigh" ? "Extra high" : e ? e[0].toUpperCase() + e.slice(1) : e;

export const effortHint = (e: string): string => EFFORT_HINT[e] ?? "";
