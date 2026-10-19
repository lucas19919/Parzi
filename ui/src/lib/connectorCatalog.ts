// Well-known connectors, one click each. Parzi speaks to hosted servers
// itself: one that signs in opens its page in your browser once and Parzi
// keeps the token; GitHub uses the GitHub CLI's login.

export interface CatalogConnector {
  id: string;
  name: string;
  // One line: what it gives your agents.
  about: string;
  url: string;
  // The service's site: its icon stands in where no mark is bundled.
  site: string;
  // "signin": the service's own sign-in page. "gh": the GitHub CLI's
  // login. "token": a key pasted here. "none": public.
  auth: "signin" | "gh" | "token" | "none";
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
  // A server this computer starts.
  command?: string;
  args?: string[];
  env?: Record<string, string>;
  // A hosted one: its address, any headers, and how it signs in
  // ("" its own sign-in or the headers, "gh" the GitHub CLI's login).
  url?: string;
  headers?: Record<string, string>;
  auth?: string;
  enabled?: boolean;
  timeout_ms?: number;
  [k: string]: unknown;
}

// A hosted server as a connector, with an API key if one was given.
export function hostedServer(url: string, opts: { token?: string; auth?: string } = {}): ServerCfg {
  const headers: Record<string, string> = opts.token ? { Authorization: `Bearer ${opts.token}` } : {};
  return { command: "", url, headers, auth: opts.auth ?? "", enabled: true, timeout_ms: 60_000 };
}

// What a connector reaches, for its row: an address or a command line.
export function serverLine(srv: ServerCfg): string {
  return srv.url || [srv.command ?? "", ...(srv.args ?? [])].join(" ").trim();
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
