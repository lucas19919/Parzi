// Well-known connectors, one click each. Hosted ones run through
// mcp-remote, which speaks to the service over the web and opens its
// sign-in page in the browser the first time; tokens stay in ~/.mcp-auth.
// Pinned to a release a few weeks old, never "latest".
export const MCP_REMOTE = "mcp-remote@0.14.3";

export interface CatalogConnector {
  id: string;
  name: string;
  url: string;
  // "signin": the service's own sign-in page. "token": a key pasted here.
  auth: "signin" | "token";
  // For token connectors: where to make one, and what it is called there.
  tokenUrl?: string;
  tokenName?: string;
}

export const CATALOG: CatalogConnector[] = [
  {
    id: "github",
    name: "GitHub",
    url: "https://api.githubcopilot.com/mcp/",
    auth: "token",
    tokenUrl: "https://github.com/settings/personal-access-tokens/new",
    tokenName: "personal access token",
  },
  { id: "higgsfield", name: "Higgsfield", url: "https://mcp.higgsfield.ai/mcp", auth: "signin" },
  { id: "notion", name: "Notion", url: "https://mcp.notion.com/mcp", auth: "signin" },
  { id: "linear", name: "Linear", url: "https://mcp.linear.app/mcp", auth: "signin" },
  { id: "sentry", name: "Sentry", url: "https://mcp.sentry.dev/mcp", auth: "signin" },
  { id: "stripe", name: "Stripe", url: "https://mcp.stripe.com", auth: "signin" },
  { id: "huggingface", name: "Hugging Face", url: "https://huggingface.co/mcp?login", auth: "signin" },
];

export interface ServerCfg {
  command: string;
  args?: string[];
  env?: Record<string, string>;
  enabled?: boolean;
  timeout_ms?: number;
  [k: string]: unknown;
}

// A hosted MCP server as a connector. Sign-in can take a while the first
// time, so its calls get three minutes. A token travels in an env var: no
// space in the argument itself, which Windows would split.
export function remoteServer(url: string, token = ""): ServerCfg {
  const args = ["-y", MCP_REMOTE, url];
  const env: Record<string, string> = {};
  if (token) {
    args.push("--header", "Authorization:${AUTH_HEADER}");
    env.AUTH_HEADER = `Bearer ${token}`;
  }
  return { command: "npx", args, env, enabled: true, timeout_ms: 180_000 };
}

// The hosted address a connector reaches, if it is one made above.
export function remoteUrl(srv: ServerCfg): string {
  const args = srv.args ?? [];
  const at = args.findIndex((a) => a.startsWith("mcp-remote"));
  return at >= 0 ? (args[at + 1] ?? "") : "";
}

// A name for a connector added by address: its host, as a config key.
export function nameFromUrl(url: string): string {
  try {
    const host = new URL(url).hostname.replace(/^(www|mcp|api)\./, "");
    return host.split(".")[0].replace(/[^a-z0-9_-]/gi, "").toLowerCase() || "connector";
  } catch {
    return "connector";
  }
}
