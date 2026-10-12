// Dev-only stand-in for the Tauri backend, so the UI runs in a plain
// browser (`npm run dev`, then open /mock.html). Used to profile the UI and
// to run `npm run lint:ui`. Only mock.html loads this; index.html and the
// app build never do.
import { mockConvertFileSrc, mockIPC, mockWindows } from "@tauri-apps/api/mocks";

const params = new URLSearchParams(location.search);
// ?view=home | session | settings | brain | history | work | setup (default: session)
// ?work=none starts with no work at all.
// ?live=1 streams tool calls for the running goals (activity lines); off for
// the UI lint, which needs a still page.
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
  parent_id: null as string | null,
  created: iso((i + 2) * 3_600_000),
  updated: iso((i + 1) * 600_000),
  ...s,
}));
// The goals' helpers and other sessions, after the four Home lists.
sessions.push(
  ...[
    { id: "s5", title: "Find what runs on every keystroke", lane: "build", model: "claude/claude-sonnet-5-5", status: "done", parent_id: "s1" },
    { id: "s6", title: "Make autosize measure once", lane: "build", model: "codex/gpt-5", status: "active", parent_id: "s1" },
    { id: "s7", title: "Bump the app version to 0.1.28", lane: "build", model: "claude/claude-opus-5-5", status: "idle", parent_id: null },
    { id: "s8", title: "Add Octicons to the side panel", lane: "build", model: "claude/claude-opus-5-5", status: "done", parent_id: null },
    { id: "s9", title: "Fix the flaky queue test", lane: "build", model: "claude/claude-opus-5-5", status: "idle", parent_id: null },
    { id: "r:a1", title: "Update outdated crates in memory-ai", lane: "build", model: "claude/claude-opus-5-5", status: "idle", parent_id: null },
    { id: "r:a2", title: "Bump them and run the tests", lane: "build", model: "claude/claude-sonnet-5-5", status: "active", parent_id: "r:a1" },
  ].map((s, i) => ({ ...sessions[0], ...s, project: "parzi", created: iso((i + 6) * 3_600_000), updated: iso((i + 6) * 600_000) })),
);

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

const project = { slug: "parzi", title: "Parzi", folder: "D:\\Repos\\Parzi", note: "projects/parzi.md", notes: ["notes/style.md", "notes/release.conflict-laptop-macos.md", "notes/release.md"] };
const notes = [
  { path: "notes/style.md", title: "Writing style", projects: ["all"], tags: [], pinned: true, links: [], modified: now / 1000, bytes: 1200, summary: "Short sentences. No em dashes." },
  { path: "notes/release.md", title: "Release checklist", projects: ["parzi"], tags: [], pinned: false, links: [], modified: now / 1000, bytes: 900, summary: "Bump versions, tag, attach installers." },
  // A sync conflict copy (both devices edited the checklist).
  { path: "notes/release.conflict-laptop-macos.md", title: "Release checklist", projects: ["parzi"], tags: [], pinned: false, links: [], modified: now / 1000, bytes: 950, summary: "Bump versions, tag, sign, attach installers.", conflict_of: "notes/release.md" },
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

// Work (WORK_CONTRACT.md): one goal per band and then some. A 4-step plan
// with needs (done, running, blocked, ready), a failed check, a budget
// pause, a failure, two Later goals, finished ones with files and checks, a
// server goal and another device's goal.
const MIN = 60_000;
const HOUR = 60 * MIN;
const here = { kind: "here", name: "This PC" };
const step = (key: string, title: string, over: Record<string, unknown> = {}) => ({
  key,
  title,
  needs: [] as string[],
  lane: "build",
  status: "todo",
  note: null,
  outcome: null,
  sessions: [] as string[],
  auto: false,
  ready: true,
  blocked_by: [] as string[],
  ...over,
});
const run = (id: string, session: string, over: Record<string, unknown> = {}) => ({
  id,
  session,
  step: null,
  model: "claude/claude-opus-5-5",
  lane: "build",
  cause: "send",
  started: now - 10 * MIN,
  ended: now - 5 * MIN,
  end: "completed",
  error: null,
  ms: 5 * MIN,
  turns: 6,
  tokens_in: 42_000,
  tokens_out: 3_100,
  cost_usd: 0.12,
  summary: "",
  files: [] as { path: string; via: string }[],
  files_more: 0,
  checks: [] as { cmd: string; ok: boolean }[],
  artifacts: 0,
  ...over,
});
const goal = (id: string, title: string, over: Record<string, unknown> = {}) => ({
  id,
  title,
  status: "working",
  project: "parzi",
  root: null as string | null,
  prompt: title,
  created: now - HOUR,
  updated: now - 5 * MIN,
  engine: here,
  steps: [] as ReturnType<typeof step>[],
  running: [] as string[],
  last: null as ReturnType<typeof run> | null,
  cost_usd: null as number | null,
  files: 0,
  asks: 0,
  later: null as { auto_start: boolean } | null,
  owned: true,
  ...over,
});
const files = (...paths: string[]) => paths.map((path) => ({ path, via: "tool" }));

const planRuns = [
  run("p1", "s1", {
    started: now - 25 * MIN,
    ended: now - 24 * MIN,
    ms: 95_000,
    cost_usd: 0.18,
    summary: "I split this into four steps. Step a finds what runs on every keystroke; the others wait on what it finds.",
  }),
  run("p2", "s5", {
    step: "a",
    model: "claude/claude-sonnet-5-5",
    started: now - 23 * MIN,
    ended: now - 17 * MIN,
    ms: 6 * MIN,
    cost_usd: 0.29,
    summary:
      "Found it: the composer measures its own height twice per key, and every draft change re-renders the **model picker** and the project chip.\n\nThe thread does not re-parse markdown; that was a guess in the issue.",
  }),
  run("p3", "s1", {
    cause: "wake",
    started: now - 16 * MIN,
    ended: now - 15 * MIN,
    ms: 40_000,
    cost_usd: 0.05,
    summary: "Step a is done. Started b on the autosize fix and kept d for after it.",
  }),
  run("p4", "s6", {
    step: "b",
    model: "codex/gpt-5",
    started: now - 4 * MIN,
    ended: null,
    end: null,
    ms: 0,
    cost_usd: null,
    summary: "",
  }),
];

const doneRun = run("d1", "s8", {
  started: now - 2 * HOUR - 14 * MIN,
  ended: now - 2 * HOUR,
  ms: 14 * MIN,
  cost_usd: 0.31,
  summary:
    "Swapped the hand-drawn dock icons for Octicons and regenerated `icons.ts`.\n\n- Every icon is 16px, or 12px where Octicons draws a 12px one.\n- The panel toggle uses `split-view`.\n\nBoth checks pass.",
  files: files(
    "ui/scripts/icons.mjs",
    "ui/src/lib/icons.ts",
    "ui/src/lib/SidePanel.svelte",
    "ui/src/lib/SessionHeader.svelte",
    "ui/src/lib/TabBar.svelte",
    "ui/src/lib/TopBar.svelte",
    "ui/src/lib/Switcher.svelte",
    "ui/src/lib/HomeView.svelte",
    "ui/src/lib/Icon.svelte",
    "ui/src/theme.css",
    "README.md",
  ),
  files_more: 2,
  checks: [
    { cmd: "npm run check", ok: true },
    { cmd: "npm test", ok: true },
  ],
});

const goals = [
  goal("g-plan", "Fix the composer freeze when typing", {
    root: "s1",
    prompt: "Typing in the composer freezes the window for a moment on long threads. Find out why and fix it, then check it stays fast with 300 turns.",
    created: now - 25 * MIN,
    updated: now - 40_000,
    steps: [
      step("a", "Find what runs on every keystroke", {
        status: "done",
        sessions: ["s5"],
        outcome: "The composer measured its height twice per key and re-rendered the model picker on every change.",
      }),
      step("b", "Make autosize measure once", { status: "running", needs: ["a"], sessions: ["s6"] }),
      step("c", "Check typing stays fast at 300 turns", { needs: ["b"], ready: false, blocked_by: ["b"] }),
      step("d", "Note the fix in the changelog", { needs: ["a"], lane: "work" }),
    ],
    running: ["s6"],
    last: planRuns[3],
    cost_usd: 0.52,
    files: 2,
  }),
  goal("g-server", "Update outdated crates in memory-ai", {
    project: "memory-ai",
    root: "r:a1",
    engine: { kind: "server", name: "box" },
    prompt: "Update the outdated crates in memory-ai and make sure the tests still pass.",
    created: now - 50 * MIN,
    updated: now - 3 * MIN,
    steps: [
      step("a", "List outdated crates", { status: "done", sessions: ["r:a1"], outcome: "Seven crates are behind; two have breaking changes." }),
      step("b", "Bump them and run the tests", { status: "running", needs: ["a"], sessions: ["r:a2"] }),
    ],
    running: ["r:a2"],
    last: run("sv2", "r:a2", { step: "b", started: now - 9 * MIN, ended: null, end: null, ms: 0, cost_usd: null }),
    cost_usd: 0.37,
    files: 1,
  }),
  goal("g-sync", "Refactor the sync hub", {
    root: "s3",
    prompt: "Split the sync hub into smaller pieces so pushes and pulls stop blocking each other.",
    created: now - 12 * MIN,
    updated: now - MIN,
    running: ["s3"],
    last: run("y1", "s3", { model: "claude/claude-sonnet-5-5", started: now - 2 * MIN, ended: null, end: null, ms: 0, cost_usd: null }),
    cost_usd: 0.44,
    files: 4,
  }),
  goal("g-check", "Bump the app version to 0.1.28", {
    status: "check",
    root: "s7",
    prompt: "Bump Parzi to 0.1.28 everywhere the version lives.",
    created: now - 30 * MIN,
    updated: now - 18 * MIN,
    last: run("c1", "s7", {
      started: now - 26 * MIN,
      ended: now - 18 * MIN,
      ms: 8 * MIN,
      cost_usd: 0.21,
      summary: "Bumped the version in package.json, Cargo.toml and tauri.conf.json.\n\nOne UI test fails: `tabs.test.ts` still expects 0.1.27 in the About text.",
      files: files("ui/package.json", "src-tauri/Cargo.toml", "src-tauri/tauri.conf.json", "crates/parzi-core/Cargo.toml"),
      checks: [
        { cmd: "npm test", ok: false },
        { cmd: "cargo test --workspace", ok: true },
        { cmd: "npm run check", ok: true },
      ],
    }),
    cost_usd: 0.21,
    files: 4,
  }),
  goal("g-paused", "Draft the 0.1.28 release notes", {
    status: "paused",
    root: "s4",
    prompt: "Draft release notes for 0.1.28 from the commits since 0.1.27.",
    created: now - 70 * MIN,
    updated: now - 35 * MIN,
    last: run("n1", "s4", {
      model: "opencode",
      lane: "work",
      end: "paused",
      started: now - 60 * MIN,
      ended: now - 35 * MIN,
      ms: 25 * MIN,
      cost_usd: 2,
      summary: "I grouped the changes since 0.1.27 into Work, Browser and Fixes. Next: write the notes for each group.",
    }),
    cost_usd: 2,
  }),
  goal("g-failed", "Fix the flaky queue test", {
    status: "failed",
    root: "s9",
    prompt: "The queue test fails about one run in five. Find out why.",
    created: now - 3 * HOUR,
    updated: now - 2 * HOUR,
    last: run("f1", "s9", {
      end: "failed",
      started: now - 2 * HOUR - 20 * MIN,
      ended: now - 2 * HOUR,
      ms: 20 * MIN,
      error: "The agent stopped after 3 tool errors in a row.",
      summary: "The test waits on a timer that the queue does not own; I could not make it fail on demand yet.",
    }),
    cost_usd: 0.63,
  }),
  goal("g-later", "Clean up old screenshots in docs", {
    status: "later",
    later: { auto_start: true },
    prompt: "Remove the screenshots in docs/ that no page links to any more.",
    created: now - 3 * HOUR,
    updated: now - 3 * HOUR,
  }),
  goal("g-later2", "Try a faster markdown renderer", {
    status: "later",
    later: { auto_start: false },
    prompt: "Compare markdown-it with a faster renderer on a 300 turn thread and tell me if it is worth switching.",
    created: now - 26 * HOUR,
    updated: now - 26 * HOUR,
  }),
  goal("g-done", "Add Octicons to the side panel", {
    status: "done",
    root: "s8",
    prompt: "Use Octicons for the side panel icons, at their own pixel sizes.",
    created: now - 2 * HOUR - 15 * MIN,
    updated: now - 2 * HOUR,
    last: doneRun,
    cost_usd: 0.31,
    files: 13,
  }),
  goal("g-device", "Outline chapter two", {
    status: "done",
    project: "thesis",
    root: "m:laptop:b1",
    engine: { kind: "device", name: "laptop" },
    owned: false,
    prompt: "Outline chapter two from my notes on type inference.",
    created: now - 6 * HOUR,
    updated: now - 5 * HOUR,
    last: run("b1", "m:laptop:b1", { lane: "work", started: now - 6 * HOUR, ended: now - 5 * HOUR, ms: 18 * MIN, summary: "An outline in five sections, saved to the thesis notes." }),
    cost_usd: 0.4,
  }),
  goal("g-stopped", "Try WebGPU for the wallpaper blur", {
    status: "stopped",
    root: "s2",
    prompt: "See if the wallpaper blur can run on WebGPU.",
    created: now - 30 * HOUR,
    updated: now - 28 * HOUR,
    last: run("w1", "s2", { end: "stopped", started: now - 29 * HOUR, ended: now - 28 * HOUR, summary: "Stopped before it found a WebGPU path in WebView2." }),
    cost_usd: 0.09,
  }),
];
const runsOf: Record<string, unknown[]> = { "g-plan": planRuns };
const notesOf: Record<string, unknown[]> = {
  "g-plan": [{ at: now - 20 * MIN, by: "you", text: "Keep the fix small, no new dependencies.", step: null }],
};
// ?work=none: nobody has asked for real work yet (the empty Work tab).
const workView = () =>
  params.get("work") === "none" ? { goals: [], asks: [], digest: null, server: { linked: false, ops: true } } : { goals, asks: [], digest: null, server: { linked: true, ops: true } };

const handlers: Record<string, (a: Record<string, unknown>) => unknown> = {
  work_view: () => workView(),
  work_goal: (a) => {
    const g = goals.find((x) => x.id === a.id);
    if (!g) throw new Error(`no goal \`${String(a.id)}\``);
    return { ...g, runs: runsOf[g.id] ?? (g.last ? [g.last] : []), notes: notesOf[g.id] ?? [] };
  },
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
  if (view === "work") tabs.push(tab("t-work", "work", "Work"));
  // ?view=page: a web page tab (no native page here), asking for the camera.
  const saved: unknown[] = [...tabs];
  if (view === "page") saved.push({ id: "t-page", kind: "page", title: "Example", url: "https://example.com/" });
  const active = { home: "t-home", session: "t-s1", brain: "t-brain", history: "t-history", work: "t-work", page: "t-page" }[view] ?? "t-s1";
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
export const mockLive = !!params.get("live");
