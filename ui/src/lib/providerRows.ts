/** What the model menus show, built once from the provider board so the
 *  composer and the role pickers cannot disagree. */
import type { ProviderStatus } from "./api";

/** The roster in menu order. Mirrors the backend `PROVIDERS` list. */
export const PROVIDER_ORDER = ["claude", "codex", "opencode", "grok", "antigravity", "cursor"];

export const PROVIDER_NAME: Record<string, string> = {
  claude: "Claude",
  codex: "Codex",
  opencode: "OpenCode",
  grok: "Grok",
  antigravity: "Antigravity",
  cursor: "Cursor",
};

export const nameOf = (id: string): string => PROVIDER_NAME[id] ?? id;

/** One pickable line: a model of an agent, or the agent's own default. */
export interface PickRow {
  provider: string;
  /** Picking it can start a turn: signed in, or not checkable short of one. */
  usable: boolean;
  label: string;
  /** `provider/model`; `provider` alone runs the agent's default; "auto" is Smart Auto. */
  value: string;
}

export const AUTO_ROW: PickRow = { provider: "auto", usable: true, label: "Smart Auto", value: "auto" };

export const isUsable = (p: ProviderStatus | undefined): boolean =>
  !!p && (p.state === "ready" || p.state === "unchecked");

/** An agent's lines: its models, or its default when it lists none. */
export function rowsOf(p: ProviderStatus): PickRow[] {
  const usable = isUsable(p);
  if (!p.models.length) {
    return [{ provider: p.provider, usable, label: `${nameOf(p.provider)} (its default model)`, value: p.provider }];
  }
  return p.models.map((m) => ({
    provider: p.provider,
    usable,
    label: m.name || m.id,
    value: `${p.provider}/${m.id}`,
  }));
}

export const allRows = (board: ProviderStatus[]): PickRow[] => board.flatMap(rowsOf);

/** The state in a few words, for a menu line or a badge. */
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

/** A menu line's second line: agent · plan or state. */
export function rowSub(row: PickRow, board: ProviderStatus[]): string {
  if (row.provider === "auto") return "Starts on the first ready agent in your order";
  const p = board.find((b) => b.provider === row.provider);
  return p ? `${nameOf(row.provider)} · ${stateLabel(p)}` : nameOf(row.provider);
}

/** What a picker trigger shows for a value. */
export function shownOf(value: string, board: ProviderStatus[]): { provider: string; name: string } {
  if (!value || value === "auto") return { provider: "auto", name: "Smart Auto" };
  const [p, ...rest] = value.split("/");
  const id = rest.join("/");
  const m = board.find((b) => b.provider === p)?.models.find((x) => x.id === id);
  if (m) return { provider: p, name: m.name || m.id };
  return { provider: p, name: id || nameOf(p) };
}

/** Parzi's own effort pill; each agent's driver translates it. */
export const PILL = ["low", "medium", "high", "extra", "ultra"];

/** The efforts a choice takes: the model's own words, Parzi's pill for
 *  Smart Auto, nothing when the agent has no effort knob. */
export function effortsFor(value: string, board: ProviderStatus[]): string[] {
  if (!value || value === "auto") return PILL;
  const [p, ...rest] = value.split("/");
  const models = board.find((b) => b.provider === p)?.models ?? [];
  const id = rest.join("/");
  const m = models.find((x) => x.id === id) ?? models.find((x) => x.is_default);
  return m?.efforts ?? [];
}

/** The effort to keep when the choice changes: the current one when the new
 *  model takes it, else medium, else the middle of what it takes. */
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
