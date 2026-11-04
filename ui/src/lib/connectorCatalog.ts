
export interface CatalogConnector {
  id: string;
  name: string;
  about: string;
  url?: string;
  command?: string;
  args?: string[];
  site: string;
  auth: "signin" | "gh" | "token" | "none";
  tokenUrl?: string;
  tokenName?: string;
}

export const CONNECTOR_ASSETS: Record<string, string> = {
  diagonaldiagrams: "/connectors/diagonaldiagrams.svg",
};

export const CATALOG: CatalogConnector[] = [
  {
    id: "diagonaldiagrams",
    name: "diagonaldiagrams",
    about: "Draw diagrams and charts, and check their layout",
    command: "uvx",
    args: ["diagonaldiagrams", "mcp"],
    site: "https://lucas19919.github.io/diagonaldiagrams/",
    auth: "none",
  },
  {
    id: "github",
    name: "GitHub",
    about: "Repositories, issues and pull requests",
    url: "https://api.githubcopilot.com/mcp/",
    site: "https://github.com",
    auth: "gh",
  },
  { id: "higgsfield", name: "Higgsfield", about: "Make images and videos", url: "https://mcp.higgsfield.ai/mcp", site: "https://higgsfield.ai", auth: "signin" },
  { id: "notion", name: "Notion", about: "Pages and databases", url: "https://mcp.notion.com/mcp", site: "https://www.notion.so", auth: "signin" },
  { id: "linear", name: "Linear", about: "Issues, projects and cycles", url: "https://mcp.linear.app/mcp", site: "https://linear.app", auth: "signin" },
  { id: "canva", name: "Canva", about: "Designs and brand assets", url: "https://mcp.canva.com/mcp", site: "https://www.canva.com", auth: "signin" },
  { id: "sentry", name: "Sentry", about: "Errors and performance", url: "https://mcp.sentry.dev/mcp", site: "https://sentry.io", auth: "signin" },
  { id: "stripe", name: "Stripe", about: "Payments, customers and billing", url: "https://mcp.stripe.com", site: "https://stripe.com", auth: "signin" },
  {
    id: "huggingface",
    name: "Hugging Face",
    about: "Models, datasets and Spaces",
    url: "https://huggingface.co/mcp?login",
    site: "https://huggingface.co",
    auth: "signin",
  },
  { id: "deepwiki", name: "DeepWiki", about: "Docs for any public GitHub repo", url: "https://mcp.deepwiki.com/mcp", site: "https://deepwiki.com", auth: "none" },
];

export interface ServerCfg {
  command?: string;
  args?: string[];
  env?: Record<string, string>;
  url?: string;
  headers?: Record<string, string>;
  auth?: string;
  enabled?: boolean;
  timeout_ms?: number;
  [k: string]: unknown;
}

export function serverOf(c: CatalogConnector): ServerCfg {
  if (c.command) return { command: c.command, args: c.args ?? [], env: {}, enabled: true, timeout_ms: 60_000 };
  return hostedServer(c.url ?? "", { auth: c.auth === "gh" ? "gh" : "" });
}

export function hostedServer(url: string, opts: { token?: string; auth?: string } = {}): ServerCfg {
  const headers: Record<string, string> = opts.token ? { Authorization: `Bearer ${opts.token}` } : {};
  return { command: "", url, headers, auth: opts.auth ?? "", enabled: true, timeout_ms: 60_000 };
}

export function asBearer(raw: string): string {
  const value = raw.trim();
  const bearer = /^bearer\s+/i.exec(value);
  if (bearer) return `Bearer ${value.slice(bearer[0].length).trim()}`;
  if (/^[A-Za-z]+ /.test(value)) return value;
  return `Bearer ${value}`;
}

function normalizeAuthorization(headers: Record<string, string>) {
  const key = Object.keys(headers).find((k) => k.toLowerCase() === "authorization");
  if (!key) return;
  const value = headers[key]?.trim() ?? "";
  delete headers[key];
  if (value) headers.Authorization = asBearer(value);
}

function headerLine(line: string): [string, string] | null {
  const match = /^([A-Za-z][A-Za-z0-9-]*)\s*:\s*(\S.*)$/.exec(line);
  if (!match) return null;
  return [match[1], match[2].trim()];
}

export function remoteFromPaste(text: string): ServerCfg | null {
  const lines = text.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
  const url = lines[0] ?? "";
  if (!/^https?:\/\/\S+$/i.test(url)) return null;
  const headers: Record<string, string> = {};
  for (const line of lines.slice(1)) {
    const header = headerLine(line);
    if (header) headers[header[0]] = header[1];
    else if (!Object.keys(headers).some((k) => k.toLowerCase() === "authorization")) headers.Authorization = line;
  }
  normalizeAuthorization(headers);
  return { command: "", url, headers, auth: "", enabled: true, timeout_ms: 60_000 };
}

export function withBearer(server: ServerCfg, token: string): ServerCfg {
  const raw = token.trim();
  if (!raw || !(server.url ?? "").trim()) return server;
  const headers = { ...(server.headers ?? {}) };
  for (const key of Object.keys(headers)) {
    if (key.toLowerCase() === "authorization") delete headers[key];
  }
  headers.Authorization = asBearer(raw);
  return { ...server, headers };
}

export function nameFromUrl(url: string): string {
  try {
    const host = new URL(url).hostname.replace(/^(www|mcp|api)\./, "");
    return host.split(".")[0].replace(/[^a-z0-9_-]/gi, "").toLowerCase() || "connector";
  } catch {
    return "connector";
  }
}
