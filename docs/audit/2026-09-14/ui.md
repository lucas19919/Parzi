# Parzi UI audit (`ui/`) — 2026-09-14, working tree incl. provider-logos lane

Scope: static read of every file listed in the brief plus cross-checks against `src-tauri/src/main.rs`, `crates/parzi-runtime`, PLAN.md §8/§9/§11, design-system MASTER.md. No cargo/npm run. Line numbers are working-tree lines.

Provider-logos lane state: clean. No import of `ui/src/assets/providers/*` survives (grep = 0), `ProviderLogo.svelte`/`providerMarks.ts` are self-contained inline SVG, and the claimed "`logos` prop plumbing" does not exist in the tree — every consumer imports `ProviderLogo`/`hasMark` directly (better than the claim; the claim text is stale). Build is not broken by the lane.

Severity legend: P0 blocker / P1 serious / P2 moderate / P3 nit. Effort S/M/L.

---

## P0

### P0-1 — `liveRun` never clears when a run finishes while another thread is open → composer locked until restart
- **File**: `ui/src/App.svelte:643-672` (`onEvent`)
- **What**: `done`/`error` for a session that is not `activeThreadId` hits the early return at 647-650 before the `liveRun = null` at 663.
  ```ts
  if (e.session !== activeThreadId) { loadThreads(); return; }   // line 647
  ...
  } else if (e.kind === "done" || e.kind === "error") { ... liveRun = null; ... }  // line 661
  ```
  `send()` guards on `liveRun` (266), Omnibar `streaming={!!liveRun}` (951/990), Esc → `stopRun()` (831). Nothing else resets `liveRun` except `handleDeleteThread` and the "queued" branch of `send()`.
- **Consequence**: start a run in thread A, click thread B in the sidebar, wait. Stop button stays red everywhere, placeholder says "Agent is working…", Enter does nothing, `/kill` errors ("not running"). Only deleting thread A or restarting recovers. Also: background-session `error` events never toast (662 is only reached for the active session) — silent failure in a MASTER-forbidden way.
- **Fix**: handle `done`/`error` per session before the active-session gate: `if (e.session === liveRun) { liveRun = null; }` and `if (e.kind === "error") toast(...)` regardless of which thread is open. Better: track `liveRuns: Set<string>` and derive `streaming = activeThreadId ? liveRuns.has(activeThreadId) : false`. **S**

---

## P1

### P1-1 — No CSP anywhere; DOMPurify allows remote `http(s)` images → tracking/IP exfil from model output
- **Files**: `ui/index.html` (no `<meta http-equiv="Content-Security-Policy">`), `src-tauri/tauri.conf.json:13-17` (`security` has only `assetProtocol`, no `csp`), `ui/src/lib/md.ts:97-101`
  ```ts
  ALLOWED_URI_REGEXP: /^(?:(?:https?|parzi):|[^a-z]|[a-z+.-]+(?:[^a-z+.\-:]|$))/i,
  ```
- **What**: PLAN §8 says "`parzi://` images only". The regexp applies to both `href` and `src`, so `![x](https://evil.tld/p.png?id=…)` in any assistant message, tool output, artifact or DocReader file loads at render time. With no CSP the webview would also execute any script that ever slipped past DOMPurify with full IPC access. Relative URLs (`[^a-z]` branch) are also allowed: `[x](/settings)` or `<img src="/x">` navigates the webview to `devUrl/…` in dev.
- **Consequence**: any model/tool/MCP server can beacon the user's IP and timing by emitting an image; a DOMPurify bypass would be total compromise (read/write_text_file are unscoped IPC, see P2-9).
- **Fix**: (1) set `app.security.csp` in tauri.conf.json: `default-src 'self'; img-src 'self' asset: http://asset.localhost parzi: data:; style-src 'self' 'unsafe-inline'; font-src 'self'; connect-src ipc: http://ipc.localhost; script-src 'self'` (Tauri injects nonces). (2) In md.ts use a DOMPurify `uponSanitizeAttribute` hook: allow `src` only for `parzi:`/`asset:` schemes, allow `href` only for `https?:`/`parzi:`, drop relative URLs. **S**

### P1-2 — External links in rendered markdown navigate the app window away (no interception, no `on_navigation`)
- **Files**: `ui/src/lib/Thread.svelte:59-77` (`onThreadClick` only handles `[data-copy]`/`[data-expand]`), `ui/src/lib/inspector/DocReader.svelte:112-121`, `src-tauri/src/main.rs` (grep `on_navigation|navigation` = 0 hits), `src-tauri/src/main.rs:1224-1234`
- **What**: markdown-it `linkify: true` turns every URL into `<a href>`; DOMPurify keeps it; nothing calls `preventDefault`. Tauri v2 has no navigation guard here, so WebView2 loads the target inside the frameless main window. `open_external_url` exists but its allowlist is only the two Parzi GitHub prefixes (1225-1228), so even the UI's own "docs" buttons are rejected (see P1-6).
- **Consequence**: click any link in a model reply → app replaced by a web page, no back button, no chrome; user must kill the process. Verify in the built app, but the code has no defence.
- **Fix**: delegated click handler in Thread/DocReader/Widget/ArtifactCard: `const a = el.closest("a[href]"); if (a) { e.preventDefault(); api.openExternalUrl(a.href) }`; widen `open_external_url` to any `https://` with a toast ("Opened in browser"); add `.on_navigation(|u| u.scheme()=="tauri" || u.host()==Some("localhost"))` on the window builder as belt-and-braces. **S**

### P1-3 — `install_pasted_skill` argument name mismatch: paste-install and "Create skill" are dead
- **File**: `ui/src/lib/api.ts:343-344`
  ```ts
  installPastedSkill: (pack_name: string, text: string) =>
    invoke<InstalledSkill>("install_pasted_skill", { pack_name, text }),
  ```
  vs `src-tauri/src/main.rs:879-883` `async fn install_pasted_skill(pack_name: String, text: String)` with no `#[tauri::command(rename_all = "snake_case")]` (only `rename_all` in the file is the serde one on line 26).
- **What**: Tauri v2 expects camelCase keys by default (`packName`). Callers: `SkillsSection.svelte:80` (Add skill) and `:127` (Create skill).
- **Consequence**: both buttons fail with "invalid args `packName` for command `install_pasted_skill`: … missing required key packName", shown as `pasteErr`. Every other invoke was cross-checked and matches (`sessionId`/`parentId`/`base64Data` are correctly camelCased).
- **Fix**: `{ packName: pack_name, text }`. Add the contract test in "Tests to add" so this class of bug cannot recur. **S**

### P1-4 — Installed "Skills" are consumed by nothing (fake feature)
- **Files**: `ui/src/lib/Omnibar.svelte:399-415` (hardcoded 11 built-in slash items), `crates/parzi-runtime/src/plugins.rs:93` (`slash_commands()` has zero callers outside the file), `src-tauri/src/main.rs:850` (`commands_for` used only to count), grep `strip_prefix("/")|starts_with("/")` across crates/src-tauri = 0.
- **What**: Settings › Skills installs `commands` packs and shows "N slash commands", but the composer's `/` menu never lists them and no runtime path expands `/name` into its prompt. Typing `/review` sends the literal string "/review" to the model.
- **Consequence**: 357-line settings page + git clone flow that changes nothing the user can observe. Not in PLAN §9 either.
- **Fix**: either (a) add `list_slash_commands` IPC, merge into `slashItems`, and on pick substitute `cmd.prompt` into `input`; or (b) remove the Skills page until the runtime consumes packs. **M**

### P1-5 — Global Enter/Esc handlers fight each other: Enter anywhere sends the draft; Esc from any Omnibar/Sidebar popup kills the run
- **Files**: `ui/src/lib/Omnibar.svelte:667` (`<svelte:window on:keydown={handleKeydown}>`), `:329-342`; `ui/src/App.svelte:826-832`; `ui/src/lib/Sidebar.svelte:297-301`
  ```ts
  // Omnibar 330-342: window-level Enter
  if (e.key === "Enter" && !e.shiftKey) { ...placeholder check never matches (see P3)...
    e.preventDefault(); ... else if (!streaming && input.trim()) dispatch("send"); }
  // App 826-832
  } else if (e.key === "Escape") { if (showNewWs) ... else if (palette) ... else if (showSettings) ...
    else if (rightBarOpen && ...) ... else if (liveRun) stopRun(); }
  ```
- **What**: (a) Enter pressed in the Ctrl+K palette input, the sidebar "Filter threads" input, or the new-workspace name field also fires `dispatch("send")` when the composer holds text (palette Enter both runs the item and sends). (b) Esc with the model picker / effort / permission / slash / @ popups or the sidebar context menu open closes them in the child AND falls through to `stopRun()` in App — the run is killed. Only `ThreadRow.onRenameKey` stops propagation.
- **Consequence**: accidental sends; accidental kills of long runs when closing a menu.
- **Fix**: move Enter/Esc handling off `window` in Omnibar onto the textarea; in App's Esc branch check `e.defaultPrevented`/a shared `popupOpen` store before `stopRun()`; child handlers call `e.stopPropagation()` after consuming Esc. **S**

### P1-6 — Composer "Permission policy" menu is cosmetic; Connectors "docs" buttons silently do nothing
- **Files**: `ui/src/lib/Omnibar.svelte:26,54-59,299-303,900-909`; `ui/src/App.svelte:945-964,984-1003` (no `bind:permission`, no `on:permissionChange`); `ui/src/lib/settings/ConnectorsSection.svelte:820` + `src-tauri/src/main.rs:1225-1234`
- **What**: `permission` (Supervised / Auto-accept edits / Auto / Full access, default "Full access") is local state; App never reads it and the backend policy is `lanes.default_mode` in config. The docs button calls `api.openExternalUrl(p.docs)` whose targets (`github.com/modelcontextprotocol/…`, `microsoft/playwright-mcp`, `gongrzhe/…`) are outside the Rust allowlist → rejected promise, no catch, no toast.
- **Consequence**: user sets "Supervised" and believes commands will ask; they run under whatever config says. Docs button is inert.
- **Fix**: bind `permission` to `cfg.lanes.default_mode` (map supervised→ask, full→auto, drop the two fake tiers or implement them) and persist via `save_config`; widen `open_external_url` to `https://` (P1-2) and wrap the click in try/notify. **S**

### P1-7 — No stream debounce, no virtualization, per-token full markdown re-render of the live message (PLAN §8)
- **Files**: `ui/src/App.svelte:651` (`live += e.text` per event), `ui/src/lib/Thread.svelte:285-297` (`splitSegments(liveText)` → `renderMarkdown(seg.body)` inline in template), `:195` (`{#each chronologicalItems as item, i (i)}` — index key, no windowing), `ui/src/lib/md.ts:3` (`import hljs from "highlight.js"` — full bundle; `dist/assets/index-*.js` is 1.58 MB, above the 1.1 MB PROGRESS.md already calls "KNOWN")
- **What**: every token event re-runs markdown-it + highlight.js over the whole live message (O(n²) for long replies with fences). No 60 ms coalescing, no `>500 blocks` virtualization, no unclosed-fence guard (markdown-it's default "fence to EOF" makes it mostly benign). Persisted messages are re-rendered wholesale whenever `events` is reassigned (`refreshEvents` on every `ui.show_artifact`, and on `done`), with no memo.
- **Consequence**: token p50 <100 ms and "10k-line thread scrollable" budgets are unverified and structurally unlikely; a 3k-line code reply will stutter.
- **Fix**: buffer tokens and flush with `setTimeout(…, 60)`/rAF into `live`; cache rendered HTML per message id in a `Map`; switch to `highlight.js/lib/core` + 12 registered languages; add a simple windowed list (or `svelte-virtual-list`) once `chronologicalItems.length > 500`. **M**

### P1-8 — `openThread` has no stale-response guard; rapid switching writes thread A's events into thread B
- **File**: `ui/src/App.svelte:197-224`
  ```ts
  activeThreadId = id; ... const [meta, ev] = await api.getThread(id);
  activeMeta = meta; events = ev; curProject = meta.project ...; model = meta.model
  ```
- **What**: two quick clicks (A then B) → if A resolves last, `activeThreadId === B` but `events/activeMeta/curProject/model` are A's. Also switching back to a streaming thread resets `live = ""` (199) so tokens received while away are lost until `done` reloads.
- **Fix**: `const token = ++openSeq; …; if (token !== openSeq) return;` after the await; keep per-session live buffers (`liveBySession[id]`) instead of one `live`. **S**

### P1-9 — Startup is one sequential try: any early failure skips event-listener registration
- **File**: `ui/src/App.svelte:840-856`
- **What**: `getThemeCss` → `backgroundUrl` → `ensureModels` → `loadProjects` → `loadThreads` → `onRunEvent` in one `try`. If theme CSS or the catalog throws, `onRunEvent(onEvent)` never runs: no tokens, no approvals, no done — forever. `loadProjects`/`loadThreads` swallow errors (`catch {}` at 159/173) so the sidebar shows "No conversations yet — start a new chat" when the store actually failed.
- **Fix**: register `onRunEvent` first and unconditionally; wrap each step in its own try with a specific toast; make the sidebar empty state distinguish "load failed" from "empty". **S**

---

## P2

### P2-1 — Approval card: no session, dropped on thread switch, no timeout signal, unhandled rejection on stale key
- **Files**: `src-tauri/src/main.rs:32` (`Approval { key, call }` has no `session`), `:68-73` (120 s timeout → `Deny`, no event emitted), `:326` (`Err("approval expired or unknown")`); `ui/src/App.svelte:203` (`approval = null` on `openThread`), `ui/src/lib/Thread.svelte:53-57` (`vote()` has no try/catch)
- **Consequence**: approval for a background session renders in whatever thread is open; switching threads erases the card and the run waits 120 s then is denied; after timeout the card is still shown and clicking Approve throws silently.
- **Fix**: add `session` to the Approval event and key `approvals: Record<session, …>`; emit an `approval_resolved`/timeout event and clear the card; try/catch + toast in `vote()`. **S**

### P2-2 — `route_transition` live event is ignored by the UI (no failover toast)
- **Files**: `src-tauri/src/main.rs:31` emits `RouteTransition {…}` (kind `route_transition`); `ui/src/lib/api.ts:66-76` `UiEvent` lacks it; `ui/src/App.svelte:669-671` falls to `else { loadThreads(); }`
- **Consequence**: MASTER "Toasts: every save/apply/failover/error" — failover is silent until the run ends and the persisted `route_transition` renders as a line.
- **Fix**: add the variant to `UiEvent`, `toast(\`${from} → ${to}: ${reason}\`)`. **S**

### P2-3 — Unvalidated widget/diagram payloads from inline fences can throw during render; no error boundary
- **Files**: `ui/src/lib/md.ts:110-129` (`splitSegments` only `JSON.parse`s), `ui/src/lib/widgets/Widget.svelte:8-10,48,57` (`items.slice`, `rows.slice` with `any` types), `ui/src/lib/widgets/Diagram.svelte:3-4,75` (`(n.label ?? n.id).slice` — numeric id throws)
- **What**: PLAN says widgets are "schema-checked in core", but assistant text containing a ```parzi-widget fence bypasses core entirely. `{"widget":1,"type":"list","items":5}` → `TypeError: items.slice is not a function` inside a component init; Svelte 4 has no error boundaries, so the thread render aborts.
- **Fix**: coerce with `Array.isArray(x) ? x : []`, `String(id)`; wrap segment rendering in a tiny `SafeRender` that catches and shows the source (Widget already has a `bad` path — route type errors into it). **S**

### P2-4 — MCP preset secrets go to `config.toml` in plaintext and are echoed back into the edit form; postgres password in a CLI arg
- **Files**: `ui/src/lib/settings/mcpPresets.ts:112-119,132-146,195-202,239-253,266-281`; `ConnectorsSection.svelte:232-239` (env → `servers[name].env`), `:303-314` (`persist` → `save_config`), `:517-522` (`startEdit` renders `KEY=value` into a plain textarea), `:779` ("Keys stay in your local config — never logged"), `mcpPresets.ts:159-164` (connection string as positional arg → visible in Task Manager/`ps`)
- **What**: provider keys use the OS keyring (ModelsSection 191-194); MCP tokens do not. The password inputs are theatre — the value round-trips through `get_config` and is displayed on edit.
- **Fix**: store env values with `secret:true` via `save_key("mcp:<server>:<KEY>")` and inject at spawn; show `••••` in the editor with a "replace" affordance. **M**

### P2-5 — Preset supply chain: unpinned `npx -y`, `@latest`, deprecated/nonexistent packages
- **File**: `ui/src/lib/settings/mcpPresets.ts:44-45` (`@modelcontextprotocol/server-fetch` — the official fetch server is Python-only, `uvx mcp-server-fetch`; verify against the registry, I expect a 404), `:226` (`@playwright/mcp@latest`), `:292` (community `@gongrzhe/server-gmail-autoauth-mcp`), all others `npx -y <name>` with no version.
- **What**: every spawn resolves whatever is newest; several `@modelcontextprotocol/server-*` reference packages (github, gitlab, postgres, brave-search, puppeteer, slack, gdrive) are archived upstream and deprecated on npm. The scope is org-owned so typosquatting of these exact names is unlikely, but the "official" badge still points at unmaintained code.
- **Fix**: pin exact versions (`@modelcontextprotocol/server-memory@0.6.x`), fix `fetch` to `uvx mcp-server-fetch`, mark archived ones "archived", show the resolved command line before install. **S**

### P2-6 — Settings has 7 pages; PLAN §9 froze 5; IPC is 74 commands vs "10 max"
- **Files**: `ui/src/lib/SettingsNav.svelte:25-33`, `ui/src/lib/Settings.svelte:46-60`; `src-tauri/src/main.rs:1343-1417`
- **What**: General (not in PLAN), Skills (Plugins + install flows, see P1-4), Context (≈Projects, real and wired), System (≈About+updater, wired). PLAN: "Nothing ships unless it matches this doc."
- **Fix**: either amend PLAN §9 explicitly (this is a spec-governance finding, not a code bug) or fold General into System and drop Skills until P1-4 is resolved. **S (doc) / M (code)**

### P2-7 — Theme discipline: 111 colour literals in `.svelte`, 105 of them the MASTER-banned `var(--x, #fallback)` pattern
- **Counts** (`grep -c` of hex/rgba/hsl in `<style>`): `inspector/AgentVisualizer.svelte` 51, `inspector/DocReader.svelte` 40, `inspector/RightPanel.svelte` 14, `settings/ColorField.svelte` 3, `settings/AppearanceSection.svelte` 3, `settings/Slider.svelte` 1, `Sidebar.svelte` 1, `Omnibar.svelte` 1, `App.svelte` 1.
- **Real hardcodes (not fallbacks)**: `App.svelte:1167` `rgba(0, 0, 0, var(--parzi-vignette))`; `Omnibar.svelte:954` `rgba(0,0,0,0.75)` thumb gradient; `Sidebar.svelte:631` `.btn.danger-solid { color: #fff }` (should be `--accent-ink`-style ink token); `AgentVisualizer.svelte:345` `background: rgba(0,0,0,0.3)`; `Slider.svelte:38`, `ColorField.svelte:42`, `AppearanceSection.svelte:561` `rgba(0,0,0,0.25/0.4)` shadows. `theme.css:16` imports `highlight.js/styles/github-dark.css` — a fixed palette for code that ignores the seven theme colours (light themes get dark-on-dark spans).
- **Fix**: strip every `, #…`/`, rgba(…)` fallback in the three inspector files (tokens always exist — `theme.css :root` defines them); add `--shadow-ink` / `--scrim` tokens for the black rgba uses; derive an hljs palette from role tokens. **M**

### P2-8 — Emoji/Unicode glyphs used as icons (MASTER: "SVG icons only")
- **Files**: `ThreadRow.svelte:178,186,193,201` (✕ ★ ☆ ✎ 🗑), `Thread.svelte:227-231,263,266` (◆ ⓘ ⇄ ✓ ✗ ▾ ▸), `ProjectOverview.svelte:155,162,201` (✕ +), `ModelsSection.svelte:298-299,410,412,421` (▲ ▼ ▸ ◉ ★), `Omnibar.svelte:800,863` (★ ☆), `SkillsSection.svelte:253`, `ConnectorsSection.svelte:864`.
- **Fix**: route through `Icon.svelte` paths (the trash/pencil/star paths already exist in Omnibar's `I` map). **S**

### P2-9 — Optimistic clears without rollback; silent catches
- **Files**: `App.svelte:268-270` (`input = ""; attachments = []` before `await api.sendMessage`; on throw the prompt is gone), `Sidebar.svelte:168-176,185-195,197-201` (`catch {}` on pin/rename/copy), `Omnibar.svelte:305-309` (`toggleFav` swallows), `App.svelte:156-174,600-608,561-567` (list loads swallow).
- **Fix**: restore `input`/`attachments` in the catch; every catch → `toast(String(e), true)`. **S**

### P2-10 — `read_text_file`/`write_text_file` are unscoped IPC exposed to the renderer
- **Files**: `src-tauri/src/main.rs:1060-1075,1155-1170` (size caps only), `ui/src/lib/api.ts:349-351`, callers `App.svelte:519`, `ContextSection.svelte:85,104,163`
- **What**: the UI only passes paths it got from `list_project_docs` or the native picker, so today's exposure is limited; but combined with no CSP (P1-1) the blast radius of any HTML injection is the whole filesystem. Rust auditor should own the fix; noting the UI contract here.
- **Fix**: scope to `currentRoot`/`~/.parzi/projects` or move the picker into Rust (return content, not path). **M**

### P2-11 — "Report issue" URL is probably truncated at the first `&` by `cmd /C start`
- **Files**: `SystemSection.svelte:143-148` builds `…/issues/new?title=…&body=…`; `main.rs:1236-1238` runs `cmd /C start "" <url>` — Rust's `Command` does not quote an arg without spaces, and cmd treats a bare `&` as a command separator.
- **Consequence**: browser opens with only the title; `body=…` is executed as a (failing) cmd command. Verify on the machine; if confirmed, `.raw_arg(format!("\"{url}\""))` or use `ShellExecuteW`/the opener plugin. **S**

### P2-12 — Zero UI tests, no test runner, no CI gate for the IPC contract
- **File**: `ui/package.json` (scripts: dev/build/check only). See "Tests to add".

---

## P3

- **P3-1** `App.svelte:358` unreachable `break;` after the `effort` block; `:135` `modelMenu` and `:689-694` `filteredModelOptions` are dead reactive state; `:952-953,991-992` `currentTask={null} currentSubfolder={null}` always null. **S**
- **P3-2** `Omnibar.svelte:332` the model-search Enter guard checks `placeholder.includes("Search models")` but placeholders are "Search Claude models…"/"Search all models…"/"Starred models" — never matches; only `onModelKey`'s `stopPropagation` (575) saves it. Delete the check. **S**
- **P3-3** Ctrl+K palette: `App.svelte:1084-1118` input is not focused on open (typing goes to the composer), `.palette-wrap` has no click-outside close, no `role="dialog"`/`aria-modal`/`aria-activedescendant`; PLAN lists "lanes + 5 settings" entries — lanes absent, one Settings entry. **S**
- **P3-4** Icon buttons rely on `title` for their accessible name (`aria-label` appears 8 times in the whole tree); `ws-backdrop` div (`App.svelte:1052`) has click but no key/role. Titlebar window buttons `tabindex="-1"` (147-153) are unreachable by keyboard (Alt+F4 exists, acceptable but document it). **S**
- **P3-5** `ThreadRow.svelte:10-12,121-130` `hasKids/kidCount/shut/toggleTree` are never passed by Sidebar — dead tree UI. `:48-52` ticker only starts if the row mounts already `active`. **S**
- **P3-6** `Sidebar.svelte:110-126` renders every thread (no 50-row cap per MASTER, no "More" per PLAN); `depthOf` (86-96) rebuilds a Map per row → O(n²). `threads` objects are mutated in the child (173,190) and only work because App shares the array. **S**
- **P3-7** `Thread.svelte:272` and `ArtifactCard.svelte:87` wrap untrusted text in a ``` fence; output containing ``` breaks out and renders as markdown (sanitized, cosmetic). Use `~~~~` with a longer fence than any run inside the text. **S**
- **P3-8** Clipboard writes unawaited/uncaught: `Thread.svelte:48,64`, `ArtifactCard.svelte:28`, `DocReader.svelte:84,117`, `SystemSection.svelte:103` — "copied" shows even when it failed. **S**
- **P3-9** `ArtifactCard.svelte:33-41`, `DocReader.svelte:89-103` "save" uses a blob `<a download>`; WebView2 download behaviour under Tauri v2 without an `on_download` handler is unverified — test in the installed build or route through `dialog.save` + `write_text_file`. **S**
- **P3-10** Composer contract drift: PLAN §9 `[+][model v][tokens][send]` / MASTER "micro-header (workspace · branch · tokens)" — tokens exist only in the card `title` tooltip (`Omnibar.svelte:672`); "Build" mode (61-65) is identical to "Chat" (`App.svelte:273` special-cases only `plan`). **S**
- **P3-11** Three hand-maintained provider rosters: `Omnibar.svelte:95-102`, `ModelsSection.svelte:21-67`, `providerMarks.ts:82-88` — drift risk; expose one roster from Rust. **S**
- **P3-12** `Titlebar.svelte:65-74` calls `window_start_dragging` on mousedown on an element that already has `data-tauri-drag-region` — double drag start. **S**
- **P3-13** `App.svelte:836,848` `document.addEventListener("parzi:bg")` and the `onRunEvent` unlisten are never stored/called — harmless for the root component in prod, duplicates handlers under Vite HMR (double tokens in dev). Store and call in `onDestroy`. **S**
- **P3-14** Tooling: `@types/dompurify@3.0.5` is redundant (dompurify 3.4.15 ships types; the DefinitelyTyped package is deprecated); Svelte 4.2.20 / vite-plugin-svelte 3 / svelte-check 3.8.6 are a legacy stack with `^` ranges (lockfile present, so `npm ci` is reproducible — make sure CI uses `npm ci`); `svelte.config.js` uses `svelte-preprocess` where `vitePreprocess` is the vite-plugin-svelte 3 default; `vite.config.ts` has no `build.sourcemap`/`manualChunks` (hljs would split cleanly); `ui/design/` (3.0 MB incl. `asuka.jpg`, `*.dc.html`) lives under `ui/` — not bundled, but it inflates checkout and confuses "what ships". `vite-dev*.log` present on disk but gitignored (fine); `ui/dist` untracked (fine). No `window.__TAURI__` guard outside `Omnibar.svelte:597` — `npm run dev` in a plain browser toasts "Startup warning" and renders an empty shell (acceptable for a Tauri-only app; say so in README). **S**
- **P3-15** `GeneralSection.svelte:44` Retry = `location.reload()`; `SystemSection.svelte:289-297` shortcut list omits Ctrl+B / Ctrl+\ / Ctrl+Shift+D; `SettingsNav.svelte:55` shows a "/" kbd hint that is not bound. **S**
- **P3-16** Dead `api` members (never called): `migrateTasks, reparentThread, runDoctor, listAllMcpTools, listPacks, listBackgrounds, uploadBackground, backgroundFile, createSkill, skillCommands, saveSkillCommands`. Rust commands with no UI: `get_provider_health`, `reset_circuit_breaker` — a tripped circuit breaker cannot be inspected or reset from the app. **S**
- **P3-17** `Widget.svelte:36` `type === undefined` is unreachable (`type = String(...)`). `Omnibar.svelte:521-526` re-fetches the whole config on every picker open just for `favorite_models`, and favourites toggled in Settings › Models do not reach the picker until reopened (duplicated state). **S**

---

## IPC inventory (for the record)

**Commands invoked from `ui/src/lib/api.ts` (66)** — all present in `generate_handler!` (`main.rs:1343-1417`): app_version, window_minimize, window_maximize, window_close, window_start_dragging, login_antigravity, logout_antigravity, migrate_tasks, list_threads, get_thread, send_message, create_subsession, reparent_thread, rename_thread, delete_thread, list_projects, list_files, create_project, delete_project, git_branch, toggle_pin, kill_run, fork_thread, approve_tool, get_models, refresh_provider, save_key, delete_key, toggle_favorite, effort_options, get_theme_css, get_theme, reset_theme, purge_sessions, get_config, save_config, save_theme, background_url, run_doctor, run_doctor_quick, run_doctor_mcp, list_mcp_tools, list_all_mcp_tools, list_packs, list_pack_infos, delete_pack, get_user_css, save_user_css, save_pack, apply_pack, list_backgrounds, list_background_urls, set_background, upload_background, save_background_data, background_file, palette_from_background, list_plugins, toggle_plugin, install_pasted_skill, install_skill_from_git, delete_skill, open_external_url, read_text_file, write_text_file, save_project_system, create_skill, skill_commands, save_skill_commands, list_project_docs.
Arg-name check: only `install_pasted_skill` mismatches (P1-3). `send_message` (`sessionId`, `parentId`), `create_subsession`, `reparent_thread`, `save_background_data` (`base64Data`), `fork_thread` (`at`) are correct.
**Rust-only**: get_provider_health, reset_circuit_breaker.
**Events**: one channel `parzi://run-event` (`api.ts:362-364`), listened once in `App.svelte:848`; kinds handled: text, reasoning, tool_call, tool_result, notice, usage, approval, subsession_created, done, error; **emitted but untyped/unhandled**: route_transition (P2-2). Other listeners: `Titlebar.svelte:59-63` window resize (cleaned up), `<svelte:window>` in Omnibar/Sidebar (Svelte-managed), `document "parzi:bg"` in App (never removed, P3-13).

## App.svelte responsibility inventory (1334 lines, "view coordinator")
Wallpaper/version · thread list + stale-selection cleanup · models mirror of `modelRows` · projects + branch · current project/lane/thread/meta/events · live stream buffers (live, liveReasoning, liveTokens, liveCost) · composer state (input, model, effort, mode, attachments) via 5 two-way binds · settings routing + anchor scroll polling · sidebar open · new-workspace modal (6 vars) · dead model-menu state · run state (sending, liveRun, approval) · toasts · scroll container · right inspector (10 vars + localStorage) · artifacts/docs/transcript loading · swarm graph computation · per-session tool tracking · palette (query/index/3 derived lists) · global shortcuts · startup chain. That is ~18 concerns. Duplicated state: `models` vs `$modelRows`; favourites (Omnibar local vs ModelsSection cfg); `threads` mutated by Sidebar. Prop depth: App → RightPanel → AgentVisualizer/DocReader (12 props, 14 re-dispatched events); App → Sidebar → ThreadRow (14 re-dispatched events). Recommended split: `runStore` (liveRuns, per-session buffers, approvals, `onEvent` as a pure reducer — testable), `threadStore` (threads, open with seq guard), `inspectorStore`, `paletteStore`, `toastStore`.

---

## Verified good
- Every model/tool/file-supplied `{@html}` goes through `renderMarkdown` → `DOMPurify.sanitize` (`Thread.svelte:203,272,288`, `Widget.svelte:43`, `ArtifactCard.svelte:84,87`, `DocReader.svelte:214`); the two remaining `{@html}` sites (`ProviderLogo.svelte:17`, `AgentVisualizer.svelte:170`) render static local SVG marks only.
- markdown-it runs with `html: false`; `highlightAuto` is deliberately avoided (`md.ts:13-15`); `javascript:`, `data:`, `mailto:` are blocked by the URI regexp; DOMPurify's default attr list drops `target`, so no `_blank` without `noopener`.
- Diagram labels/edge labels are Svelte text interpolation inside `<text>` (escaped); approval args go through `JSON.stringify` into `{}` text.
- Caps exist: attachments 8, table rows 50, list 50, kanban 8×20, diagram 200 nodes/400 edges, chart 200 points, tool output 6 k, artifact 60 k, DocReader 200 k, read/write file size limits in Rust, swarm walk 120.
- Provider-logos lane: no dangling imports, no `logos` prop drilling, marks are inline SVG with `aria-hidden`.
- `asset:` protocol scoped to `$HOME/.parzi/backgrounds/*` and `$RESOURCE/*`; `open_external_url` allowlisted (too tight, but safe); keys saved via keyring with `type="password"` inputs.
- Theme pipeline: `applyThemeCss` always clears inline previews; `cssFontList` sanitizes font names; `ColorField` validates hex; Appearance flushes pending save on destroy; `user.css` loads last as PLAN says.
- Global `:focus-visible` ring and `prefers-reduced-motion` kill-switch in `theme.css:155-168`; App gates Svelte transitions on RM; Omnibar/SettingsNav/shared.css have RM blocks.
- Model picker: search-first, Arrow/Enter/Esc, Ctrl+1..5, no-key rows route to key settings, cached rows stay visible during refresh (`Omnibar.svelte:482-494`).
- Two-step delete with subtree counts in the sidebar context menu; Inbox undeletable; stale selection cleared after purge/delete.
- `RightPanel` grip has `role="slider"` + keyboard resize; width clamped 340–680; `localStorage` reads/writes wrapped in try/catch.
- `Titlebar` removes its resize listener; `ThreadRow` clears its interval; `updateStore` collapses concurrent checks and delays the boot check 12 s.
- `ui/dist` untracked, `vite-dev*.log` gitignored, `package-lock.json` present, `.gitignore` excludes updater keys.

## Tests to add (minimum)
1. **vitest + jsdom for `md.ts`** (S): (a) `[x](javascript:alert(1))` → no href; (b) `![x](data:image/png;base64,…)` → stripped; (c) `![x](https://evil/p.png)` → must be stripped once P1-1 lands (today this test documents the hole); (d) `![x](parzi://img/1)` → kept; (e) `<script>`/`<svg onload>`/raw HTML → dropped (`html:false` + DOMPurify); (f) `target`/`onclick` attrs absent; (g) unclosed ``` fence renders as one code block, no leaked HTML; (h) `splitSegments` with malformed JSON falls back to md; (i) diff fence tints `+`/`-` and not `+++`/`---`; (j) >30-line fence emits `data-expand` button with escaped label.
2. **IPC contract test** (S): a script (`ui/scripts/ipc-contract.test.ts`) that regex-parses `#[tauri::command]` fns from `src-tauri/src/main.rs`, camelCases their param names (respecting `rename_all`), parses every `invoke("<cmd>", {…})` in `api.ts`, and asserts (a) every command exists in `generate_handler!`, (b) every payload key matches a param. Would have caught P1-3.
3. **`onEvent` reducer tests** (S, after extracting it): done for non-active session clears `liveRun`; error for any session toasts; `route_transition` toasts; approval for session X keyed by X; text for a non-active session buffers and is shown on switch.
4. **Widget/Diagram fuzz** (S): `@testing-library/svelte` mount with `items: 5`, `rows: "x"`, `nodes: [{id: 1}]`, missing `type` — must render the `bad` source card, never throw.
5. **Keyboard tests** (S): Esc with model picker open → picker closes, `stop` not dispatched; Enter in sidebar filter with composer text → `send` not dispatched; Ctrl+K → palette input focused; Enter in palette with composer text → only the palette item runs.
6. **One end-to-end smoke** (M): Playwright against `tauri-driver` (or the dev server with a mocked `window.__TAURI_INTERNALS__`): boot → theme applied → send prompt with a mock provider streaming 500 tokens with a fence → switch thread mid-stream → wait for done → composer unlocked, tokens visible on return, no console errors; plus a 10 k-line thread scroll FPS sample to actually check the §11 budget.
