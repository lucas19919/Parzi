const LOCAL_HOSTS = new Set(["localhost", "127.0.0.1", "[::1]"]);

function localHostOf(urls?: string[]): string {
  if (!urls?.length) return "";
  for (const raw of urls) {
    try {
      const u = new URL(raw);
      if (/^https?:$/.test(u.protocol) && LOCAL_HOSTS.has(u.hostname)) return u.host;
    } catch {
      // ignore malformed url
    }
  }
  return "";
}

/** Strips wrapper prefixes like `cd dir &&`, env vars, or `npm --prefix ui` to reach the real verb. */
function coreCommand(raw: string): string {
  let s = raw.trim();
  // Strip leading PowerShell/cmd wrappers
  s = s.replace(/^(?:cmd(?:\.exe)?\s+\/[ck]\s+|powershell(?:\.exe)?\s+(?:-\w+\s+)*)/i, "");
  // Strip leading `cd ... && ` or `cd ... ; `
  s = s.replace(/^cd\s+(?:"[^"]*"|'[^']*'|[^\s;&]+)\s*(?:&&|;)\s*/i, "");
  // Strip leading KEY=VAL env assignments
  while (/^[A-Za-z_][A-Za-z0-9_]*=(?:"[^"]*"|'[^']*'|\S+)\s+/.test(s)) {
    s = s.replace(/^[A-Za-z_][A-Za-z0-9_]*=(?:"[^"]*"|'[^']*'|\S+)\s+/, "");
  }
  return s.trim();
}

function verb(running: boolean, active: string, done: string): string {
  return running ? `${active}…` : done;
}

const RULES: { test: RegExp; active: string; done: string }[] = [
  // Git
  { test: /^git\b.*\bcommit\b/i, active: "Committing", done: "Committed" },
  { test: /^git\b.*\bpush\b/i, active: "Pushing", done: "Pushed" },
  { test: /^git\b.*\b(?:pull|fetch)\b/i, active: "Pulling", done: "Pulled" },
  { test: /^git\b.*\b(?:checkout|switch|worktree|branch)\b/i, active: "Switching branch", done: "Switched branch" },
  { test: /^git\b.*\b(?:merge|rebase|cherry-pick)\b/i, active: "Updating branch", done: "Updated branch" },
  { test: /^git\b.*\b(?:add|stage)\b/i, active: "Staging changes", done: "Staged changes" },
  { test: /^git\b.*\btag\b/i, active: "Tagging release", done: "Tagged release" },
  { test: /^git\b.*\b(?:status|diff|log|show|rev-parse)\b/i, active: "Checking git", done: "Checked git" },
  { test: /^git\b/i, active: "Running git", done: "Ran git" },
  // GitHub CLI
  { test: /^gh\s+pr\b/i, active: "Managing PR", done: "Managed PR" },
  { test: /^gh\s+issue\b/i, active: "Checking issues", done: "Checked issues" },
  { test: /^gh\s+(?:run|workflow)\b/i, active: "Checking CI", done: "Checked CI" },
  { test: /^gh\b/i, active: "Checking GitHub", done: "Checked GitHub" },
  // Dev servers
  {
    test: /^(?:(?:npm|pnpm|yarn|bun)\b.*\b(?:dev|start|serve|preview)\b|(?:npx\s+)?(?:vite|next\s+dev|nuxt\s+dev|astro\s+dev|tauri\s+dev)\b|uvicorn\b|flask\s+run\b|rails\s+s(?:erver)?\b|python[3]?\s+-m\s+http\.server\b)/i,
    active: "Running localhost",
    done: "Ran dev server",
  },
  // Tests
  {
    test: /^(?:cargo\s+(?:nextest|test)\b|(?:npm|pnpm|yarn|bun)\b.*\btest\b|node\s+--test\b|(?:npx\s+)?(?:vitest|jest|playwright|cypress)\b|pytest\b|go\s+test\b|dotnet\s+test\b)/i,
    active: "Running tests",
    done: "Ran tests",
  },
  // Checks / lint / format / typecheck
  {
    test: /^(?:cargo\s+(?:clippy|fmt|check)\b|(?:npm|pnpm|yarn|bun)\b.*\b(?:check|lint|typecheck|fmt|format)\b|(?:npx\s+)?(?:svelte-check|tsc|eslint|prettier|biome|oxlint)\b|ruff\b|mypy\b|pyright\b|flake8\b)/i,
    active: "Checking code",
    done: "Checked code",
  },
  // Builds
  {
    test: /^(?:cargo\s+build\b|(?:npm|pnpm|yarn|bun)\b.*\bbuild\b|(?:npx\s+)?(?:vite\s+build|tsc\s+-b|tauri\s+build)\b|go\s+build\b|make\b|cmake\b|gradle\b|mvn\b)/i,
    active: "Building",
    done: "Built",
  },
  // Package installs
  {
    test: /^(?:(?:npm|pnpm|yarn|bun)\b.*\b(?:install|ci|add|update)\b|pip[3]?\s+install\b|uv\s+(?:sync|pip|add)\b|cargo\s+(?:add|fetch|update)\b|brew\s+install\b)/i,
    active: "Installing packages",
    done: "Installed packages",
  },
  // Code search / file inspection
  { test: /^(?:rg|ripgrep|grep|ag|ack|Select-String|findstr|fd|find)\b/i, active: "Searching code", done: "Searched code" },
  { test: /^(?:ls|dir|Get-ChildItem|tree)\b/i, active: "Listing files", done: "Listed files" },
  { test: /^(?:cat|Get-Content|head|tail|sed|awk)\b/i, active: "Reading files", done: "Read files" },
  { test: /^(?:rm|del|Remove-Item|rmdir)\b/i, active: "Cleaning up", done: "Cleaned up" },
  { test: /^(?:curl|wget|Invoke-RestMethod|Invoke-WebRequest)\b/i, active: "Fetching URL", done: "Fetched URL" },
  { test: /^docker\b/i, active: "Running Docker", done: "Ran Docker" },
];

/** Maps a raw shell command (and optional detected URLs) to a short human-intent label. */
export function commandIntent(cmd: string, urls?: string[], running = false): string {
  const host = localHostOf(urls);
  if (host) return running ? `Running ${host}` : `Ran ${host}`;

  // For compound commands joined by ; or &&, prefer the most specific matching segment.
  const segments = cmd
    .split(/\s*(?:&&|;)\s*/)
    .map(coreCommand)
    .filter(Boolean);
  for (const seg of segments) {
    for (const rule of RULES) {
      if (rule.test.test(seg)) return verb(running, rule.active, rule.done);
    }
  }

  const first = segments[0] ?? coreCommand(cmd);
  const prog = first.split(/\s+/)[0]?.replace(/^.*[/\\]/, "").replace(/\.exe$/i, "") ?? "";
  if (!prog) return verb(running, "Running command", "Ran command");
  return verb(running, `Running ${prog}`, `Ran ${prog}`);
}

function baseName(raw: string): string {
  const clean = raw
    .trim()
    .replace(/^["'`]|["'`]$/g, "")
    .replace(/(?::\d+(?:[-:]\d+)?|#.*)$/, "");
  const parts = clean.replace(/\\/g, "/").split("/").filter(Boolean);
  return parts[parts.length - 1] ?? clean;
}

/** Turns a tool step label into a short, readable intent label for the Running card or live thread fold. */
export function stepIntent(label: string, running = false): string {
  const t = label.trim();
  if (!t) return verb(running, "Working", "Worked");

  // Shell-style tool labels (`$ cmd`, `Run cmd`, `Shell: cmd`)
  const shellMatch = /^(?:\$\s+|(?:Run|Shell|Bash|Terminal|Exec)[:\s]+)(.+)$/i.exec(t);
  if (shellMatch) {
    return commandIntent(shellMatch[1], undefined, running);
  }

  const readMatch = /^(?:Read|View|Open|Inspect)\s+(.+)$/i.exec(t);
  if (readMatch) {
    const name = baseName(readMatch[1]);
    return name ? (running ? `Reading ${name}…` : `Read ${name}`) : verb(running, "Reading file", "Read file");
  }

  const editMatch = /^(?:Edit|Write|Patch|Update|Create|Modify)\s+(.+)$/i.exec(t);
  if (editMatch) {
    const name = baseName(editMatch[1]);
    return name ? (running ? `Editing ${name}…` : `Edited ${name}`) : verb(running, "Editing file", "Edited file");
  }

  if (/^(?:Search|Grep|Find|Glob)\b/i.test(t)) {
    return verb(running, "Searching code", "Searched code");
  }

  if (/^(?:Fetch|Browse|Web|Search web)\b/i.test(t)) {
    return verb(running, "Browsing web", "Browsed web");
  }

  // Already a clean short label: add ellipsis while running if not present.
  const short = t.length > 42 ? `${t.slice(0, 41).trimEnd()}…` : t;
  return running && !short.endsWith("…") && !short.endsWith("...") ? `${short}…` : short;
}

/** True when a run-log step appears to be a shell execution matching a ShellJob row. */
export function isShellStepLabel(label: string): boolean {
  return /^(?:\$\s+|(?:Run|Shell|Bash|Terminal|Exec)[:\s]+)/i.test(label.trim());
}
