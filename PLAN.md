# Parzi — Explainer + Dev Plan (frozen)

> Lean, beautiful, Rust agent harness. Tauri GUI + headless core + CLI.
> Single source of truth. Nothing ships unless it matches this doc.

## 0. Vision

Parzi routes many models through one elegant harness. Three UI elements only:
left sidebar, stage with background, centered glass omni-bar (screenshot is the spec).
Projects contain lanes (task folders with their own `SYSTEM.md`), lanes contain threads.
A process-manager viewer (not a chat list) manages running agents.
Other harnesses talk to Parzi via CLI + files, no daemon required.

Non-goals: orchestration frameworks, plugin IDE API, vector DB, multi-window,
collaboration, Automations/PRs UI, git UI (agent handles git), full settings maze.
Cut = quality.

## 1. Architecture

```
              +--------------- ui/ (Svelte + Vite, dumb renderer) --------+
              | sidebar(search+threads) | stage | glass bar | viewer | 5x |
              | md-it+GFM+DOMPurify+shiki | widget/diagram SVG | CSS vars |
              +------------------------- Tauri IPC -----------------------+
              | 10 commands max  |  events: token/done/widget/approval    |
              +-------------------- src-tauri (glue only) ----------------+
              | Orchestrator -> AgentRun x N (max_concurrent = 4)         |
+--------+    +--^--------------^--------------^--------------------------+
| parzi- |       |              |              |
| cli    |   parzi-core     parzi-providers  parzi-runtime
| (lean) |   no net/GUI     Provider trait   handler+orch+tools/mcp/plugins
```

Dependency rule: inward only. `core` never imports providers/runtime/tauri.
`src-tauri` wires, never implements. Adding Gemini = one file in providers.

Crates (3 lib + 1 bin, no more):
- `parzi-core`: config v1, theme, store (meta/events.jsonl/session.md), lanes,
  context builder, budget, widget/diagram schema validation.
- `parzi-providers`: `Provider` trait + thin natives.
- `parzi-runtime`: handler loop, orchestrator, Tool enum, McpConnector, plugins.
- `parzi-cli`: list/show/export/send/doctor. Zero webview deps.

## 2. Disk truth

```
~/.parzi/
  config.toml            # version=1, providers, defaults, mcp servers, lanes mode
  theme.toml             # colors, font, sizes, background, glass, splash
  user.css               # optional override, loaded last
  catalog.json           # cached model list, 24h TTL
  backgrounds/*.png
  projects/<p>/SYSTEM.md, parzi.toml, lanes/<l>/{SYSTEM.md, parzi.toml}
  sessions/<id>/{meta.json, events.jsonl, session.md}
  plugins/<name>/parzi-plugin.toml
  logs/parzi.log (rotated)
```

- `events.jsonl` append-only truth. Never mutate; add new `kind`.
- `session.md` rendered view for humans + other harnesses.
- `meta.json`: {id, title, project, lane, model, status, tokens_in/out, cost_usd, updated}.
- All writes atomic (tmp + rename). Config carries `version=1`, one `migrate()` fn.

`config.toml` sketch:
```toml
version = 1
default_provider = "anthropic"
[providers.anthropic] default_model = "claude-sonnet-4"
[lanes] default_mode = "ask"   # auto|ask|deny
[mcp.servers.fs] command="npx" args=[..] allow=["read"] timeout_ms=30000
```

`theme.toml` sketch:
```toml
[font] family="Inter" size=14 mono="JetBrains Mono" mono_size=13
[colors] sidebar="#0A0A0C" accent="#7C8CFF" text_dim="#9AA0AE"
[background] image="backgrounds/rooftop.png" dim=0.62 vignette=0.45
# background = one constant picture behind the app (not a splash window).
# per-project override allowed. --low-mem replaces it with a solid color.
[glass] opacity=0.85 radius=12 border="#2A2E3A" shadow=true
```

## 3. Providers — bespoke CLI routing

One trait, thin adapters. Shared SSE + tool-call core; only auth/endpoint deltas differ.

```rust
trait Provider: Send + Sync {
    fn id(&self) -> &'static str;                 // "claude-code"
    async fn models(&self) -> Result<Vec<Model>>; // -> catalog.json
    async fn chat_stream(&self, req: ChatReq) -> Result<TokenStream>;
    fn auth_status(&self) -> AuthStatus;           // ok|missing|expired (no secrets)
}
struct Model { id: String, context_limit: u32, price_in: f64, price_out: f64 }
```

Adapters (each one file, `Auth::resolve()` = read CLI creds read-only -> keyring -> env):
- `openai` (API key), `anthropic` (API key)
- `xai` / `grok-cli` (`https://api.x.ai/v1`, `XAI_API_KEY`, models grok-4/code)
- `openrouter`, `ollama` (base_url)
- `opencode` (`opencode serve` endpoint, read opencode auth.json)
- `codex` (Responses API, read `~/.codex/auth.json` OAuth)
- `claude-code` (Messages API + beta headers, read `~/.claude/.credentials.json`)
- `antigravity` (Google Cloud Code endpoint + GCP OAuth; isolated, may fail alone.
  Port the MIT-licensed opencode-antigravity-auth core logic with attribution:
  OAuth refresh -> account select -> endpoint fallback (daily->autopush->prod) ->
  wrap `{project, model, request}` -> Claude<->Gemini transform -> strip thinking
  blocks -> schema allowlist (const->$ref removed, const->enum) -> SSE transform ->
  session recovery via synthetic `tool_result`. TOS WARNING: unofficial, ban reports
  exist -> opt-in toggle + warning in Settings, never default-on.)
Router string: `provider/model` e.g. `claude-code/sonnet-4`. Catalog caches all.

Logos: `ui/src/assets/providers/*.svg`, monochrome, mapped by `provider.id`,
generic dot fallback. Never touch Rust for icons.

## 4. Context window

`ContextBuilder` (core, provider-agnostic):
- `budget = model.context_limit - 20% reserve_out - 10% reserve_tools`
- pinned: global SYSTEM + project SYSTEM.md + lane SYSTEM.md + latest user msg
- fill newest-first: history + @files (max 8, 12k chars each, `〈…〉` on cut)
- count v0.1: `chars/4` + catalog fudge; `count()` fn swappable for native later
- at 80%: auto-compact older half into one `checkpoint` summary, keep last 20 raw
- bar meter subtle, amber at 80%. Full = compact, never silent-truncate pinned.

## 5. Agent handler (single run)

```rust
enum RunState { Idle, Streaming, AwaitingApproval, ExecutingTool, Done, Killed }
struct AgentRun { id: SessionId, state: RunState, budget: Budget, cancel: CancellationToken }
loop {
  assemble_context(); provider.chat_stream() -> emit tokens + append events;
  on tool_call { allowlist_check()?; if ask -> AwaitingApproval (UI card);
    execute (30s timeout) -> append tool_result (tagged untrusted) -> continue }
  stop on done | max_steps(32) | budget | cancel
}
```

Every event persisted to `events.jsonl` as it happens. Stop button + `parzi kill`
cancel mid-tool. Tool outputs never treated as instructions.

## 6. Orchestrator + viewer

```rust
struct Orchestrator { runs: HashMap<SessionId, AgentHandle>, limits: Limits{max_concurrent:4} }
// spawn(lane,prompt) | focus | fork(at_step) | kill | retry
```

Viewer = table: `● | lane/thread | model logo+name | tokens/$ | last tool | elapsed`
`[Focus] [Fork] [Kill]`. Row click -> live preview + diff tab. Grouped by
project > lane. Fork clones transcript at step to new id. Rebuilds from
`meta.json` on boot. CLI mirrors: `parzi list/show/fork/kill`.

## 7. MCP + tools + plugins

```rust
trait McpConnector: Send+Sync {
  async fn start(&self,cfg: McpServerCfg)->Result<Handle>;
  async fn list_tools(&self,h:&Handle)->Result<Vec<ToolDef>>;  // cached
  async fn call_tool(&self,h:&Handle,n:&str,a:Json)->Result<Json>;
}
enum Tool { Local { name, run: fn }, Mcp { server, name } }
```

- STDIO via `rmcp` v3.2.0 v0.1 with features `client, transport-child-process,
  transport-io` (defaults lack a client — naive `cargo add rmcp` is not enough),
  +Streamable HTTP v0.2. Lazy spawn, kill after 60s idle.
- Lane allowlist `allowed-tools=["fs.read","fetch.*"]`, deny default.
- Plugins v0.1 (no code exec): `commands` pack, `theme` pack, `mcp-pack`.
  v0.2 `js` sandbox: `registerCommand/registerToolView/onThread`, capability-gated.
  Manifest `parzi-plugin.toml` {name, version, kind, entry}.

Agent widget tools (runtime, also over MCP):
`ui.show_markdown(md) | ui.show_widget(WidgetV1) | ui.show_diagram(DiagramV1)` —
schema-checked in core, appended as `widget` events, fail-safe to code block.

## 8. Markdown + widgets

Store raw md, render only in `ui/`: `markdown-it` GFM + `DOMPurify` strict
(no script, `parzi://` images only) + `shiki` (12 langs) + 60ms stream debounce +
unclosed-fence guard + virtualize >500 blocks + copy buttons + diff red/green.

Widgets (versioned, additive):
````md
```parzi-widget
{"widget":1,"type":"progress","title":"Auth","value":0.6}
```
````
Types v0.1: stat, progress, list, table, chart-line, chart-bar, kanban.
Diagrams: `{diagram:1, nodes[], edges[]}` (max 200 nodes) -> SVG, no mermaid dep.
Lane SYSTEM docs show 10-line usage example.

## 9. UI contract

- Tauri v2, ONE window (`main`, `transparent:true`, frameless). No splash window:
  the app shows one constant background picture (`theme.toml [background].image`,
  cached 1080p texture) under everything. `--low-mem` = solid bg, no shadow.
- Glass bar only: `backdrop-filter: blur(18px) saturate(1.2)`, never fullscreen blur.
- Sidebar: search filter top-left (substring over meta index, recent first),
  New thread, threads grouped, More. No PRs/Automations rows.
- Omni-bar: input + `[+][model v][tokens][send]`. `/` commands, `@` files.
- Palette `Ctrl+K` (commands+threads+lanes+5 settings), `Ctrl+N`, `Esc`.
- Settings, 5 flat pages, real buttons: Appearance / Models(keys+Test+Refresh) /
  Connectors(toggle+health) / Plugins(toggle) / Projects+About(Copy diagnostics).
- IPC (10 max): list_threads, get_thread, send, kill, fork, approve_tool,
  get_models, test_provider, get_theme, save_theme. Events: token, done,
  widget, approval, status.

## 10. CLI + doctor + updates

`parzi list|show <id>|export <id> --md|send <id> msg|fork <id>|kill <id>|doctor`
`doctor` checks: config schema, key presence (no values), provider ping, MCP
spawn, disk/log perms, webview version — same fn GUI About uses.
Tauri signed updater, stable channel only. Logs `~/.parzi/logs/`, rotation.

## 11. Quality bars (enforced on every agentic task)

- `#![deny(unsafe_code)]`, clippy all+pedantic deny in CI, rustfmt, no `unwrap`
  outside tests (`thiserror` + `Result`), timeouts on all I/O, secrets never logged.
- Budgets: CLI idle <25MB, GUI idle target <150MB, cold start <1.5s, token
  emit p50 <100ms, 10k-line thread scrollable.
- Tests per task: `cargo test -p <crate>` + `cargo clippy -- -D warnings`.
  Provider adapters mocked (no live keys in CI). Store tests use tempdir.
- Files <400 lines; split on growth. Trait boundaries over `if provider==`.
- Release: `opt-level="z"`, `lto=true`, `strip=true`.

## 12. Agentic build order (do in order, one phase per agent)

- P0 skeleton: workspace + 3 crates + cli + tauri conf + ui hello + `doctor` stub.
  Accept: `cargo check`, `parzi doctor` runs, app opens main window.
  (tauri-cli NOT installed yet — `cargo install tauri-cli --locked` at P0 start.)
- P1 core store: config/theme load+save atomic, sessions meta/events/md,
  lanes scan, context builder + counter test. Accept: round-trip tests.
- P2 providers: trait + openai + anthropic streaming mocked, catalog cache,
  keyring+env auth. Accept: mocked stream test, `Test` button green on mock.
- P3 bespoke adapters: xai/openrouter/ollama, then opencode/codex/claude-code/
  antigravity behind `auth_status` (no live creds in CI). Accept: status matrix.
- P4 runtime: handler loop + budget + cancel + approvals enum + orchestrator
  spawn/kill/fork + limits. Accept: kill mid-tool test, fork equality test.
- P5 MCP+tools: STDIO spawn/list/call cache, idle kill, allowlist deny test.
- P6 CLI: list/show/export/send/fork/kill/doctor real. Accept: scripted session.
- P7 UI baseline: sidebar+search+bar+theme vars+user.css+background picture+glass+palette.
  Accept: screenshot parity review, low-mem toggle.
- P8 md+widgets: reader hardening + 7 widget types + diagram SVG + 3 agent tools.
- P9 settings+updates+logs: 5 pages wired to tomls, updater, Copy diagnostics.
- P10 hardening: compact test, 10k-line perf, clippy/test green, release sizes.

Each phase ends with `cargo test` green plus the LOOP.md clippy gate
(correctness/suspicious/complexity/perf deny; style advisory).
No phase starts unless prior acceptance passes. No new crates without approval.

## 13. Spike results (verified 2026-09-09, this machine)

- WebView2/Chromium 152 present -> `backdrop-filter` glass OK. Win10 Home b26100.
- Node v24.15.0 + npm 11.12.1 present. Rust 1.98.1.
- Credential files CONFIRMED present (existence only, contents never read):
  `~/.codex/auth.json`, `~/.claude/.credentials.json`,
  `~/.local/share/opencode/auth.json`, `%APPDATA%/Antigravity/`,
  `%LOCALAPPDATA%/Antigravity/`. Auth::resolve reads these read-only.
- `rmcp` 3.2.0 + `keyring` 4.2.0 resolve on Rust 1.98.1 (168 pkgs locked).
  keyring uses windows-native store on this machine.
- T3 internal connector source not available here; the portable equivalent is
  MIT `opencode-antigravity-auth` (port with LICENSE attribution, not copy-paste).
- Fallacies caught: rmcp defaults lack client features; one SSE shape is false
  (per-adapter normalizers required); cred paths drift (degrade gracefully);
  chars/4 counting is rough (meter mitigates); max_concurrent=4 is configurable.
