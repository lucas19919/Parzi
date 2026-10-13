// Dev-only stand-in for the Tauri backend, so the UI runs in a plain
// browser (`npm run dev`, then open /mock.html). Used to profile the UI and
// to run `npm run lint:ui`. Only mock.html loads this; index.html and the
// app build never do.
import { mockConvertFileSrc, mockIPC, mockWindows } from "@tauri-apps/api/mocks";

const params = new URLSearchParams(location.search);
// ?view=home | session | settings | brain | history | setup | page (default: session)
const view = params.get("view") ?? "session";
// ?turns=N sizes the open session's transcript (default 40).
// ?bg=1 shows a wallpaper, served at http://asset.localhost/ (route it to a file).
const turns = Math.max(1, Number(params.get("turns")) || 40);

const now = Date.now();
const iso = (ago: number) => new Date(now - ago).toISOString();
const MIN = 60_000;
const HOUR = 60 * MIN;

// How a session's last run ended (RunOutcome).
const outcome = (over: Record<string, unknown> = {}) => ({
  end: "completed",
  summary: "",
  files: [] as { path: string; via: string }[],
  files_more: 0,
  checks: [] as { cmd: string; ok: boolean }[],
  cost_usd: 0.12,
  ms: 5 * MIN,
  ended: now - 30 * MIN,
  ...over,
});
const files = (...paths: string[]) => paths.map((path) => ({ path, via: "tool" }));

// Newest first, as list_threads sends them. The first four are Home's
// recent sessions: running, waiting on an approval, failed, done.
const session = (id: string, title: string, ago: number, over: Record<string, unknown> = {}) => ({
  id,
  title,
  pinned: false,
  project: "parzi",
  lane: "build",
  model: "claude/claude-opus-5-5",
  status: "idle",
  tokens_in: 12000,
  tokens_out: 4000,
  cost_usd: 0.42,
  context_tokens: 48000,
  context_limit: 200000,
  cwd: "D:\\Repos\\Parzi",
  parent_id: null as string | null,
  created: iso(ago + 2 * HOUR),
  updated: iso(ago),
  last_run: null as ReturnType<typeof outcome> | null,
  branch: "",
  effort: "medium",
  ...over,
});
const sessions = [
  session("s3", "Tidy the settings page", MIN, { status: "active", branch: "settings", model: "claude/claude-sonnet-5-5" }),
  // Waits on an approval (mockMain sends it).
  session("s9", "Speed up the thread", 2 * MIN, { status: "active", branch: "thread" }),
  session("s10", "Fix the flaky queue test", 40 * MIN, {
    last_run: outcome({ end: "failed", summary: "The test waits on a timer the queue doesn't own; I couldn't make it fail on demand yet.", ended: now - 40 * MIN }),
  }),
  session("s7", "Name sessions in a few words", 55 * MIN, {
    branch: "names",
    last_run: outcome({
      summary:
        "Sessions get a name from the first clause of the prompt, and agents rename their own session in the first turn.\n\n- `name_from_prompt` drops openers like *hey* and *can you*.",
      files: files("crates/parzi-core/src/names.rs", "crates/parzi-runtime/src/launch.rs", "crates/parzi-core/src/tools.rs", "parzi.md", "ui/src/App.svelte", "ui/src/lib/Thread.svelte"),
      files_more: 2,
      checks: [
        { cmd: "cargo test -p parzi-core names", ok: true },
        { cmd: "npm test", ok: true },
      ],
      cost_usd: 0.31,
      ended: now - 55 * MIN,
    }),
  }),
  session("s1", "Fix the composer freeze", 70 * MIN, { last_run: outcome({ summary: "Autosize measures once per key now.", ended: now - 70 * MIN }) }),
  session("s2", "Lets test something", 3 * MIN, { status: "active", lane: "work", model: "codex/gpt-5" }),
  session("s6", "Check the settings copy", 4 * MIN, { status: "active", parent_id: "s3", model: "codex/gpt-5" }),
  session("s5", "Badges from gh pr list", 90 * MIN, {
    branch: "prs",
    last_run: outcome({ summary: "PR badges read `gh pr list`; the desk caches it for a minute.", checks: [{ cmd: "cargo test --bin parzi-app", ok: true }], ended: now - 90 * MIN }),
  }),
  session("s8", "Strip the Work view", 3 * HOUR, { status: "done", last_run: outcome({ ended: now - 3 * HOUR }) }),
  session("s11", "Store run outcomes on sessions", 5 * HOUR, { last_run: outcome({ ended: now - 5 * HOUR }) }),
  session("s4", "Draft release notes", 6 * HOUR, { project: "", lane: "work", model: "opencode", status: "done" }),
  session("r:a1", "Update outdated crates in memory-ai", 8 * HOUR, { project: "memory-ai" }),
  session("r:a2", "Bump them and run the tests", 7 * HOUR, { project: "memory-ai", parent_id: "r:a1", status: "active", model: "claude/claude-sonnet-5-5" }),
];

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

// The last turn's charts: one ```viz block per type, and one it can't draw.
const viz = (spec: unknown) => ["```viz", JSON.stringify(spec), "```", ""];
const VIZ_ANSWER = [
  "Here is where the release stands.",
  "",
  ...viz({
    type: "stat",
    data: [
      { label: "Sessions done", value: 3, delta: "+2 this week" },
      { label: "Open PRs", value: 8, delta: -3, vs: "vs last week" },
      { label: "Test time", value: 41.2, unit: "s", spark: [52, 50, 49, 47, 47, 44, 43, 41.2] },
    ],
  }),
  ...viz({
    type: "bars",
    title: "Keystroke cost by step",
    unit: "ms",
    data: [
      { label: "Measure height", value: 18 },
      { label: "Model picker", value: 11 },
      { label: "Project chip", value: 6 },
      { label: "Markdown", value: 3 },
    ],
  }),
  ...viz({ type: "bars", title: "Sessions per day", data: { Mon: 4, Tue: 7, Wed: 5, Thu: 9, Fri: 6, Sat: 2, Sun: 1 } }),
  ...viz({
    type: "line",
    title: "Frame time while typing",
    unit: "ms",
    area: true,
    data: {
      labels: Array.from({ length: 12 }, (_, i) => `${i}s`),
      series: [
        { name: "Before", values: [16, 18, 17, 22, 19, 24, 21, 26, 23, 20, 25, 22] },
        { name: "After", values: [5, 4, 6, 5, 4, 5, 6, 4, 5, 5, 4, 5] },
      ],
    },
  }),
  ...viz({ type: "spark", title: "Thread render time", unit: "ms", data: [9, 8.5, 8, 7, 7.2, 6, 5.5, 5, 4.6, 4.2, 4, 3.9] }),
  ...viz({
    type: "progress",
    title: "Release checklist",
    data: [
      { label: "Issues closed", value: 3, max: 9 },
      { label: "Tests passing", value: 0.97 },
      { label: "Docs", value: 40, max: 100 },
    ],
  }),
  ...viz({
    type: "table",
    title: "Slowest renders",
    data: [
      { Component: "Thread", Renders: 1240, "Avg ms": 3.1 },
      { Component: "Omnibar", Renders: 860, "Avg ms": 1.4 },
      { Component: "HomeView", Renders: 210, "Avg ms": 0.9 },
    ],
  }),
  ...viz({
    type: "list",
    title: "Checks",
    data: [
      { label: "cargo test", detail: "workspace", value: "42s", tone: "ok" },
      { label: "npm test", value: "3s", tone: "ok" },
      { label: "lint:ui", detail: "2 findings", value: "18s", tone: "warn" },
      { label: "Release build", value: "failed", tone: "bad" },
    ],
  }),
  ...viz({
    type: "timeline",
    title: "This week",
    data: [
      { when: "2026-10-12T09:30", label: "Home cards merged", tone: "ok" },
      { when: "2026-10-12T14:05", label: "Session names in review", detail: "PR 42, checks passing", tone: "accent" },
      { when: "2026-10-13", label: "Widgets started" },
    ],
  }),
  ...viz({ type: "pie", data: [1, 2] }),
].join("\n");

function transcript() {
  const out: unknown[] = [];
  for (let i = 0; i < turns; i++) {
    // The last turn quotes a plan item, as older builds sent them.
    const last = i === turns - 1;
    const quote = `\n\n<plan-item project="parzi" id="quotes" title="Quote chips">\nClick an item to quote it into the composer.\nState: active. Needs: none. Branch: none.\n</plan-item>`;
    out.push({ kind: "user", text: last ? `Start this, and keep it small.${quote}` : `Turn ${i + 1}: look at why typing stalls, and check the thread too.` });
    out.push({ kind: "reasoning", text: "Checking the composer and the thread renderer." });
    out.push({ kind: "tool_call", id: `t${i}`, name: "fs.read", args: { path: "ui/src/lib/Omnibar.svelte" } });
    out.push({ kind: "tool_result", id: `t${i}`, name: "fs.read", ok: true, output: "ok", ms: 12 });
    out.push({ kind: "assistant", text: ANSWER, done: true });
    if (last) out.push({ kind: "assistant", text: VIZ_ANSWER, done: true });
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

const project = { slug: "parzi", title: "Parzi", folder: "D:\\Repos\\Parzi", note: "projects/parzi.md", notes: ["notes/style.md", "notes/release.conflict-laptop-macos.md", "notes/release.md"] };
const notes = [
  { path: "notes/style.md", title: "Writing style", projects: ["all"], tags: [], pinned: true, links: [], modified: now / 1000, bytes: 1200, summary: "Short sentences. No em dashes." },
  { path: "notes/release.md", title: "Release checklist", projects: ["parzi"], tags: [], pinned: false, links: [], modified: now / 1000, bytes: 900, summary: "Bump versions, tag, attach installers." },
  // A sync conflict copy (both devices edited the checklist).
  { path: "notes/release.conflict-laptop-macos.md", title: "Release checklist", projects: ["parzi"], tags: [], pinned: false, links: [], modified: now / 1000, bytes: 950, summary: "Bump versions, tag, sign, attach installers.", conflict_of: "notes/release.md" },
  { path: "projects/parzi.md", title: "Parzi", projects: [], tags: [], folder: "D:\\Repos\\Parzi", pinned: false, links: [], modified: now / 1000, bytes: 300, summary: "The harness itself." },
  { path: "widgets/open-prs.md", title: "Open pull requests", projects: [], tags: [], pinned: false, links: [], modified: now / 1000, bytes: 280, summary: "Open pull requests by repository." },
  // No title of its own: the card takes the chart's.
  { path: "widgets/release.md", title: "release", projects: [], tags: [], pinned: false, links: [], modified: now / 1000, bytes: 320, summary: "Where the release stands." },
];

// Home widgets agents wrote: Brain notes under widgets/ with a ```viz block.
const widgetNotes: Record<string, string> = {
  "widgets/open-prs.md": [
    "---",
    "title: Open pull requests",
    "---",
    "Open pull requests by repository. Refreshed when asked.",
    "",
    "```viz",
    JSON.stringify({ type: "bars", data: [{ label: "parzi", value: 4 }, { label: "memory-ai", value: 2 }, { label: "thesis", value: 1 }, { label: "dotfiles", value: 1 }] }),
    "```",
  ].join("\n"),
  "widgets/release.md": [
    "Where the release stands.",
    "",
    "```viz",
    JSON.stringify({
      type: "stat",
      title: "Release 0.1.28",
      data: [
        { label: "Items done", value: 3, delta: "+2 this week" },
        { label: "Tests", value: 412, delta: 18, spark: [361, 370, 372, 380, 388, 394, 401, 412] },
      ],
    }),
    "```",
  ].join("\n"),
};

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
  app_version: () => "0.1.27-dev",
  // The window's error reports (lib/crash.ts).
  ui_log: () => null,
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
  brain_context: () => ({ project, text: "", attached: [], listed: [], overflow: [], chars: 0, tokens: 1840 }),
  // The pinned style note is too big to send in full in Thesis sessions.
  brain_overflow: () => ({ "notes/style.md": ["thesis"] }),
  brain_resolve_conflict: () => "notes/release.md",
  brain_projects: () => [
    project,
    { slug: "memory-ai", title: "memory-ai", folder: "D:\\Repos\\memory-ai", note: "projects/memory-ai.md", notes: [] },
    { slug: "thesis", title: "Thesis", folder: "D:\\Uni\\thesis", note: "projects/thesis.md", notes: ["notes/style.md"] },
  ],
  brain_list: () => notes,
  brain_read: (a) => widgetNotes[String(a.path)] ?? `# ${String(a.path)}\n\nA note for the mock backend.\n`,
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
  const tab = (id: string, kind: string, title: string, sessionId: string | null = null, extra: Record<string, unknown> = {}) => ({
    id,
    kind,
    title,
    sessionId,
    cwd: "D:\\Repos\\Parzi",
    composer: { ...composer, ...extra },
  });
  const title = (id: string) => sessions.find((x) => x.id === id)?.title ?? "Session";
  const tabs = [tab("t-home", "session", "New session"), tab("t-s1", "session", title("s1"), "s1"), tab("t-s2", "session", title("s2"), "s2")];
  if (view === "brain") tabs.push(tab("t-brain", "brain", "Brain"));
  if (view === "history") tabs.push(tab("t-history", "history", "History"));
  // Earlier builds saved a Work tab: it is dropped on load.
  tabs.push(tab("t-work", "work", "Work"));
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
