import type { ProviderStatus } from "./api";

export const PROVIDER_ORDER = ["claude", "codex", "opencode", "grok", "antigravity", "cursor"];

const PROVIDER_NAME: Record<string, string> = {
  claude: "Claude",
  codex: "Codex",
  opencode: "OpenCode",
  grok: "Grok",
  antigravity: "Antigravity",
  cursor: "Cursor",
  acp: "ACP",
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

const CHIP_FILLER = new Set(["contributor", "free", "preview", "latest", "thinking", "new", "fast"]);
const CHIP_LAB = new Set(["claude", "anthropic", "openai"]);
const CHIP_VERSION = /^(?:v|k)?\d+(?:\.\d+)+$|^\d+$/i;

// The omnibar chip. The logo already says OpenCode, so "OpenCode Go/Muse
// Spark 1.3 Contributor" reads as Muse and 1.3. The full name stays on the
// tooltip.
export function chipOf(name: string): { title: string; version: string } {
  const slash = name.indexOf("/");
  const body = (slash >= 0 ? name.slice(slash + 1) : name).replace(/\s*\([^)]*\)/g, " ").replace(/\s+/g, " ").trim();
  if (!body) return { title: name, version: "" };
  const words: string[] = [];
  let version = "";
  for (const token of body.split(" ")) {
    if (CHIP_FILLER.has(token.toLowerCase())) continue;
    if (!version && words.length && CHIP_VERSION.test(token)) {
      version = token;
      continue;
    }
    words.push(token);
  }
  if (words.length >= 2 && CHIP_LAB.has(words[0].toLowerCase())) words.shift();
  const head = (words[0] ?? body).replace(/^(.+?\d(?:\.\d+)?)-[A-Za-z].*$/, "$1");
  if (version) return { title: head, version };
  if (words.length >= 2 && head === words[0]) return { title: `${words[0]} ${words[1]}`, version: "" };
  return { title: head, version: "" };
}

const sourceOf = (id: string): string => id.split("/").slice(0, -1).join("/");

// Only an agent that answered "ready" is offered: a never-checked one may
// not even be installed.
export const isUsable = (p: ProviderStatus | undefined): boolean => p?.state === "ready";

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
      return "Checking…";
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
  if (!value || value === "auto") return { provider: "", name: "Pick a model" };
  const [p, ...rest] = value.split("/");
  const id = rest.join("/");
  const m = board.find((b) => b.provider === p)?.models.find((x) => x.id === id);
  if (m) return { provider: p, name: modelLabel(m) };
  return { provider: p, name: id ? prettyModel(id) : nameOf(p) };
}

// The effort levels the model itself lists. ACP agents (opencode, grok,
// cursor, antigravity) take no --effort flag but still record the choice on
// the session, so they fall back to the standard five rather than hiding
// the picker.
const FALLBACK_EFFORTS = ["low", "medium", "high", "xhigh", "max"];
export function effortsFor(value: string, board: ProviderStatus[]): string[] {
  if (!value) return [];
  const [p, ...rest] = value.split("/");
  const models = board.find((b) => b.provider === p)?.models ?? [];
  const id = rest.join("/");
  const m = (id && models.find((x) => x.id === id)) || models.find((x) => x.is_default);
  const listed = m?.efforts ?? [];
  if (listed.length) return listed;
  if (["opencode", "grok", "cursor", "antigravity", "acp"].includes(p)) return [...FALLBACK_EFFORTS];
  return [];
}

// False only when the agent said this model can't see images.
export function seesImages(value: string, board: ProviderStatus[]): boolean {
  const [p, ...rest] = value.split("/");
  const id = rest.join("/");
  return board.find((b) => b.provider === p)?.models.find((x) => x.id === id)?.images !== false;
}

export function fitEffort(current: string, efforts: string[]): string {
  if (!efforts.length || efforts.includes(current)) return current;
  if (efforts.includes("medium")) return "medium";
  return efforts[Math.floor(efforts.length / 2)];
}

export const effortLabel = (e: string): string =>
  e === "xhigh" ? "Extra high" : e ? e[0].toUpperCase() + e.slice(1) : e;

// On a segment, where five must fit side by side: "Extra", as Claude says it.
export const effortShort = (e: string): string => (e === "xhigh" ? "Extra" : effortLabel(e));
