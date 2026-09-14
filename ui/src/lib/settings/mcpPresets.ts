/** Curated one-click connectors (Claude Code-style essentials + productivity).
 * Official modelcontextprotocol packages where they exist; community picks are
 * labelled and link to their repo so a bad package never installs silently. */

export interface PresetEnvField {
  key: string;
  label: string;
  placeholder: string;
  required: boolean;
  secret: boolean;
}

export interface PresetArgField {
  label: string;
  placeholder: string;
  required: boolean;
  /** Shown above the input (e.g. why a path is needed). */
  hint: string;
}

export interface McpPreset {
  id: string;
  /** Config key installed (`servers.<name>`). */
  name: string;
  label: string;
  desc: string;
  group: "Essentials" | "Code & Dev" | "Data" | "Web" | "Productivity";
  command: string;
  args: string[];
  /** Extra CLI args the user must supply at install (appended to `args`). */
  argField?: PresetArgField;
  envFields: PresetEnvField[];
  official: boolean;
  docs: string;
}

export const MCP_PRESETS: McpPreset[] = [
  {
    id: "fetch",
    name: "fetch",
    label: "Fetch",
    desc: "Turn any web page into markdown for the agent.",
    group: "Essentials",
    command: "npx",
    args: ["-y", "@modelcontextprotocol/server-fetch"],
    envFields: [],
    official: true,
    docs: "https://github.com/modelcontextprotocol/servers/tree/main/src/fetch",
  },
  {
    id: "memory",
    name: "memory",
    label: "Memory",
    desc: "Knowledge graph that persists across sessions.",
    group: "Essentials",
    command: "npx",
    args: ["-y", "@modelcontextprotocol/server-memory"],
    envFields: [],
    official: true,
    docs: "https://github.com/modelcontextprotocol/servers/tree/main/src/memory",
  },
  {
    id: "thinking",
    name: "thinking",
    label: "Sequential Thinking",
    desc: "Structured step-by-step reasoning tools.",
    group: "Essentials",
    command: "npx",
    args: ["-y", "@modelcontextprotocol/server-sequential-thinking"],
    envFields: [],
    official: true,
    docs: "https://github.com/modelcontextprotocol/servers/tree/main/src/sequentialthinking",
  },
  {
    id: "everything",
    name: "everything",
    label: "Everything (test)",
    desc: "Reference server — verifies your MCP setup works.",
    group: "Essentials",
    command: "npx",
    args: ["-y", "@modelcontextprotocol/server-everything"],
    envFields: [],
    official: true,
    docs: "https://github.com/modelcontextprotocol/servers/tree/main/src/everything",
  },
  {
    id: "filesystem",
    name: "filesystem",
    label: "Filesystem",
    desc: "Scoped file access for the agent (you pick the folder).",
    group: "Code & Dev",
    command: "npx",
    args: ["-y", "@modelcontextprotocol/server-filesystem"],
    argField: {
      label: "Allowed folder",
      placeholder: "C:\\Users\\lucas\\Documents",
      required: true,
      hint: "The agent can only touch files under this folder.",
    },
    envFields: [],
    official: true,
    docs: "https://github.com/modelcontextprotocol/servers/tree/main/src/filesystem",
  },
  {
    id: "github",
    name: "github",
    label: "GitHub",
    desc: "Repos, issues, PRs, search — needs a personal access token.",
    group: "Code & Dev",
    command: "npx",
    args: ["-y", "@modelcontextprotocol/server-github"],
    envFields: [
      {
        key: "GITHUB_PERSONAL_ACCESS_TOKEN",
        label: "Personal access token",
        placeholder: "ghp_…",
        required: true,
        secret: true,
      },
    ],
    official: true,
    docs: "https://github.com/modelcontextprotocol/servers/tree/main/src/github",
  },
  {
    id: "gitlab",
    name: "gitlab",
    label: "GitLab",
    desc: "Projects, issues, MRs — needs a personal access token.",
    group: "Code & Dev",
    command: "npx",
    args: ["-y", "@modelcontextprotocol/server-gitlab"],
    envFields: [
      {
        key: "GITLAB_PERSONAL_ACCESS_TOKEN",
        label: "Personal access token",
        placeholder: "glpat-…",
        required: true,
        secret: true,
      },
      {
        key: "GITLAB_API_URL",
        label: "API URL (self-hosted only)",
        placeholder: "https://gitlab.example.com",
        required: false,
        secret: false,
      },
    ],
    official: true,
    docs: "https://github.com/modelcontextprotocol/servers/tree/main/src/gitlab",
  },
  {
    id: "postgres",
    name: "postgres",
    label: "Postgres",
    desc: "Read-only-friendly SQL bridge (you supply the connection).",
    group: "Data",
    command: "npx",
    args: ["-y", "@modelcontextprotocol/server-postgres"],
    argField: {
      label: "Connection string",
      placeholder: "postgresql://user:pass@localhost:5432/db",
      required: true,
      hint: "Passed as the server argument. Prefer a read-only role.",
    },
    envFields: [],
    official: true,
    docs: "https://github.com/modelcontextprotocol/servers/tree/main/src/postgres",
  },
  {
    id: "sqlite",
    name: "sqlite",
    label: "SQLite",
    desc: "Local .db file access via uvx.",
    group: "Data",
    command: "uvx",
    args: ["mcp-server-sqlite", "--db-path"],
    argField: {
      label: "Database file",
      placeholder: "C:\\Users\\lucas\\data\\app.db",
      required: true,
      hint: "Appended after --db-path. Needs uv installed.",
    },
    envFields: [],
    official: false,
    docs: "https://github.com/modelcontextprotocol/servers",
  },
  {
    id: "brave-search",
    name: "brave-search",
    label: "Brave Search",
    desc: "Web + local search for the agent.",
    group: "Web",
    command: "npx",
    args: ["-y", "@modelcontextprotocol/server-brave-search"],
    envFields: [
      {
        key: "BRAVE_API_KEY",
        label: "Brave API key",
        placeholder: "BSA…",
        required: true,
        secret: true,
      },
    ],
    official: true,
    docs: "https://github.com/modelcontextprotocol/servers/tree/main/src/brave-search",
  },
  {
    id: "puppeteer",
    name: "puppeteer",
    label: "Puppeteer",
    desc: "Headless browser: navigate, click, screenshot.",
    group: "Web",
    command: "npx",
    args: ["-y", "@modelcontextprotocol/server-puppeteer"],
    envFields: [],
    official: true,
    docs: "https://github.com/modelcontextprotocol/servers/tree/main/src/puppeteer",
  },
  {
    id: "playwright",
    name: "playwright",
    label: "Playwright",
    desc: "Microsoft's browser automation (heavier than Puppeteer).",
    group: "Web",
    command: "npx",
    args: ["-y", "@playwright/mcp@latest"],
    envFields: [],
    official: false,
    docs: "https://github.com/microsoft/playwright-mcp",
  },
  {
    id: "slack",
    name: "slack",
    label: "Slack",
    desc: "Read/post messages — needs a bot token.",
    group: "Productivity",
    command: "npx",
    args: ["-y", "@modelcontextprotocol/server-slack"],
    envFields: [
      {
        key: "SLACK_BOT_TOKEN",
        label: "Bot token",
        placeholder: "xoxb-…",
        required: true,
        secret: true,
      },
      {
        key: "SLACK_TEAM_ID",
        label: "Team ID",
        placeholder: "T…",
        required: true,
        secret: false,
      },
    ],
    official: true,
    docs: "https://github.com/modelcontextprotocol/servers/tree/main/src/slack",
  },
  {
    id: "gdrive",
    name: "gdrive",
    label: "Google Drive",
    desc: "Docs/Sheets search — Google OAuth setup required.",
    group: "Productivity",
    command: "npx",
    args: ["-y", "@modelcontextprotocol/server-gdrive"],
    envFields: [
      {
        key: "GDRIVE_CLIENT_ID",
        label: "OAuth client ID",
        placeholder: "…apps.googleusercontent.com",
        required: true,
        secret: false,
      },
      {
        key: "GDRIVE_CLIENT_SECRET",
        label: "OAuth client secret",
        placeholder: "GOCSPX-…",
        required: true,
        secret: true,
      },
    ],
    official: true,
    docs: "https://github.com/modelcontextprotocol/servers/tree/main/src/gdrive",
  },
  {
    id: "gmail",
    name: "gmail",
    label: "Gmail",
    desc: "Community server — read/search mail via Google OAuth.",
    group: "Productivity",
    command: "npx",
    args: ["-y", "@gongrzhe/server-gmail-autoauth-mcp"],
    envFields: [],
    official: false,
    docs: "https://github.com/gongrzhe/server-gmail-autoauth-mcp",
  },
];

export const PRESET_GROUPS = ["Essentials", "Code & Dev", "Data", "Web", "Productivity"] as const;
