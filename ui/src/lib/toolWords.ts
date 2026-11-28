import { commandIntent, mainCommand } from "./intentLabel.ts";

/** What sort of work a step is: steps of one sort in a row are shown as one line. */
export type ToolKind = "read" | "edit" | "search" | "list" | "run" | "web" | "browser" | "helper" | "plan" | "other";

/** One step, worded the same while it runs and once it is saved. */
export interface ToolWords {
  kind: ToolKind;
  /** "Read", "Edited", "Ran tests". */
  done: string;
  /** "Reading", "Editing", "Running tests". */
  active: string;
  /** What it worked on, short: a file name, a command, a host. May be empty. */
  target: string;
  /** The target in full, for its tooltip. */
  full: string;
  /** The target is code (a command, a pattern). */
  code: boolean;
  /** The command, for a shell step. */
  cmd: string;
}

type Args = Record<string, unknown>;

/** The agent's tool name and arguments, unwrapped: MCP prefixes go, a
 *  `use_tool` wrapper opens, nested `arguments` are read, and an agent's
 *  title ("Running view_file", "Run edit_file?") gives up its tool. */
function unwrap(rawName: string, rawArgs: unknown): { name: string; args: Args } {
  let name = (rawName || "").trim();
  let args: Args = rawArgs && typeof rawArgs === "object" && !Array.isArray(rawArgs) ? { ...(rawArgs as Args) } : {};
  if (name === "use_tool" && typeof args.tool_name === "string") {
    name = args.tool_name;
    const inner = args.tool_input;
    args = inner && typeof inner === "object" ? { ...(inner as Args) } : {};
  }
  const nested = args.arguments;
  if (nested && typeof nested === "object" && !Array.isArray(nested)) args = { ...(nested as Args), ...args };
  const own = name.replace(/^(?:mcp__parzi__|parzi__|parzi\.|parzi_)/, "");
  // Parzi's own tools come over MCP as "session_rename": the group is the first word.
  if (own !== name) name = own.replace(/^(session|fs|shell|browser|brain|plan|todo|project|ui|app|models|ask|desk|connector|image)_/, "$1.");
  const mcp = /^mcp__([^_].*?)__(.+)$/.exec(name);
  if (mcp) name = `${mcp[1]}.${mcp[2]}`;
  // Antigravity titles its calls "Running view_file" or "Run edit_file?".
  const titled = /^Run(?:ning)?\s+([a-z][\w.]*)\??$/.exec(name);
  if (titled) name = titled[1];
  return { name, args };
}

function str(args: Args, ...keys: string[]): string {
  for (const k of keys) {
    const v = args[k];
    if (typeof v === "string" && v.trim()) return v.trim();
    if (Array.isArray(v) && v.length && v.every((p) => typeof p === "string")) return v.join(" ");
  }
  return "";
}

const firstLine = (s: string, max = 80) => {
  const line = s.split("\n")[0].trim();
  return line.length > max ? `${line.slice(0, max - 1).trimEnd()}…` : line;
};

/** "D:\a\b\composer.rs" → "composer.rs"; a folder keeps its last name. */
function baseName(path: string): string {
  const parts = path.trim().replace(/[\\/]+$/, "").split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path.trim();
}

function hostOf(url: string): string {
  try {
    const u = new URL(url);
    return u.host + (u.pathname.length > 1 ? u.pathname : "");
  } catch {
    return url;
  }
}

const PATH_KEYS = ["file_path", "path", "target_file", "AbsolutePath", "TargetFile", "notebook_path", "file", "filePath"];
const DIR_KEYS = ["target_directory", "path", "dir", "directory", "DirectoryPath", "SearchPath"];
const CMD_KEYS = ["cmd", "command", "CommandLine", "commandLine"];

function words(kind: ToolKind, done: string, active: string, full = "", target = full, code = false, cmd = ""): ToolWords {
  return { kind, done, active, target: firstLine(target, 60), full, code, cmd };
}

function file(kind: ToolKind, done: string, active: string, path: string, fallback: string): ToolWords {
  return path ? words(kind, done, active, path, baseName(path)) : words(kind, `${done} ${fallback}`, `${active} ${fallback}`);
}

/** A helper by its title; a bare session id says only "a helper". */
function helperName(s: string): string {
  return !s || /^[0-9a-f-]{16,}$/i.test(s) ? "" : s;
}

function helper(done: string, active: string, name: string): ToolWords {
  const who = helperName(name);
  return who ? words("helper", done, active, who, who) : words("helper", `${done} a helper`, `${active} a helper`);
}

function shell(cmd: string, background = false): ToolWords {
  if (!cmd) return words("run", "Ran a command", "Running a command");
  const done = commandIntent(cmd, undefined, false).replace(/^Ran command$/, "Ran a command");
  const active = commandIntent(cmd, undefined, true).replace(/…$/, "").replace(/^Running command$/, "Running a command");
  const main = mainCommand(cmd);
  if (!background) return words("run", done, active, cmd, main, true, cmd);
  return words("run", done.replace(/^Ran/, "Started"), active.replace(/^Running/, "Starting"), cmd, main, true, cmd);
}

/** How a step reads in the thread. One table for every agent. */
export function describeTool(rawName: string, rawArgs: unknown): ToolWords {
  const { name, args } = unwrap(rawName, rawArgs);
  const n = name.toLowerCase();
  const path = str(args, ...PATH_KEYS);
  const cmd = str(args, ...CMD_KEYS);
  const title = str(args, "title", "description");
  const session = str(args, "title", "session", "session_id");
  switch (n) {
    // Files
    case "read":
    case "read_file":
    case "view_file":
    case "fs.read":
    case "readfile":
    case "notebookread":
      return file("read", "Read", "Reading", path, "a file");
    case "brain.read":
      return file("read", "Read", "Reading", path, "a note");
    case "edit":
    case "multiedit":
    case "edit_file":
    case "search_replace":
    case "str_replace":
    case "str_replace_editor":
    case "apply_patch":
    case "patch":
    case "notebookedit":
    case "replace_file_content":
    case "multi_replace_file_content": {
      const paths = Array.isArray(args.paths) ? (args.paths as unknown[]).filter((p): p is string => typeof p === "string") : [];
      if (paths.length > 1) return words("edit", "Edited", "Editing", paths.join("\n"), `${baseName(paths[0])} and ${paths.length - 1} more`);
      return file("edit", "Edited", "Editing", path || paths[0] || "", "a file");
    }
    case "write":
    case "write_file":
    case "create_file":
    case "write_to_file":
    case "fs.write":
      return file("edit", "Wrote", "Writing", path, "a file");
    case "brain.write":
      return file("edit", "Wrote", "Writing", path, "a note");
    case "brain.delete":
      return file("edit", "Deleted", "Deleting", path, "a note");
    // Looking around
    case "grep":
    case "search":
    case "codebase_search":
    case "grep_search":
    case "find_by_name":
    case "brain.search": {
      const q = str(args, "pattern", "query", "Query", "Pattern", "regex");
      return q ? words("search", "Searched for", "Searching for", q, q, true) : words("search", "Searched", "Searching", "the code", "the code");
    }
    case "glob":
    case "find":
    case "file_search": {
      const q = str(args, "pattern", "glob", "query", "Pattern");
      return q ? words("search", "Found", "Finding", q, q, true) : words("search", "Found files", "Finding files");
    }
    case "ls":
    case "list_dir":
    case "list":
    case "fs.list":
    case "list_directory": {
      const dir = str(args, ...DIR_KEYS);
      return dir ? words("list", "Listed", "Listing", dir, baseName(dir)) : words("list", "Listed", "Listing", ".", "the folder");
    }
    // Commands
    case "bash":
    case "shell":
    case "shell.exec":
    case "shell_exec":
    case "execute":
    case "exec_command":
    case "powershell":
    case "run_terminal_command":
    case "run_command":
    case "terminal":
      return shell(cmd);
    case "shell.start":
      return shell(cmd, true);
    case "shell.logs":
      return words("run", "Read command output", "Reading command output");
    case "shell.kill":
      return words("run", "Stopped a command", "Stopping a command");
    case "wait":
      return words("run", "Waited", "Waiting");
    // The web
    case "websearch":
    case "web_search":
    case "search_web":
    case "web search:": {
      const q = str(args, "query", "q");
      return q ? words("web", "Searched the web for", "Searching the web for", q, q) : words("web", "Searched the web", "Searching the web");
    }
    case "webfetch":
    case "web_fetch":
    case "fetch":
    case "read_url_content": {
      const url = str(args, "url", "Url", "uri");
      return url ? words("web", "Fetched", "Fetching", url, hostOf(url)) : words("web", "Fetched a page", "Fetching a page");
    }
    // Parzi's browser and window
    case "browser.open": {
      const url = str(args, "url");
      return url ? words("browser", "Opened", "Opening", url, hostOf(url)) : words("browser", "Opened a page", "Opening a page");
    }
    case "browser.click":
    case "app.click": {
      const t = str(args, "text", "selector", "label");
      return t ? words("browser", "Clicked", "Clicking", t, t) : words("browser", "Clicked", "Clicking");
    }
    case "browser.type":
    case "app.type": {
      const t = str(args, "field", "selector");
      return t ? words("browser", "Typed into", "Typing into", t, t) : words("browser", "Typed", "Typing");
    }
    case "browser.read":
      return words("browser", "Read the page", "Reading the page");
    case "browser.shot":
    case "app.screen":
      return words("browser", "Looked at the page", "Looking at the page");
    case "browser.scroll":
      return words("browser", "Scrolled", "Scrolling");
    case "browser.wait":
      return words("browser", "Waited for the page", "Waiting for the page");
    case "browser.tabs":
      return words("browser", "Listed tabs", "Listing tabs");
    case "browser.close":
      return words("browser", "Closed a tab", "Closing a tab");
    case "browser.press": {
      const k = str(args, "key");
      return words("browser", "Pressed", "Pressing", k, k, true);
    }
    // Helpers
    case "session.spawn":
    case "task":
    case "agent":
      return helper("Started", "Starting", title);
    case "session.send":
    case "session.send_message":
    case "sendmessage":
      return helper("Messaged", "Messaging", session);
    case "session.read":
    case "session.read_session":
      return helper("Checked on", "Checking on", session);
    case "session.list":
      return words("helper", "Listed helpers", "Listing helpers");
    case "session.draft":
      return title ? words("helper", "Drafted", "Drafting", title, title) : words("helper", "Drafted a session", "Drafting a session");
    // Plans
    case "plan.write":
    case "todo.write":
    case "todowrite":
    case "todo_write":
    case "update_plan":
      return words("plan", args.propose === true ? "Proposed a plan" : "Updated the plan", args.propose === true ? "Proposing a plan" : "Updating the plan");
    case "plan.read":
    case "project.plan":
      return words("plan", "Read the plan", "Reading the plan");
    // Everything else
    case "session.rename":
      return title ? words("other", "Named the session", "Naming the session", title, title) : words("other", "Named the session", "Naming the session");
    case "ask.user":
    case "askuserquestion":
      return words("other", "Asked you", "Asking you");
    case "ui.show_artifact":
      return str(args, "title", "id") ? words("other", "Published", "Publishing", str(args, "title", "id")) : words("other", "Published an artifact", "Publishing an artifact");
    case "ui.offer_replies":
      return words("other", "Offered replies", "Offering replies");
    case "toolsearch":
    case "search_tool":
      return words("other", "Looked up tools", "Looking up tools");
    case "models.list":
      return words("other", "Listed models", "Listing models");
    case "image.generate":
    case "generate_image":
      return words("other", "Made an image", "Making an image");
  }
  // A command line as the title (Antigravity names a run by its command).
  if (cmd && (n === cmd.toLowerCase() || /\s/.test(name))) return shell(cmd);
  if (/^(?:read|view|open)\b/i.test(name)) return file("read", "Read", "Reading", path, name.replace(/^\w+\s*/, "") || "a file");
  if (/^(?:edit|write|create|update|patch)\b/i.test(name)) return file("edit", "Edited", "Editing", path, name.replace(/^\w+\s*/, "") || "a file");
  if (/^(?:search|grep|find)\b/i.test(name)) return words("search", "Searched", "Searching", str(args, "query", "pattern"), str(args, "query", "pattern"), true);
  if (/^(?:browser\.|app\.)/.test(n)) return words("browser", "Used the browser", "Using the browser");
  // A connector tool: "github.pull_request_read" → "Used pull request read".
  const short = (name.split(/__|\./).pop() || name).replace(/_/g, " ").trim();
  const hint = path ? baseName(path) : str(args, "query", "url", "title", "id", "material_id");
  return words("other", `Used ${short}`, `Using ${short}`, hint, firstLine(hint, 60));
}

const NOUNS: Record<ToolKind, [string, string]> = {
  read: ["file", "files"],
  edit: ["file", "files"],
  search: ["search", "searches"],
  list: ["folder", "folders"],
  run: ["command", "commands"],
  web: ["page", "pages"],
  browser: ["step", "steps"],
  helper: ["step", "steps"],
  plan: ["update", "updates"],
  other: ["tool", "tools"],
};

/** A line for several steps of one kind: "Read 5 files", "Running 2 commands". */
export function groupLine(kind: ToolKind, items: { done: string; active: string; target: string; full: string }[], running: boolean): string {
  const distinct = new Set(items.map((i) => i.full || i.target || i.done)).size;
  const n = kind === "read" || kind === "edit" || kind === "list" ? distinct : items.length;
  const [one, many] = NOUNS[kind];
  const noun = n === 1 ? one : many;
  switch (kind) {
    case "read":
      return `${running ? "Reading" : "Read"} ${n} ${noun}`;
    case "edit":
      return `${running ? "Editing" : "Edited"} ${n} ${noun}`;
    case "search":
      return `${running ? "Searching" : "Searched"} ${n === 1 ? "once" : `${n} times`}`;
    case "list":
      return `${running ? "Listing" : "Listed"} ${n} ${noun}`;
    case "run":
      return `${running ? "Running" : "Ran"} ${n} ${noun}`;
    case "web":
      return `${running ? "Browsing" : "Browsed"} ${n} ${noun}`;
    case "browser":
      return `${running ? "Using" : "Used"} the browser`;
    case "helper":
      return `${running ? "Working" : "Worked"} with helpers`;
    case "plan":
      return running ? "Updating the plan" : "Updated the plan";
    default:
      return `${running ? "Using" : "Used"} ${n} ${noun}`;
  }
}

/** One line for a stretch of mixed work, most of it first: "Read 7 files,
 *  ran 9 commands, searched code". Up to three kinds are named. */
export function runLine(groups: { tool: ToolKind; items: { done: string; active: string; target: string; full: string }[] }[]): string {
  const byKind = new Map<ToolKind, { done: string; active: string; target: string; full: string }[]>();
  for (const g of groups) byKind.set(g.tool, [...(byKind.get(g.tool) ?? []), ...g.items]);
  const ranked = [...byKind].sort((a, b) => b[1].length - a[1].length);
  const parts = ranked.slice(0, 3).map(([kind, items]) => {
    const line = groupLine(kind, items, false);
    return line.charAt(0).toLowerCase() + line.slice(1);
  });
  const rest = ranked.slice(3).reduce((n, [, items]) => n + items.length, 0);
  if (rest) parts.push(`${rest} more step${rest === 1 ? "" : "s"}`);
  const text = parts.join(", ");
  return text.charAt(0).toUpperCase() + text.slice(1);
}
