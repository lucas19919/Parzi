// Well-known connectors, one click each. Hosted ones run through
// mcp-remote, which speaks to the service over the web and opens its
// sign-in page in the browser the first time; tokens stay in ~/.mcp-auth.
// Pinned to a release a few weeks old, never "latest".
export const MCP_REMOTE = "mcp-remote@0.14.3";

export interface CatalogConnector {
  id: string;
  name: string;
  // One line: what it gives your agents.
  about: string;
  url: string;
  // The service's site: its icon stands in where no mark is bundled.
  site: string;
  // "signin": the service's own sign-in page. "token": a key pasted here.
  // "none": public, nothing to sign in to.
  auth: "signin" | "token" | "none";
  // For token connectors: where to make one, and what it is called there.
  tokenUrl?: string;
  tokenName?: string;
}

export const CATALOG: CatalogConnector[] = [
  {
    id: "github",
    name: "GitHub",
    about: "Repositories, issues and pull requests",
    url: "https://api.githubcopilot.com/mcp/",
    site: "https://github.com",
    auth: "token",
    tokenUrl: "https://github.com/settings/personal-access-tokens/new",
    tokenName: "personal access token",
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
