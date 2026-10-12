// Dev-only stand-in for the Tauri backend, so the UI runs in a plain
// browser (`npm run dev`, then open /mock.html). Used to profile the UI and
// to run `npm run lint:ui`. Only mock.html loads this; index.html and the
// app build never do.
import { mockConvertFileSrc, mockIPC, mockWindows } from "@tauri-apps/api/mocks";

const params = new URLSearchParams(location.search);
// ?view=home | session | settings | brain | history | setup (default: session)
const view = params.get("view") ?? "session";
// ?turns=N sizes the open session's transcript (default 40).
// ?bg=1 shows a wallpaper, served at http://asset.localhost/ (route it to a file).
const turns = Math.max(1, Number(params.get("turns")) || 40);

const now = Date.now();
const iso = (ago: number) => new Date(now - ago).toISOString();

const sessions = [
  { id: "s1", title: "Fix the composer freeze", lane: "build", model: "claude/claude-opus-5-5", status: "idle" },
  { id: "s2", title: "Reading list for compilers", lane: "work", model: "codex/gpt-5", status: "done" },
  { id: "s3", title: "Refactor the sync hub", lane: "build", model: "claude/claude-sonnet-5-5", status: "active" },
  { id: "s4", title: "Draft release notes", lane: "work", model: "opencode", status: "done" },
].map((s, i) => ({
  pinned: false,
  project: i === 0 ? "parzi" : "",
  tokens_in: 12000 + i * 900,
  tokens_out: 4000 + i * 300,
  cost_usd: 0.42 + i / 10,
  context_tokens: 48000,
  context_limit: 200000,
  cwd: "D:\\Repos\\Parzi",
  parent_id: null,
  created: iso((i + 2) * 3_600_000),
  updated: iso((i + 1) * 600_000),
  ...s,
}));

const ANSWER = [
  "## What I found",
  "",
  "The freeze comes from work that runs on every keystroke. Here is what I found:",
  "",
  "- The composer measures its own height twice per key.",
  "- Each draft change re-renders the **model picker** and the project chip.",
  "- The thread re-parses markdown when nothing changed.",
  "",
  "```ts",
  "export function autosize(el: HTMLTextAreaElement) {",
  "  el.style.height = \"auto\";",
  "  el.style.height = `${Math.min(el.scrollHeight, 180)}px`;",
  "}",
  "```",
  "",
  "| Step | Before | After |",
  "| --- | --- | --- |",
  "| Keystroke | 38 ms | 4 ms |",
  "| Paste 2 KB | 210 ms | 9 ms |",
].join("\n");

function transcript() {
  const out: unknown[] = [];
  for (let i = 0; i < turns; i++) {
    out.push({ kind: "user", text: `Turn ${i + 1}: look at why typing stalls, and check the thread too.` });
    out.push({ kind: "reasoning", text: "Checking the composer and the thread renderer." });
    out.push({ kind: "tool_call", id: `t${i}`, name: "fs.read", args: { path: "ui/src/lib/Omnibar.svelte" } });
    out.push({ kind: "tool_result", id: `t${i}`, name: "fs.read", ok: true, output: "ok", ms: 12 });
    out.push({ kind: "assistant", text: ANSWER, done: true });
  }
  return out;
}

const models = (ids: string[]) => ids.map((id, i) => ({ id, name: "", is_default: i === 0, efforts: ["low", "medium", "high"] }));
const board = [
  { provider: "claude", state: "ready", version: "2.4.0", account: "dev@example.com", hint: "", usage: [{ label: "5h", used_percent: 22 }], models: models(["claude-opus-5-5", "claude-sonnet-5-5"]), gated: false, checked_at: now / 1000 },
  { provider: "codex", state: "ready", version: "0.9.1", hint: "", usage: [], models: models(["gpt-5", "gpt-5-mini"]), gated: false, checked_at: now / 1000 },
  { provider: "opencode", state: "signed_out", hint: "Run opencode auth login", usage: [], models: [], gated: false, checked_at: now / 1000 },
  { provider: "grok", state: "not_installed", hint: "", usage: [], models: [], gated: true, checked_at: now / 1000 },
  { provider: "antigravity", state: "unchecked", hint: "", usage: [], models: [], gated: true, checked_at: now / 1000 },
  { provider: "cursor", state: "disabled", hint: "", usage: [], models: [], gated: true, checked_at: now / 1000 },
];

const config = {
  version: 1,
  providers: { claude: { enabled: true, default_model: "claude/claude-opus-5-5" }, codex: { enabled: true } },
  lanes: { default_mode: "supervised", default_allowed_tools: [], max_steps: 60 },
  mcp: { servers: {} },
  orchestrator: { max_concurrent: 4, mcp_idle_kill_secs: 600, queue_when_busy: true },
  routing: { order: ["claude", "codex"] },
  budget: { max_cost_usd: null, max_tokens: null },
  favorite_models: ["claude/claude-opus-5-5"],
};

const theme = {
  font: { family: "Inter", size: 14, mono: "JetBrains Mono", mono_size: 13 },
  colors: { sidebar: "#0b0b10", stage: "#0b0b10", accent: "#e6e8ee", text: "#ededf2", text_dim: "#9aa0ae", bar: "#0b0b10", border: "#24242c" },
  background: { image: params.get("bg") ? "backgrounds/mock.png" : "", dim: 0.66, vignette: 0.5, blur: 0, grain: 0.16, grain_size: 32, height: 1 },
};

const project = { slug: "parzi", title: "Parzi", folder: "D:\\Repos\\Parzi", note: "projects/parzi.md", notes: ["notes/style.md", "notes/release.md"] };
const notes = [
  { path: "notes/style.md", title: "Writing style", projects: ["all"], tags: [], pinned: true, links: [], modified: now / 1000, bytes: 1200, summary: "Short sentences. No em dashes." },
  { path: "notes/release.md", title: "Release checklist", projects: ["parzi"], tags: [], pinned: false, links: [], modified: now / 1000, bytes: 900, summary: "Bump versions, tag, attach installers." },
  { path: "projects/parzi.md", title: "Parzi", projects: [], tags: [], folder: "D:\\Repos\\Parzi", pinned: false, links: [], modified: now / 1000, bytes: 300, summary: "The harness itself." },
];

const browserPrefs = {
  devtools: false,
  agentSignIns: false,
  windows: true,
  sites: [
    { origin: "https://meet.google.com", kind: "camera", allow: true },
    { origin: "https://meet.google.com", kind: "microphone", allow: true },
    { origin: "https://news.example.com", kind: "notifications", allow: false },
  ],
};

const downloadsList = [
  { id: "d1", tab: "t", url: "https://example.com/report.pdf", name: "report.pdf", path: "C:\\Users\\dev\\Downloads\\report.pdf", received: 2_400_000, total: 2_400_000, state: "done", started: now - 600_000 },
  { id: "d2", tab: "t", url: "https://example.com/big.zip", name: "big.zip", path: "C:\\Users\\dev\\Downloads\\big.zip", received: 31_000_000, total: 88_000_000, state: "progress", started: now - 60_000 },
  { id: "d3", tab: "t", url: "https://example.com/old.iso", name: "old.iso", path: "C:\\Users\\dev\\Downloads\\old.iso", received: 0, total: 0, state: "failed", started: now - 90_000_000 },
];

const handlers: Record<string, (a: Record<string, unknown>) => unknown> = {
  app_version: () => "0.1.25-dev",
  list_threads: () => sessions,
  get_thread: (a) => [sessions.find((s) => s.id === a.id) ?? sessions[0], transcript()],
  provider_statuses: () => board,
  refresh_providers: () => board,
  get_config: () => config,
  get_theme: () => theme,
  get_theme_css: () => "",
  background_url: () => (params.get("bg") ? "C:/mock/wallpaper.png" : ""),
  list_background_urls: () => [],
  list_pack_infos: () => [{ name: "Default", builtin: true, colors: theme.colors, has_art: false }],
  // ?remote=1 links a server.
  remote_info: () =>
    params.get("remote")
      ? { label: "box", user: "lucas", host: "box.example.net", version: "0.1.27", linked_at: now, alive: true, synced_at: now - 180_000, device: "pc" }
      : null,
  remote_sync: () => "synced",
  remote_providers: () => [],
  remote_projects: () => [],
  brain_context: () => ({ project, text: "", attached: [], listed: [], chars: 0, tokens: 1840 }),
  brain_projects: () => [
    project,
    { slug: "memory-ai", title: "memory-ai", folder: "D:\\Repos\\memory-ai", note: "projects/memory-ai.md", notes: [] },
    { slug: "thesis", title: "Thesis", folder: "D:\\Uni\\thesis", note: "projects/thesis.md", notes: ["notes/style.md"] },
  ],
  brain_list: () => notes,
  brain_read: (a) => `# ${String(a.path)}\n\nA note for the mock backend.\n`,
  brain_dir: () => "C:\\Users\\dev\\.parzi\\brain",
  brain_obsidian: () => "missing",
  git_branch: () => "main",
  git_status: () => ({ branch: "main", upstream: true, ahead: 2, behind: 0, changed: 3, untracked: 1, last_subject: "matte surfaces, one icon size", last_time: Math.floor(now / 1000) - 3600 }),
  list_files: (a) => ["src/App.svelte", "src/lib/Omnibar.svelte", "src/lib/Thread.svelte"].filter((f) => f.includes(String(a.query ?? ""))),
  search_suggest: (a) => [`${a.query} docs`, `${a.query} tutorial`],
  run_doctor_quick: () => [{ name: "Sessions folder", ok: true, detail: "~/.parzi/sessions" }],
  vault_state: () => ({ status: "missing", email: "" }),
  adblock_state: () => ({ enabled: true, allowed: false, host: "", supported: true }),
  browser_prefs: () => browserPrefs,
  browser_set_pref: (a) => {
    if (a.name === "devtools") browserPrefs.devtools = !!a.on;
    if (a.name === "agentSignIns") browserPrefs.agentSignIns = !!a.on;
    return browserPrefs;
  },
  browser_forget_site: (a) => {
    browserPrefs.sites = browserPrefs.sites.filter((s) => !(s.origin === a.origin && (!a.kind || s.kind === a.kind)));
    return browserPrefs;
  },
  browser_downloads: () => downloadsList,
  browser_downloads_clear: () => downloadsList.filter((d) => d.state === "progress"),
  onboard_scan: () => ({ browsers: [], tools: [], folders: [] }),
  send_message: () => "s1",
};

// Backend events (`listen`) are mocked too, so subscriptions resolve.
mockWindows("main");
mockConvertFileSrc("windows");
mockIPC((cmd, payload) => handlers[cmd]?.((payload ?? {}) as Record<string, unknown>) ?? null, { shouldMockEvents: true });

try {
  localStorage.setItem("parzi.onboarded.v1", view === "setup" ? "false" : "true");
  const composer = { input: "", mode: "build", model: "claude/claude-opus-5-5", effort: "medium", attachments: [] };
  const tab = (id: string, kind: string, title: string, sessionId: string | null = null) => ({ id, kind, title, sessionId, cwd: "D:\\Repos\\Parzi", composer });
  const tabs = [tab("t-home", "session", "New session"), tab("t-s1", "session", sessions[0].title, "s1"), tab("t-s2", "session", sessions[1].title, "s2")];
  if (view === "brain") tabs.push(tab("t-brain", "brain", "Brain"));
  if (view === "history") tabs.push(tab("t-history", "history", "History"));
  // ?view=page: a web page tab (no native page here), asking for the camera.
  const saved: unknown[] = [...tabs];
  if (view === "page") saved.push({ id: "t-page", kind: "page", title: "Example", url: "https://example.com/" });
  const active = { home: "t-home", session: "t-s1", brain: "t-brain", history: "t-history", page: "t-page" }[view] ?? "t-s1";
  localStorage.setItem("parzi.tabs.v1", JSON.stringify({ tabs: saved, active }));
  const sites = [
    ["https://github.com/lucas19919/Parzi", "Parzi on GitHub"],
    ["https://docs.rs/tauri", "tauri - Rust docs"],
    ["https://svelte.dev/docs", "Svelte docs"],
    ["https://news.ycombinator.com", "Hacker News"],
  ];
  localStorage.setItem("parzi.pins.v1", JSON.stringify(sites.map(([url, title]) => ({ url, title }))));
} catch {}

export const mockView = view;
