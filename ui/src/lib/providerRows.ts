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

export type Billing = "subscription" | "api_key";

export const BILLING_OPTIONS: { value: Billing; label: string }[] = [
  { value: "subscription", label: "Subscription" },
  { value: "api_key", label: "API key" },
];

const BILLING_NOTE: Record<string, Record<Billing, string>> = {
  claude: { subscription: "Claude Pro or Max", api_key: "Anthropic API key, billed per token" },
  codex: { subscription: "ChatGPT plan", api_key: "OpenAI API key, billed per token" },
  opencode: { subscription: "OpenCode Go or a linked plan", api_key: "Zen and provider keys, billed per token" },
  grok: { subscription: "grok.com sign-in", api_key: "xAI API key, billed per token" },
  antigravity: { subscription: "Google account", api_key: "Gemini API key, billed per token" },
  cursor: { subscription: "Cursor plan", api_key: "Cursor API key" },
};

export const billingNote = (id: string, billing: Billing): string => BILLING_NOTE[id]?.[billing] ?? "";

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
  // Claude and Codex list efforts per model. With no list at all the check has not
  // come back yet or failed; keep the picker instead of dropping it. A model that
  // is listed with no efforts really has none.
  if (["claude", "codex"].includes(p) && !models.length) return [...FALLBACK_EFFORTS];
  return [];
}

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

