# Parzi — full audit and fix plan (2026-09-14)

Eight review agents read every file in the tree, one subsystem each, and the
orchestrator ran the gates twice (20:09 and 20:32). Per-area reports with every
finding, quoted code and line numbers live in `docs/audit/2026-09-14/`. This
file is the consolidated verdict and the plan. Nothing was fixed; nothing was
committed.

Snapshot: HEAD `4d80a72` (tag `v0.1.7`, CI still building at 20:32), 63
uncommitted files from at least five parallel lanes. The tree moved during the
audit (v0.1.7 was tagged, the providers crate was edited live), so line numbers
are from 20:10–20:25 and may be a few lines off.

Grade of this document: static review plus gate runs. The two approval-gate
defects were re-read by the orchestrator in the source; nothing was proven by
running the app. Treat every "works" below as "built, unproven" unless a test
is named.

---

## 0. Where the tree stands (measured)

| Gate | 20:09 | 20:32 | Note |
|---|---|---|---|
| `cargo test --workspace` | RED | RED | `plugins::tests::discover_and_install_library` panics (`plugins.rs:775/784`). The test installs into the real `~/.parzi/plugins`, failed to clean up, and now trips on its own leftovers (`parzi-fix-alpha`, `parzi-fix-beta` are sitting in your live plugins folder). Cargo stops at the first failing target, so the 15 runtime integration tests never ran. 64 pass / 1 fail / 15 not run. |
| clippy deny gate (workspace + src-tauri) | clean | clean | 2 rustc warnings (`handler.rs:336`, dead fns `orchestrator.rs:323/554`). |
| `cargo check` src-tauri | green | green | |
| `svelte-check` | 2 errors | 0 errors | A lane fixed the `updateStore` type errors in the working tree. HEAD still has them. 10 a11y warnings. |
| `vite build` | green | green | Main chunk 1.54 MB (PROGRESS says 1.1 MB). |
| `npm audit` | 0 | 0 | `cargo audit`/`cargo deny` not installed. |
| HEAD consistency | broken | broken | `SystemSection.svelte:148` at HEAD calls `api.openExternalUrl`; neither `api.ts` nor `main.rs` at HEAD has it. v0.1.7 ships a dead Report-issue button and no provider marks. PROGRESS at HEAD says the OpenCode overhaul is verified; `opencode.rs` is untracked. |
| IPC surface | 72 commands | 71 | PLAN cap is 10. |
| Versions | 0.1.6 | 0.1.7 | Root `package.json` says 1.0.0 / ISC. Lockfiles lag. |
| Releases | all Draft, repo private | same | Updater endpoint can never resolve. |

---

## 1. Blockers (P0) — fix before anyone else runs this

| # | What | Where | Consequence | Fix | Size |
|---|---|---|---|---|---|
| B1 | **Ask mode never waits for the human.** `ask_approver` emits `ApprovalRequest { reply: tx }` and then `select!`s on `reply_rx` beside the real approver. Both consumers drop the sender the moment they receive the event (`main.rs:224` `continue`, `cli/main.rs:295` `{}`), so `reply_rx` resolves `Err` within microseconds and the gate returns Deny. The `GuiApprover` future is cancelled, its `pending` entry leaks, the card stays on screen, and your click lands on a dead receiver. Present since the first commit. No test covers Ask mode with a slow approver. Your live config is `default_mode = "ask"` with an empty allowlist, which is why nothing has visibly exploded yet. | `handler.rs:486-500`, `src-tauri/src/main.rs:224`, `cli/main.rs:295` | Every tool call in Ask mode is denied instantly; users are pushed to Auto/`--yes`. | One approval path only: drop the second channel; the `Approver` is the gate. Test: Ask lane + approver that answers after 500 ms → tool runs exactly once after the answer. | S |
| B2 | **Harness-spawned children run with `AutoApprover`.** `QueuedRun.approver` is `None` for `session.spawn`, `session.send_message` and UI `create_subsession`; `launch` substitutes `AutoApprover`. One approved `session.spawn` card and the child runs `shell.exec`/`fs.write` with no prompts, in any lane it names (`orchestrator.rs:808`). | `orchestrator.rs:535, 646, 829, 901` | Complete policy escape from an Ask lane via delegation. | Approver is non-optional and inherited from the caller; fallback is `DenyApprover`. Child lane must exist under the caller's project and may only be stricter. Depth/child cap. | S–M |
| B3 | **The fs sandbox does not confine.** `base.join(path)` replaces the base for absolute, rooted, drive-relative and UNC paths; the `..` depth is counted over the joined path, so `..\..\..` climbs to the drive root; nothing is canonicalized. `fs.write` also creates parent dirs. | `tools.rs:344-367` | `fs.read ../../.claude/.credentials.json`; `fs.write` into the Startup folder or into `~/.parzi/config.toml` (new MCP server = arbitrary command next run) or a lane `parzi.toml` (`mode = "auto"`). Test `path_escape_rejected` only covers `../../secret`. | Reject absolute/prefix/root components; count depth over `path`; canonicalize root and target, require `starts_with`; refuse empty cwd (see R-7). Tests for `C:\`, `\foo`, `C:foo`, `\\?\`, UNC, junction. | S |
| B4 | **Run handles are never released.** `launch` inserts into `handles`; only `kill` removes. `handles.len()` is the live count everywhere. | `orchestrator.rs:659-670, 464, 285, 308, 679, 725` | In the GUI, after `max_concurrent` (4) finished runs every new thread queues forever, and a second message to any finished thread is refused with "run … is active; kill it first". `tests/teamwork.rs:199-227` codifies the leak. | Remove the handle when the task ends (before `notify_one`); prune `is_finished()` wherever the count is read; rewrite the teamwork test with a hanging provider. | S |
| B5 | **Composer locks forever.** `done`/`error` for a session that is not the active thread hits the early return before `liveRun = null`. | `App.svelte:643-672` | Start a run, click another thread, wait: Stop stays red, Enter is dead, `/kill` errors, until restart. | Handle `done`/`error` per session before the active-session gate; `liveRuns: Set<string>`. | S |
| B6 | **Bundled copyrighted art.** `assets/backgrounds/asuka.png` is Evangelion key art (khara/Gainax), shipped in every installer via `bundle.resources`, seeded into `~/.parzi`, hard-coded as the default in `theme.rs:117`, `paths.rs:60`, a built-in pack, a test and the README. Three copies in git since the first commit. | `assets/`, `examples/`, `ui/design/asuka.jpg`, `tauri.conf.json:51` | The repo cannot go public and the installer cannot be handed to anyone outside friends. It also breaks the auto-update story (needs a public feed). | Replace with owned/CC0 art or a procedural gradient (`image = ""` is supported); update every reference; `git filter-repo` the three paths; force-push; re-tag. **Human gate: history rewrite.** | M |
| B7 | **HEAD v0.1.7 is inconsistent and the gate is red.** See §0. | `4d80a72` | v0.1.7 installer has a dead Report-issue button and no logos; `cargo test` red; 15 tests unexecuted. | Do not publish the v0.1.7 draft. Commit per lane (§4 Phase 0). Make tests hermetic. Add CI. | S |

---

## 2. Serious (P1) — grouped by theme

Each row: what · where · fix. Sizes S/M/L. Full evidence in the area reports.

### 2.1 Security and hardening (report: `security.md`, `shell-cli.md`)

| # | What | Where | Fix | Size |
|---|---|---|---|---|
| H-1 | No CSP at all (`app.security.csp` absent → Tauri v2 injects none). Nine `{@html}` sinks; DOMPurify is the only barrier. With H-2/H-3 any script execution in the webview is RCE. | `tauri.conf.json:13-18` | `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' asset: http://asset.localhost data:; connect-src ipc: http://ipc.localhost; font-src 'self' data:; object-src 'none'; base-uri 'none'`. Verify hljs, fonts, wallpapers, `user.css`. | S |
| H-2 | `read_text_file` / `write_text_file` accept any absolute path (2 MiB cap only), parents auto-created. `save_config` accepts arbitrary MCP `command`/`args` and provider `base_url` (bearer redirect). | `main.rs:1060-1080, 1155-1179, 521-530` | Confine to `~/.parzi/**` and registered project roots (canonicalize + `starts_with`); native confirm dialog for MCP command changes; restrict `base_url` to loopback/https. | M |
| H-3 | `open_external_url` runs `cmd /C start "" <url>`: `&`, `|`, `^`, `%` inject commands; `&body=` also breaks the Report-issue URL; the allowlist rejects every MCP docs link. Uncommitted, but HEAD already calls it. | `main.rs:1224-1239` | Use `tauri-plugin-opener` or the existing `rundll32 url.dll,FileProtocolHandler` path from `antigravity_oauth.rs`; widen allowlist to `https://github.com/modelcontextprotocol/`; drop `parzi/parzi`. | S |
| H-4 | Google OAuth: no `state`, no PKCE, first local connection wins, browser opened before the listener binds; GUI login has no consent/TOS gate (PLAN §3 says opt-in + warning). | `antigravity_oauth.rs:55-60, 110-163`, `cli/main.rs:371`, `main.rs:1291`, `ModelsSection.svelte:233` | Random `state` + S256 PKCE, verify both and the `/oauth-callback` path, bind before opening, loop `accept` until match; `confirm()` dialog before login. | S–M |
| H-5 | `session.*` tools skip the allowlist, read/list transcripts across projects, and `send_message` appends a `User` turn into a running session (cross-session injection). | `handler.rs:451-459`, `orchestrator.rs:807-808, 864-990` | Scope list/read/send to the caller's project/subtree; gate reads in Ask mode; injected turns become `System`/`Tool` tagged untrusted; honour `is_allowed("session.*")`. | M |
| H-6 | Child processes inherit the full env, including `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `XAI_API_KEY`, `ANTIGRAVITY_*` that Parzi documents as credential sources. Stdin inherited. | `tools.rs:433-445`, `mcp.rs:158-165` | `env_clear()` + allowlist (`PATH`, `SYSTEMROOT`, `TEMP`, `HOME`, `APPDATA`, locale) + `cfg.env`; `stdin(null)`. | S |
| H-7 | Session ids are joined into paths unvalidated; ids come from the webview and from the model. `delete_thread` does `remove_dir_all(dir(id))`. | `store.rs:120-122` | `Uuid::parse_str` in `dir()`, once. | S |
| H-8 | Rendered markdown allows `https?:` and relative `src`/`href`: remote images = tracking/exfil beacon; clicking a link navigates the main window away (no `on_navigation`, no anchor interception). | `md.ts:99`, `Thread.svelte:59-77`, `DocReader.svelte:112-121` | DOMPurify hook: `src` only `parzi:`/`asset:`, `href` only `https?:`; delegated click → opener with allowlist; `Builder::on_navigation` guard. | S |
| H-9 | MCP secrets: preset tokens stored plaintext in `config.toml`, returned whole by `get_config`, echoed in the Connectors form; Postgres password in `args`. Presets run unpinned `npx -y` packages, several archived upstream. Tool descriptions/results passed verbatim and uncapped. | `config.rs:99`, `main.rs:518`, `mcpPresets.ts`, `ConnectorsSection.svelte:520,685`, `mcp.rs:30-36, 279-290` | Keyring-backed env (`keyring://name`); mask in UI; pin versions; cap description 1–2 k and result 24–32 k with a truncation marker. | M |
| H-10 | `find_token` scrape returns the first string under any `token`/`key` in opencode's `auth.json`, so another provider's API key can be sent as a Bearer to opencode.ai and then classified as a subscription. Same pattern for Grok. | `opencode.rs:79-97`, `compat_providers.rs:28-38`, `types.rs:205-223` | Delete the fallback. | S |
| H-11 | `delete_project` accepts `.` (wipes every project) and `C:` (`join` replaces the base). | `lanes.rs:146-161` | Shared `paths::safe_name()` (`[A-Za-z0-9_-]{1,64}`, reject reserved device names) used by every name-taking command; assert `starts_with(projects_dir)`. | S |
| H-12 | `parzi send … --yes` is the README quick start and, given B1, the only CLI mode in which tools run at all. | `README.md:16`, `cli/main.rs:248` | Fix B1; red banner when `--yes` meets `shell.exec`/`fs.write`/`*`; change the example. | S |

### 2.2 Runtime correctness (report: `runtime.md`)

| # | What | Where | Fix | Size |
|---|---|---|---|---|
| R-1 | Cancellation only checked between steps. Stream loop never selects on `cancel`; tool execution unguarded; `kill()` drops the `JoinHandle` instead of aborting; the run continues and `finish(Done)` overwrites `Killed`. | `handler.rs:337, 396, 466`, `orchestrator.rs:66-69, 463-474` | `select!` on cancel around the stream loop and tool execution; kill the subprocess; `abort()` as backstop; `finish()` never downgrades `Killed`. | M |
| R-2 | Orphaned subprocesses: no `kill_on_drop` anywhere; MCP init timeout (12 s, every turn) leaks a half-initialised server; no shutdown on app exit; `cmd /C` kill leaves the tree. | `tools.rs:446-462`, `mcp.rs:158-165`, `tools.rs:101-113` | `kill_on_drop(true)`; `McpManager::shutdown()` from Tauri exit and CLI end; Job object or `taskkill /T`; cache `defs_with_mcp` per run. | M |
| R-3 | MCP JSON-RPC reads one line per request with no `id` match; any notification desyncs the connection; a read timeout leaves the late response in the buffer. | `mcp.rs:111-142` | Loop until `id == expected`, skip notifications, `stop()` + respawn on timeout, check `protocolVersion`. Per-server mutex instead of one global lock; a real idle reaper. | M |
| R-4 | No token/cost budget stop; `turns` resets per provider slot so the 32-step cap is per slot. | `handler.rs:296-307` | `Budget { max_cost_usd, max_tokens }` checked after each `Usage` and before each `chat_stream`; run-level turns. | S–M |
| R-5 | Queued runs: their events are discarded when the pump launches them, the prompt lives only in memory (lost on restart), a launch error leaves the session `Queued` forever, and the GUI has no channel to them. | `orchestrator.rs:257-262, 691-696`, `main.rs:198-229` | One `broadcast` of `(SessionId, RunEvent)` on the orchestrator, subscribed once in `setup`; append the `User` event at enqueue; re-enqueue `Queued` metas on boot; launch error → Idle + error event. | M |
| R-6 | Transcript appends and status writes are `let _ =` in 14 places. Disk-full or permission errors drop events and the run continues; a failed `set_status` leaves the thread `Active`. | `handler.rs:211…697` | Propagate for User/Assistant/ToolCall/ToolResult/status → `RunEvent::Error` + `Killed`. | S |
| R-7 | `spawn()` never persists `cwd`; children inherit an empty cwd; the sandbox root becomes the app process cwd. | `orchestrator.rs:242 vs 269`, `tools.rs:345-349` | `store.set_cwd` in `spawn`; refuse empty cwd in `resolve`. | S |
| R-8 | Per-tool `auto` override beats lane `deny` ("Lockdown" is not a lockdown); the handler asks the user before the executor checks the allowlist, so users approve tools that are then denied. | `handler.rs:461-478`, `tools.rs:125-143` | One `decide(name) -> Denied(reason) | Auto | Ask`; lane `Deny` is absolute. | S |
| R-9 | `parzi kill` cannot cancel a GUI-owned run (each process has its own `handles`); `recover()` in either process flips the other's `Active` sessions to `Idle`. | `cli/main.rs:325-331, 247`, `orchestrator.rs:197-206` | `sessions/<id>/kill` marker polled at step boundaries; `sessions/<id>/lock` with pid + heartbeat; `recover()` skips live pids. | M |
| R-10 | Circuit breaker trips on any 3 errors incl. 400/auth; cooldown from `Retry-After` unclamped (a day is possible); reset only via a Settings button and two dead commands. | `circuit_breaker.rs:49-68`, `handler.rs:270-275`, `router.rs:128-145` | Count 5xx/transport only; clamp ≤ 15 min; show cooldown in the model menu. | S |
| R-11 | Tool results are `role: user` text with a `[result:name untrusted]` prefix; tool calls are flattened to `[tool:name {...}]` text; no native tool_use/tool_result pairing on any provider. | `context.rs:70-83`, adapters | Keep ids; emit native tool blocks per provider; delimited data block for results. Cross-cutting with providers. | L |

### 2.3 Providers (report: `providers.md`)

| # | What | Where | Fix | Size |
|---|---|---|---|---|
| P-1 | `max_tokens` for Extra/Ultra (131 072 / 262 144) and High (65 536) exceeds the output limit of every Claude model, Haiku, Grok and all Antigravity models; the 400 is classified terminal, so the run dies on the default provider. | `orchestrator.rs:20-28`, `anthropic.rs:106`, `antigravity.rs:187`, `openai_compat.rs:151`, `codex.rs`, `opencode_wire.rs:64` | Clamp centrally to the catalog's output limit in every body builder; test every (provider, effort). | S |
| P-2 | `String::from_utf8_lossy` per TCP chunk in seven stream loops: any multi-byte character on a chunk boundary becomes U+FFFD (garbled text; wrong file paths in tool args for umlauts). | `anthropic.rs:227`, `codex.rs`, `openai_compat.rs`, `antigravity.rs:448`, `opencode_wire.rs:81,202,331` | One shared byte-buffered line reader; decode complete lines only. | M |
| P-3 | Parallel tool calls: a second `tool_use` block / `function_call` item overwrites the first; only the last survives on Anthropic, Codex and both Zen kinds. | `anthropic.rs:264-279`, `codex.rs`, `opencode_wire.rs:235-279, 356-372` | Collect into a `Vec`, emit all. | S |
| P-4 | In-stream error frames (`event: error`, `response.failed`, `{"error":…}` without `choices`/`candidates`) are ignored; the partial answer is recorded as complete and nothing fails over. | all parsers | Return `Err` with a retriable kind. | S–M |
| P-5 | Timeouts: 180 s *total* deadline aborts long generations, then is classified retriable (hop + duplicated partial answer); no connect/read timeout; Antigravity token refresh has no timeout and runs inside `chat_stream` before `rx` is returned. | `anthropic.rs:75`, `codex.rs`, `antigravity.rs:238-243, 380`, `opencode.rs:161`, `antigravity_oauth.rs:91` | `connect_timeout(10 s)` + `read_timeout(90 s)`, no total; 30 s on token/models; refresh inside the spawned task; client timeouts after text streamed are not retriable. | S |
| P-6 | Antigravity `Usage` emitted per chunk (Gemini sends cumulative counts) → tokens over-counted by the chunk count. Adapters never stop on cancel (`let _ = tx.send`) → quota burns to completion. `stop_reason`/`finishReason` never read (refusal / max_tokens look like normal completions). | `antigravity.rs:462-466`, all adapters, `anthropic.rs:281-286` | Emit last-seen once; return on send error; `StreamEvent::Stop(reason)`. | S |
| P-7 | Failover classification by substring: `insufficient` matches 403 permission errors, `timeout` matches client aborts, plain 500/502 are not retriable; `Retry-After` HTTP-date parses to the day of month. | `router.rs:178-204` | Structured error kind set at the HTTP boundary; table-driven test. | M |
| P-8 | Pricing and catalog: Smart Auto low/medium picks (`gpt-5.6-luna`/`terra`) are priced $0 while the API path remaps and spends; Codex never checks JWT expiry; opencode key → always "Subscription"; hard-coded foreign GCP project fallback; cache TTL gates *reads* so a day offline loses the model list; `catalog.tmp` races between concurrent `models()` calls. | `catalog.rs:374-375, 507-553`, `codex.rs:60-75`, `opencode.rs:306-312`, `antigravity.rs:198`, `handler.rs:176-183` | Price by the id actually sent; decode `exp`; Subscription only for `opencode-go`; error instead of fallback project; per-provider freshness, TTL gates fetch not read; unique tmp. | S–M |

### 2.4 Core (report: `core.md`)

| # | What | Where | Fix | Size |
|---|---|---|---|---|
| C-1 | Corrupt, newer or version-less `config.toml` panics the GUI at boot behind `windows_subsystem = "windows"`: double-click, nothing happens, no log. `parzi init` silently overwrites a corrupt config with defaults. | `config.rs:232-259`, `main.rs:1325`, `cli/main.rs:172-176` | `load_or_recover()` → rename to `config.toml.broken-<ts>`, defaults + native message box; `init` refuses to overwrite unparsable files. | S |
| C-2 | `events.jsonl` is read all-or-nothing; append is two syscalls with no fsync (a torn line bricks the session for every later run, fork and render); `#[serde(tag = "kind")]` has no catch-all, so a newer build's kind bricks older readers. | `store.rs:229-251` | Single `write_all` of `json + \n`; per-line tolerant parse with a warning; catch-all variant. | S |
| C-3 | Context builder: latest user message is not pinned; an oversized newest item (a big tool result) makes `messages` empty; no §4 budget formula; compaction triggers at `> 96 events`, not 80 %; `limit` hard-coded 100 000 in the handler. | `context.rs:94-99`, `handler.rs:423-439` | Always include the latest `User` event (truncate with a marker); skip non-fitting items instead of `break`; move budget + 80 % trigger into core with the catalog limit. | M |
| C-4 | `migrate()` drops any provider entry not in the roster on load (custom `base_url` entries vanish on next save); favourites keep old prefixes. | `config.rs:276-279` | Keep unknown entries; rewrite favourites. | S |
| C-5 | `render_md` re-renders the whole session on every append (O(n²)), writes non-atomically, and never closes the widget fence (everything after the first widget is inside a code block in `session.md`). `atomic_write` has no fsync and a deterministic tmp name shared by GUI and CLI. | `store.rs:238-251, 375-454`, `error.rs:26-37` | Lazy render; close the fence; unique tmp + `sync_all`. | S–M |
| C-6 | `list()` rescans every session directory on every call (sidebar refresh, six other paths) and silently drops unparsable metas. | `store.rs:176-191` | In-memory index + mtime check; log bad metas. | M |
| C-7 | Widget/diagram validation caps nodes but not total size; 5 of 8 widget types unchecked; `Theme.background.image` accepted unvalidated (absolute/`..` paths get copied into packs); `image::open` with default decoder limits. | `widgets.rs:478-565`, `theme.rs:676-679` | Total-size cap; validate every type; reject non-relative background paths; decoder limits. | S |

### 2.5 Shell and CLI (report: `shell-cli.md`)

| # | What | Where | Fix | Size |
|---|---|---|---|---|
| S-1 | 71 IPC commands (cap 10, documented deviation 14), 1 419 lines, business logic in the shell (subsession creation duplicating `orch.create_subsession`, ancestor walk duplicating `store::subtree_ids`, base64 decoder, file walkers, TOML writer). Seven dead commands; a `window_*` quartet that duplicates granted capabilities. | `main.rs` | Move logic into core/runtime; delete dead commands; amend PLAN with the real cap. | M |
| S-2 | `install_pasted_skill` is called with `{ pack_name }`; Tauri expects `packName`. Settings › Skills paste and "Create skill" always fail. | `api.ts:343-344`, `main.rs:879-883` | Send `packName`; add the IPC contract test (§5). | S |
| S-3 | Fresh installs have no wallpaper: the bundled resource lands at `$RESOURCE/_up_/assets/backgrounds/asuka.png`; `background_url` resolves `$RESOURCE/asuka.png`; the seeder looks next to the exe. PROGRESS skipped this because a dev machine already had the seeded file. | `tauri.conf.json:51`, `main.rs:686-690`, `paths.rs:58-78` | Map-form `resources` (`"../assets/…": "asuka.png"`) or move the asset; verify on a clean user account. Inherited by whatever replaces the image (B6). | S |
| S-4 | No logs anywhere: the GUI installs no tracing subscriber (every `tracing::warn!` in runtime/providers is dropped), the CLI logs WARN to stderr, `~/.parzi/logs/` has been empty since Sep 9. No panic hook. | `main.rs`, `cli/main.rs:133` | `tracing-appender` daily rolling file (5-file cap) in both binaries; panic hook → dialog + log; Copy-diagnostics appends the tail. | S |
| S-5 | No single-instance guard; every launch starts another orchestrator on the same `~/.parzi` and `recover()` stomps the first one's runs. | `main.rs` | `tauri-plugin-single-instance`, focus existing window. | S |
| S-6 | Approval event carries no session id; the UI keeps one `approval` slot, so concurrent cards overwrite each other, and switching threads drops the card → 120 s → Deny. `vote()` has no error handling. | `main.rs:35`, `App.svelte:203, 621`, `Thread.svelte:53-57` | Add `session`; queue per session; keep the card across thread switches. | S |
| S-7 | Updater can never fetch: private repo + every release a Draft + `releases/latest/download` endpoint; `updateStore` swallows the 404 into `idle`. All signing work is inert. | `tauri.conf.json:72-80`, `release.yml`, `updateStore.ts` | Decide: public repo (blocked by B6) or a separate public release feed repo; surface `error` in Settings. | S + decision |
| S-8 | Release profile and lints do not apply to the GUI: `src-tauri` is its own workspace root (opt-level 3, no LTO, no strip, no `unsafe_code = forbid`, no `[lints]`). | `src-tauri/Cargo.toml` | Copy `[profile.release]` and `[lints]`. | S |
| S-9 | CLI: ambiguous id prefix picks the first match silently and `send <id>` skips prefix resolution; `send` exits 0 on run failure; `parzi logout <any>` deletes keyring slots for any string (`logout anthropic` deletes the API key); `export --md` from PLAN is absent; `--effort` help says `med`. | `cli/main.rs:216-218, 283, 308-311, 394-400, 486-495` | Error on >1 match; non-zero exit; validate provider; add `--md`. | S |
| S-10 | Uninstaller "Also delete my threads, settings and local data" removes only WebView2 data, never `~/.parzi`. `app_version` returns a hard-coded `0.1.7-v3` suffix. `create_project` writes `root` unescaped into TOML. `save_background_data` has no size cap and hand-rolls base64 in the shell. | `installer/English.nsh:8`, `main.rs:84, 1043, 749-790` | `NSIS_HOOK_POSTUNINSTALL`; `CARGO_PKG_VERSION`; `toml::to_string`; cap + move to core. | S |

### 2.6 UI (report: `ui.md`)

| # | What | Where | Fix | Size |
|---|---|---|---|---|
| U-1 | Global Enter/Esc handlers fight: Enter in the palette, sidebar filter or workspace name field sends the draft; Esc from any popup falls through to `stopRun()`. | `Omnibar.svelte:329-342, 667`, `App.svelte:826-832`, `Sidebar.svelte:297-301` | Enter/Esc on the textarea only; check `defaultPrevented` / a popup store before `stopRun`. | S |
| U-2 | Fake controls: the composer "Permission policy" menu is not bound to anything (tools run under `lanes.default_mode` regardless); Connectors "docs" buttons reject silently; the "Build" mode equals "Chat". | `Omnibar.svelte:26-59, 900-909`, `App.svelte:945-1003`, `ConnectorsSection.svelte:820` | Bind to `cfg.lanes.default_mode` and persist; fix H-3; remove or implement Build. | S |
| U-3 | Installed Skills are consumed by nothing: slash items are 11 hard-coded entries; `plugins::slash_commands()` has no caller; typing `/review` sends the literal string. | `Omnibar.svelte:399-415`, `plugins.rs:93` | `list_slash_commands` IPC merged into the menu with prompt substitution, or drop the page. | M |
| U-4 | Streaming: no 60 ms debounce, no virtualization, `renderMarkdown` inline in the template on an index-keyed `{#each}` (full re-render per token), full `highlight.js` (1.54 MB chunk). PLAN §8 and §11 budgets are not met. | `App.svelte:651`, `Thread.svelte:195, 285-297`, `md.ts:3` | Flush buffer; per-message HTML cache; `highlight.js/lib/core` + 12 languages; windowed list > 500 blocks. | M |
| U-5 | `openThread` has no stale-response guard (rapid switching writes A's events into B); `live` buffer discarded on switch; startup is one sequential `try` so an early failure skips event-listener registration; listeners never unlistened. | `App.svelte:197-224, 836-856` | Sequence token; per-session live buffers; register `onRunEvent` first; per-step toasts; unlisten on destroy. | S |
| U-6 | `route_transition` is emitted by Rust but absent from `UiEvent` → no failover toast. Inline `parzi-widget` fences bypass core validation and `Widget.svelte`/`Diagram.svelte` throw on malformed payloads; Svelte 4 has no error boundary, so the thread render aborts. Tool output fence can be broken out of with a ``` line. | `api.ts:66-76`, `Widget.svelte:8-10`, `Diagram.svelte:3-4`, `Thread.svelte:272` | Add the event; validate in the component and render a `bad` card; longer fence or `<pre>` text node. | S |
| U-7 | Theme discipline: 111 colour literals in `.svelte` (105 are the banned `var(--x, #fallback)` in the inspector), hard-coded hljs `github-dark` palette, emoji glyphs as icons, 8 `aria-label`s total, palette not focused on open. | `AgentVisualizer`, `DocReader`, `RightPanel`, `ThreadRow.svelte:178-201`, `theme.css:16` | Tokenize; SVG icons; labels; focus management. | M |
| U-8 | Zero UI tests; no runner. | `ui/package.json` | vitest + jsdom for `md.ts` and the IPC contract; one Playwright smoke. | M |

### 2.7 Release, CI, repo, docs (report: `repo-release.md`)

| # | What | Where | Fix | Size |
|---|---|---|---|---|
| D-1 | No CI on push/PR. Only the tag-triggered release workflow exists; PLAN's "clippy pedantic deny in CI" is prose. This is how B7 happened. | `.github/workflows/` | `ci.yml` (fmt, clippy deny gate, `cargo test --workspace --no-fail-fast` with `PARZI_HOME`, src-tauri check + test, `svelte-check`, build, `cargo deny`, version/junk hygiene); release depends on it. Full YAML in `repo-release.md` §4. | S |
| D-2 | Release pipeline: floating `tauri-action@v0`, `rust-toolchain@stable`, `rust-cache@v2` with the update-signing key in the step env; no `rust-toolchain.toml`; `workflow_dispatch` from a branch tags a release `main`; no Authenticode (SmartScreen); CLI never built or published. | `release.yml` | Pin SHAs; `rust-toolchain.toml` 1.98.1; explicit tag input; `cli` job; Trusted Signing (cost). | S–M |
| D-3 | `THIRD_PARTY_NOTICES.md` attributes `t3.rs` (never existed) and the deleted `ui/src/assets/providers/`, omits lobehub icons (MIT, SVG copied verbatim), CLIProxyAPI, ui-ux-pro-max and the three OFL fonts compiled into `dist`; the file is not in `bundle.resources`. | root | Rewrite; add to resources; link from About. | S |
| D-4 | Tests write to the real `~/.parzi` (no `PARZI_HOME` override in `paths.rs`); env vars mutated in tests without restore; the failing plugin test is the direct result. | `paths.rs:6-7`, `logic.rs:163`, `failover.rs:77`, `queue.rs:47`, `teamwork.rs:49`, `plugins.rs:706-790`, `router.rs:32-83` | `PARZI_HOME` + RAII temp home; `static Mutex` for env tests. Prerequisite for every test in §5. | M |
| D-5 | Repo hygiene: root `package.json` (`npm init` junk, ISC, 1.0.0); no `.gitattributes` (40 CRLF warnings per command); lockfiles lag every bump; `src-tauri/gen/` and android/ios icons tracked; `parzi-icon-src.png` is the obsolete serif P; `.lane_claim_*` in root; `ui/design/` (3 MB) inside the Vite root; 16 U+FFFD characters committed in PROGRESS; `examples/config.toml` lists ten providers incl. `t3`; `CHANGES_SUMMARY.md` describes files that never existed; LICENSE names "Parzi" as the copyright holder; no changelog; Windows-only unstated. | various | Delete/ignore/renormalize; bump script; `STATUS.md` (≤ 15 lines: version, what ships, open gates, known broken); move PROGRESS and CHANGES_SUMMARY under `docs/`; regenerate examples from defaults in a test. | S |
| D-6 | PLAN.md is "frozen" but reality diverged on: crates ("wires, never implements"), IPC cap (10 → 71), MCP (rmcp → hand-rolled, no HTTP), providers (10 → 5), settings pages (5 → 7), concepts not in PLAN (Skills, Context, Artifacts, subsessions, packs, queue, circuit breaker, inspector, "Scheduled Tasks" row with no implementation), files < 400 lines (18 violators), no-unwrap (7 real sites), logs, budgets (never measured), `--low-mem` (missing), `RunState` enum (absent), `retry` (absent), Streamable HTTP (absent). | `PLAN.md` | Amend PLAN once, deliberately (§6 decisions); enforce the rules that survive in CI; delete the rest so REVIEW stops being theatre. | S |

---

## 3. Moderate and nits (P2/P3)

Not repeated here. Each area report ends with its P2/P3 list and a "Verified good" list. Counts: core 10+10, providers 12+12, runtime 16+11, shell/CLI 15+16, UI 12+17, security 9+4, repo 12+11.

---

## 4. The fix plan

Lanes are file-owned so agents can run in parallel (their lane-claim protocol). Sizes are lane-days; the conversion uses the measured calibration (light lane ≈ 50 min build; heavy lane ≈ 2.5 h including two review rounds). Anything touching the approval gate, the sandbox, auth, ids or paths is heavy and gets a hostile review before merge.

### Phase 0 — stop the bleeding (today, one lane, light, 1 day)

1. Do not publish the v0.1.7 draft. Let its CI finish and leave it.
2. Fix H-3 (`open_external_url`) before it is committed.
3. Commit the working tree per lane, in this order, running the gates between each: (a) MCP exposure/per-tool modes/hot-apply config; (b) skills paste/git install + the fixed opener + `api.ts` (this repairs HEAD); (c) providers OpenCode Zen/Go + effort ladder, only once that session says it is done; (d) UI inline provider marks + favicon + appearance polish, with the NOTICES lobehub line; (e) lockfiles. Use `git add -p` on `config.rs`, `handler.rs`, `orchestrator.rs`, `main.rs`, `api.ts`. Drop the root `package.json`; ignore `.lane_claim_*`.
4. D-4: `PARZI_HOME` override + temp home in every test; `cargo test --workspace --no-fail-fast` green (80/80). Remove `parzi-fix-alpha`/`beta` from your live `~/.parzi/plugins` (the orchestrator was not permitted to delete them).
5. D-1: `ci.yml` on push/PR; release depends on it. `.gitattributes`; `[lints]` + release profile in `src-tauri/Cargo.toml` (S-8).
6. Tag v0.1.8 from a HEAD where every gate is green.

Acceptance: CI green on `main`; `svelte-check` 0 errors at HEAD; 80 tests run.

### Phase 1 — security gate (before any external user; five lanes, parallel)

| Lane | Owns | Does | Kind | Size |
|---|---|---|---|---|
| S1 | `handler.rs`, `orchestrator.rs`, `tools.rs`, runtime tests | B1, B2, B3, B4, R-7, R-8, H-5, H-6 | heavy | 2 |
| S2 | `src-tauri/**`, `store.rs` (id check only) | H-1, H-2, H-3 (if not done in Phase 0), H-7, S-5, S-6, `on_navigation` guard | heavy | 1.5 |
| S3 | `antigravity_oauth.rs`, `opencode.rs`, `compat_providers.rs`, `types.rs`, GUI login gate | H-4, H-10 | heavy | 1 |
| S4 | `assets/`, `examples/`, `theme.rs` defaults, `paths.rs`, packs, README | B6 replacement + every reference; history rewrite is a **human gate** (force-push, re-tag) | light + gate | 0.5 |
| S5 | `md.ts`, `Thread.svelte`, `DocReader.svelte`, `App.svelte` (event reducer only) | B5, H-8, U-6 fence fix | light | 1 |

Pre-review probes for S1–S3 (from the calibration skill): fail-open defaults (grep every `unwrap_or_else(|| … Auto …)`, `or ""`, empty-cwd), confirmations bind the whole target (approval key ↔ session), the documented instruction is the tested call (README `--yes` example), surfaces agree (GUI, CLI, harness bridge all hit the same gate).

Acceptance: the four tests named in B1–B4 pass; sandbox table test (absolute, rooted, drive-relative, UNC, `\\?\`, junction) passes; CSP smoke (`<img onerror>`, `<script>`, remote `<img>`) shows no execution and no outbound request; OAuth test rejects a wrong `state`.

### Phase 2 — runtime and provider correctness (three lanes; S1 must land first because R1 shares its files)

| Lane | Owns | Does | Kind | Size |
|---|---|---|---|---|
| R1 | `handler.rs`, `orchestrator.rs`, `mcp.rs`, `tools.rs`, `circuit_breaker.rs`, `src-tauri` event forwarding | R-1, R-2, R-3, R-4, R-5, R-6, R-9, R-10 | heavy | 3 |
| R2 | `crates/parzi-providers/**` | P-1 … P-8 via one shared SSE reader | heavy | 3 |
| R3 | `crates/parzi-core/**` | C-1 … C-7, H-11 (`safe_name`), budget + 80 % compaction moved into core | heavy | 2 |

Acceptance: kill-mid-tool, kill-mid-stream, fork-equality, budget-stop, MCP spawn→list→call→idle-kill, queued-run persistence, mocked SSE fixtures per parser (split UTF-8, error frames, parallel tool calls), config/theme round-trip, torn-line recovery, context pins latest user.

### Phase 3 — product, shell, release (three lanes; U1 and U3 can overlap Phase 2)

| Lane | Owns | Does | Kind | Size |
|---|---|---|---|---|
| U1 | `ui/src/**` except `md.ts` | U-1, U-2, U-3, U-4, U-5, U-7; extract `runStore`/`threadStore` from `App.svelte` | light | 3 |
| U2 | `src-tauri/**`, `crates/parzi-cli/**`, installer | S-1, S-2, S-3, S-4, S-9, S-10, H-9 keyring env, R-11 approval session id (if not in S2) | heavy | 2.5 |
| U3 | `.github/**`, docs, `THIRD_PARTY_NOTICES.md`, `PLAN.md`, `examples/` | S-7 feed decision, D-2, D-3, D-5, D-6, `STATUS.md`, changelog, bump script | light | 1.5 |

Acceptance: IPC contract test passes (every `invoke` name + camelCase args match `#[tauri::command]`); clean-VM install shows the wallpaper; `~/.parzi/logs/parzi.log` exists after a run; second launch focuses the first window; a fresh install can fetch `latest.json` from the chosen feed.

### Phase 4 — tests and conformance (two light lanes, 3 days total)

The fifteen tests in `tests-conformance.md` §F (T0–T15), the provider SSE fixtures, the vitest harness (`md.ts`, `api.ts` contract), one Playwright smoke with a mocked bridge, and CI gates for the rules that survive §6 (file length, non-test `unwrap`, `cargo deny`).

### Time

Long pole is the runtime chain: Phase 0 → S1 → R1 → its tests, roughly 1 + 5 + 7.5 + 2 hours of wall time at the measured rates, plus review bounces. With the other lanes running beside it and a 15 % process tax, expect **five to seven working sessions of four hours** to reach "every P0/P1 merged with its test green". That reaches "merged and tested", not "verified by a second person on a clean machine" — the P7 screenshot gate, the clean-install wallpaper check and the Antigravity live login still need your eyes.

---

## 5. Tests that must exist (top of the list)

From `tests-conformance.md` §F, ordered by what they would have caught this week:

1. `PARZI_HOME` + temp home for every store/plugin test (unblocks the gate).
2. Ask-mode approval with a slow approver → tool runs once, after the answer (B1).
3. Child session in an Ask lane with no interactive approver → tool does not run (B2).
4. Sandbox table: absolute, rooted, drive-relative, UNC, `\\?\`, junction, empty cwd (B3).
5. `max_concurrent + 1` sequential completed runs → none queues; second message to a finished thread succeeds (B4).
6. Kill mid-tool: child pid gone < 1 s, no later `ToolResult`, status stays `Killed`.
7. Fork at step k equals the source prefix.
8. Budget stop on `Usage` over the ceiling.
9. MCP mock server: list cached, call round-trip, notification-before-response skipped, timeout respawns, idle reap, child killed on drop.
10. SSE fixtures per parser: split multi-byte char, CRLF, keep-alive, `[DONE]`, error frame → retriable `Err`, two tool calls → two events.
11. `max_tokens ≤ output_limit` for every (provider, effort).
12. Config and theme round-trip; corrupt config recovers without panic; torn `events.jsonl` line skipped.
13. IPC contract: parse `generate_handler!` params vs every `invoke()` payload in `api.ts` (would have caught S-2).
14. `md.ts`: `javascript:`/`data:`/remote `src` stripped, `parzi:` kept, `<svg onload>` dropped, unclosed fence is one block.
15. Playwright smoke: boot → send → stream with a fence → switch thread mid-stream → done → composer unlocked (B5).

---

## 6. Decisions only you can make

1. **Asuka.** Replacement art, and the history rewrite (force-push of a private repo; every existing clone and tag is invalidated).
2. **Public repo or a separate public release feed.** Without one, auto-update stays dead by construction.
3. **PLAN amendments.** IPC cap (10 is fiction; pick 30 and enforce, or delete the rule), the 400-line rule (enforce with a split of `main.rs`/`App.svelte`/`orchestrator.rs`, or delete), rmcp vs hand-rolled MCP (keep hand-rolled but fix R-3), settings page count, provider list, `--low-mem`, `RunState`.
4. **Antigravity.** PLAN says opt-in with a warning; the router auto-routes to it as soon as any credential exists (`tests/router.rs:69-76`). Keep or revert.
5. **Skills page.** Wire it (U-3) or cut it.
6. **Authenticode.** Trusted Signing costs money; without it every friend sees SmartScreen.
7. **Tool-call fidelity (R-11).** Native tool blocks per provider is the one L-sized item; it changes transcript format. Now or after v0.2.

---

## 7. Verified good (do not refactor)

- Deny-by-default allowlist; `deny` beats `allow`; unknown modes parse to `Ask`; disabled servers never exposed. Tests cover it.
- Opened project folders are never read for policy: `parzi.toml`/`SYSTEM.md` come only from `~/.parzi/projects/**`. A cloned repo cannot flip lane mode directly (only via B3).
- Credential files are read-only and never logged; no `Debug` derive on token-holding structs; keyring is the real OS store; `save_key` writes keyring only; `get_config` returns no API keys; doctor is presence-only.
- `GuiApprover` keys are random single-use UUIDs with a 120 s deny timeout; the backend never listens to webview events, so `core:event:allow-emit` gains nothing.
- OAuth listener binds `127.0.0.1` with a 5-minute timeout; browser opened via `rundll32` argv.
- All model-facing `{@html}` sinks go through markdown-it `html:false` + DOMPurify; widgets/diagrams validated in core and rendered as text/numeric attributes; asset protocol scope is narrow.
- `store.fork` truly slices events into a fresh id; `subtree_ids`/`cascade_kill_ids` are pure and cycle-safe; `Theme::normalized()` clamps before save and CSS emission; `css_font_list` quoting and `accent_ink` are correct.
- Router: subscriptions first, keys only with `keys_in_auto`, unauthed skipped, deterministic; failover bounded by slot count; Antigravity refresh-and-retry exactly once; Claude model ids and prices match the current reference table.
- Skills packs execute no code; names validated; git install is https-only with fixed args and `GIT_TERMINAL_PROMPT=0`.
- `.gitignore` really ignores `target/`, `node_modules/`, `ui/dist/`, `*.key`, `*.sig`; index line endings are uniformly LF; no secrets in the tree; only the minisign public key is committed.
- Custom-protocol feature present; the blank-window navigate hunk is gone from the working tree.

---

## 8. Method and limits

- Eight agents, one area each: core, providers, runtime, shell+CLI, UI, security (cross-cutting), tests+conformance, repo+release. Every file in scope was read in full; PROGRESS claims were treated as claims.
- Gates run by the orchestrator at 20:09 and 20:32 (logs in the session scratchpad; summary in §0).
- Static only. B1 was re-read by the orchestrator (`emit` → sink; forwarder `continue`; `select!` semantics) but not timed in a running app. Nothing was executed against a provider.
- The tree was edited by other sessions during the audit: v0.1.7 tagged at 18:17Z; providers crate edited 20:05–20:22 (for ~7 minutes it did not compile); `svelte-check` errors fixed in the working tree between the two gate runs.
- `cargo audit` is not installed; Rust advisories were checked by version inspection only.
