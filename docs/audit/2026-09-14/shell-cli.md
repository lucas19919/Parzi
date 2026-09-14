# Parzi audit — Tauri shell (`src-tauri/`) + CLI (`crates/parzi-cli`)

Static review, read-only, 2026-09-14. Working tree audited as-is (uncommitted diff to `src-tauri/src/main.rs` = +218/-15, mostly `list_mcp_tools`, `list_all_mcp_tools`, skills install/delete, `open_external_url`, `save_config` hot-apply; CLI diff = a 3-line borrow fix in `cmd_health`). Nothing half-done in the diff itself; the problems below are in the design and in what the diff added.

Headline: no P0 in the sense of "cannot build or run", but eight P1s, three of which are shipped-feature breakage that PROGRESS.md marks as done (paste-install a skill, report-an-issue link, fresh-install wallpaper), two are security posture (no CSP while the webview holds arbitrary file read/write + a `cmd.exe` launcher), and three are spec claims that are false (10-command cap → 72, `parzi kill` mid-run, clippy CI gate).

Confirmed: the "navigate-fallback hunk" is NOT present in the working tree (`grep navigate|localhost:1420|TcpStream|probe` in main.rs finds nothing); `custom-protocol` is in `src-tauri/Cargo.toml`; `devUrl` + `beforeDevCommand` are back in `tauri.conf.json`.

---

## 1. Command inventory (72 commands; PLAN §9 cap = 10; PROGRESS:372 last claimed 11)

Legend — Val: input validation. Logic: business logic living in the shell (PLAN §1 "wires, never implements"). All commands return `Result<T, String>` (anyhow/ParziError stringified — untyped; UI gets prose, cannot branch on error kind).

| # | command (main.rs line) | inputs | delegates to | Val / flags |
|---|---|---|---|---|
| 1 | `app_version` :83 | — | `env!(CARGO_PKG_VERSION)` | returns `"0.1.6-v3"` — hardcoded `-v3` suffix (P3) |
| 2 | `toggle_favorite` :88 | spec | `ParziConfig::load/save` | Logic (toggle + truncate 9) in shell; does not `apply_config` → orch cfg stale (P3) |
| 3 | `effort_options` :101 | provider | `router::effort_options` | ok |
| 4 | `list_threads` :106 | — | `store.list` | ok |
| 5 | `get_thread` :111 | id | `store.get/events` + raw `fs::read_to_string(sessions/<id>/session.md)` | `id` unvalidated → `sessions_dir().join(id)`; reads only `meta.json`/`session.md` under any dir (low) |
| 6 | `send_message` :129 | session_id?, project, lane, model, prompt, cwd, effort?, attachments?, parent_id? | `orch.send_to` / `orch.spawn` / `store.create_with_parent` | ~60 lines of Logic: subsession creation, title derivation, model/lane/cwd inheritance — duplicates `orch.create_subsession` (P1-7). `read_attachments` comment lies (P3). Per-call event forwarder → queued runs never stream (P1-5) |
| 7 | `kill_run` :235 | id | `orch.kill` | kills idle threads too (status → Killed) (P3) |
| 8 | `fork_thread` :240 | id, at? | `orch.fork` | ok |
| 9 | `create_subsession` :253 | parent_id, title, prompt?, model? | `orch.create_subsession` | ok; emits `SubsessionCreated` |
| 10 | `reparent_thread` :274 | id, parent_id? | `store.set_parent` | ok (cycle check is store's job) |
| 11 | `toggle_pin` :310 | id, pinned | `store.set_pinned` | ok |
| 12 | `approve_tool` :315 | key, allow | `pending` map → oneshot | unknown/expired key → Err; no session binding (see P2-8) |
| 13 | `get_models` :388 | refresh? | `provider()`, `p.models()` JoinSet 10s/provider | Logic: roster ordering, auth mapping (borderline) |
| 14 | `refresh_provider` :429 | provider | same, 15s | `canonical_id` validates |
| 15 | `get_provider_health` :446 | — | `orch.provider_health` | DEAD — no invoke in ui/src |
| 16 | `reset_circuit_breaker` :451 | provider? | `orch.reset_circuit_breaker` | DEAD — no invoke in ui/src |
| 17 | `save_key` :460 | provider, value | `keyring::Entry("parzi", slot).set_password` | keyring only, trimmed, empty rejected, slot via `key_entry` — good |
| 18 | `delete_key` :473 | provider | keyring delete | errors swallowed (`let _`) |
| 19 | `reset_theme` :483 | — | `Theme::default().save` | ok |
| 20 | `purge_sessions` :490 | — | `store.purge_finished` | ok |
| 21 | `get_theme_css` :495 | — | `Theme::load` + `theme_css` | ok |
| 22 | `get_user_css` :501 | — | `theme::read_user_css` | ok |
| 23 | `save_user_css` :507 | css | `theme::write_user_css` | no size cap (P3) |
| 24 | `save_theme` :513 | theme | `theme.save` | ok |
| 25 | `get_config` :518 | — | `ParziConfig::load` | returns `mcp.servers[*].env` (plaintext secrets) to webview (P2-14); no API keys (good) |
| 26 | `save_config` :522 | cfg | `cfg.save` + `orch.apply_config` | version check; persists arbitrary MCP `command`/`args` (by design = code exec on next run) |
| 27 | `list_mcp_tools` :552 | server | `mcp.list_tools` 20s | trims; spawns the server as a side effect |
| 28 | `list_all_mcp_tools` :591 | — | same, JoinSet | 40 lines duplicated from #27 (P3) |
| 29 | `get_theme` :642 | — | `Theme::load` | ok |
| 30 | `list_packs` :647 | — | `theme::list_packs` | wrapper exists, no UI caller (dead) |
| 31 | `save_pack` :652 | name | `theme::save_pack` | `check_pack_name` (alnum/_/-) — good |
| 32 | `list_pack_infos` :657 | — | `theme::list_pack_infos` | ok |
| 33 | `delete_pack` :662 | name | `theme::delete_pack` | name check + builtin guard — good |
| 34 | `apply_pack` :667 | name | `theme::apply_pack` | name check — good |
| 35 | `background_url` :673 | — | `Theme::load`, `path().resolve("asuka.png", Resource)` | WRONG resource path on installed builds (P1-4) |
| 36 | `run_doctor` :693 | — | `Doctor::run` | spawns a 2nd `McpManager` (duplicate MCP procs) (P3) |
| 37 | `run_doctor_quick` :700 | — | `Doctor::run_quick` | ok |
| 38 | `run_doctor_mcp` :707 | — | `Doctor::run_mcp_only` | ok |
| 39 | `list_background_urls` :721 | — | `theme::list_backgrounds` | ok |
| 40 | `list_backgrounds` :734 | — | `theme::list_backgrounds` | wrapper unused (dead) |
| 41 | `set_background` :739 | name | `theme::set_background` | `/ \ ..` rejected + `is_bg_file` — good |
| 42 | `upload_background` :745 | src | `theme::upload_background` | arbitrary src path, but image-ext + ≤20MB; wrapper unused (dead) |
| 43 | `save_background_data` :775 | name, base64_data | hand-rolled `decode_b64` (:749) + `fs::write` + `set_background` | Logic in shell; no size cap on base64; no ext/magic check; `set_background` error swallowed (P3) |
| 44 | `background_file` :793 | name | `backgrounds_dir().join(name)` | NO traversal check (`../config.toml` accepted → path echoed); no `is_bg_file`; wrapper unused (dead) (P3) |
| 45 | `palette_from_background` :806 | name? | `theme::extract_palette` | `/ \ ..` rejected — good |
| 46 | `list_plugins` :844 | — | `plugins::scan` + `commands_for` | ok |
| 47 | `install_pasted_skill` :879 | pack_name, text | `plugins::install_pasted_skill` | BROKEN: JS sends `pack_name`, Tauri expects `packName` (P1-3) |
| 48 | `install_skill_from_git` :889 | url | `plugins::install_skill_from_git` (spawn_blocking) | https-only, no opt-injection, `GIT_TERMINAL_PROMPT=0` — good |
| 49 | `delete_skill` :901 | name | `plugins::delete_skill` | `validate_pack_name` — good |
| 50 | `rename_thread` :906 | id, title | `store.set_title` (120-char cap) | ok |
| 51 | `delete_thread` :913 | id | `orch.kill` ×N + `store.delete_thread` | Logic: re-implements `store::subtree_ids` ancestor walk (P3) |
| 52 | `delete_project` :943 | name | `orch.kill` ×N, `store.delete_project_threads`, `lanes::delete_project` | duplicates core's `default`/traversal check |
| 53 | `list_projects` :971 | — | `lanes::project_views` | ok |
| 54 | `git_branch` :977 | cwd | `tokio::process::Command("git")` | arbitrary cwd (harmless) |
| 55 | `list_files` :993 | root, query | own dir walker | Logic in shell; arbitrary root; bounded depth 4 / 200 |
| 56 | `create_project` :1033 | name, root | `fs::create_dir_all` + hand-written TOML | name validated; `root` written unescaped into TOML (P3) |
| 57 | `toggle_plugin` :1051 | name, enabled | `plugins::set_enabled` | matches by manifest name, marker file — ok |
| 58 | `read_text_file` :1060 | path | `fs::read` | ARBITRARY absolute path read, 2 MiB cap (P2-2) |
| 59 | `list_project_docs` :1094 | project, root | own walker | Logic in shell; `project` used in `projects_dir().join(project)` unvalidated (read-only) |
| 60 | `write_text_file` :1155 | path, content | `fs::create_dir_all` + `fs::write` | ARBITRARY absolute path write, creates parents (P2-2 / P1-1) |
| 61 | `save_project_system` :1182 | project, content | `fs::write(projects/<p>/SYSTEM.md)` | `/ \ ..` rejected — good |
| 62 | `create_skill` :1200 | name | `plugins::create_commands_pack` | validated — good |
| 63 | `skill_commands` :1206 | name | `plugins::commands_for` | good |
| 64 | `save_skill_commands` :1214 | name, commands | `plugins::save_commands` | good |
| 65 | `open_external_url` :1224 | url | `cmd /C start "" <url>` | allowlist ok; launcher is a cmd-metachar sink and functionally broken (P1-2) |
| 66 | `window_minimize` :1258 | window | `window.minimize` | redundant with `getCurrentWindow()` + granted caps (P3) |
| 67 | `window_maximize` :1263 | window | maximize/unmaximize | redundant |
| 68 | `window_close` :1275 | window | `window.close` | redundant |
| 69 | `window_start_dragging` :1280 | window | `start_dragging` | redundant |
| 70 | `migrate_tasks` :1285 | project | `lanes::migrate_tasks_to_lanes` | `project` unvalidated (`projects_dir().join(project)`), read+create; wrapper unused (dead) |
| 71 | `login_antigravity` :1290 | — | `antigravity_oauth::{auth_url, open_browser, wait_for_code, exchange_code}` + keyring | no consent gate in command or UI; OAuth has no `state`/PKCE (P2-1) |
| 72 | `logout_antigravity` :1315 | — | keyring delete ×3 | ok |

TS→Rust cross-check: every `invoke()` name in `ui/src/lib/api.ts` exists in `generate_handler!` (no phantom commands). Rust→TS: `get_provider_health`, `reset_circuit_breaker` have no wrapper; `background_file`, `list_backgrounds`, `upload_background`, `migrate_tasks`, `list_packs` have wrappers no component calls. Seven dead commands.

---

## 2. Findings

### P0 — none
No blocker that prevents build/launch. The three shipped-feature breakages below (P1-2, P1-3, P1-4) should gate the next tag.

### P1

**P1-1 — No CSP, while the webview holds file-write and process-launch primitives.**
`src-tauri/tauri.conf.json:13-18` — `app.security` has only `assetProtocol`; no `csp` key. In Tauri v2 a null CSP means no CSP header is injected at all: inline/remote scripts, `connect-src *`, `img-src *` all permitted. The UI renders LLM output through `{@html}` in 9 places (`Thread.svelte:203,272,288`, `Widget.svelte:43`, `ArtifactCard.svelte:84,87`, `DocReader.svelte:214`, plus SVG marks) behind DOMPurify (`md.ts:98`). DOMPurify is the only barrier; one bypass or one raw-HTML slip (md.ts builds its own `<div class="codeblock">…` HTML before sanitizing) and the attacker has every command in §1 — including `write_text_file` (arbitrary path + `create_dir_all`, e.g. `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\x.bat`), `save_config` (MCP `command`/`args` = arbitrary process on next run), `open_external_url` (cmd metachar sink, P1-2), `read_text_file` (exfil via `<img src=https://…>` since `img-src` is unrestricted).
User-visible: none until an XSS lands; then silent RCE from a chat message.
Fix: add `"csp": "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' asset: http://asset.localhost data: blob:; font-src 'self' data:; connect-src ipc: http://ipc.localhost"` and test highlight.js/fonts; scope `write_text_file`/`read_text_file` (P2-2). Effort: S for CSP, M for scoping.

**P1-2 — `open_external_url` launches via `cmd /C start "" <url>`: functionally broken and a command-injection sink; its only two callers are broken.**
`src-tauri/src/main.rs:1236-1237`:
```rust
let status = std::process::Command::new("cmd")
    .args(["/C", "start", "", &url])
```
`&`, `%`, `^`, `|` are cmd metacharacters; Rust does not quote them for `cmd.exe`. Caller 1, `SystemSection.svelte:143-148`, builds `…/issues/new?title=<enc>&body=<enc>` → cmd splits at `&body=`: the browser gets only `?title=`, and `body=…` runs as a command; `%20`-style escapes are eaten by env-var expansion. Caller 2, `ConnectorsSection.svelte:820`, passes `p.docs` = `https://github.com/modelcontextprotocol/servers/...` (`mcpPresets.ts:48-217`) which the allowlist (`main.rs:1225-1228`, `lucas19919/Parzi/` + `parzi/parzi/`) rejects → every "docs" button shows "that link isn't allowed". The repo already documents this exact hazard and the correct fix in `crates/parzi-providers/src/antigravity_oauth.rs:63-69` (`rundll32 url.dll,FileProtocolHandler`).
User-visible: Report-an-issue opens a title-only issue (or nothing); MCP docs links dead.
Fix: use `tauri-plugin-opener` (`opener:allow-open-url` with a URL scope) or reuse `antigravity_oauth::open_browser`; widen allowlist to `https://github.com/modelcontextprotocol/`. Effort: S.

**P1-3 — `install_pasted_skill` cannot be invoked: snake_case arg key.**
`ui/src/lib/api.ts:343-344`: `invoke("install_pasted_skill", { pack_name, text })`. `main.rs:879-882` declares `pack_name: String` without `#[tauri::command(rename_all = "snake_case")]` (the only `rename_all` in the file is serde, line 26). Tauri v2 expects `packName` → every call fails with `missing required key packName`. Both Settings › Skills flows depend on it (`SkillsSection.svelte:80` paste, `:127` New-skill composer).
User-visible: "Add" on a pasted skill and "Create" in the composer always error.
Fix: send `packName` (or add `rename_all`). Effort: S. (No PROGRESS line claims an end-to-end paste test; :554 only lists the UI.)

**P1-4 — Fresh installs get no wallpaper: bundled resource path is wrong twice.**
`tauri.conf.json:51` `"resources": ["../assets/backgrounds/asuka.png"]` — Tauri v2 maps a `..` component to `_up_`, and the installed app on this machine proves it: `%LOCALAPPDATA%\Parzi\_up_\assets\backgrounds\asuka.png`. `main.rs:686-690` resolves `"asuka.png"` under `BaseDirectory::Resource` (= `<install>\asuka.png`, absent) and `parzi-core/src/paths.rs:71-77` seeds from `<exe dir>\asuka.png` (absent) or cwd `assets/backgrounds/asuka.png` (dev only). PROGRESS:612 explicitly skipped the `_up_` lookup because an installed-app screenshot "proves the wallpaper already loads" — on a dev machine whose `~/.parzi/backgrounds/asuka.png` was seeded on Sep 9 from the repo. Presence is not truth.
User-visible: first-run users see a solid stage; the "one constant background picture" of PLAN §0/§9 is missing.
Fix: `"resources": { "../assets/backgrounds/asuka.png": "asuka.png" }` (map form renames) and keep `resolve("asuka.png", Resource)`; make `seed_default_background` accept the resource path from the shell (core cannot know it). Effort: S. Verify on a clean VM/user.

**P1-5 — Queued runs never stream to the UI.**
`main.rs:198-229` forwards events from the per-call `rx` only. `orchestrator.rs:257-262` returns `closed_rx()` when the run is queued (forwarder exits immediately), and `orchestrator.rs:691-693` `if Self::launch(p.clone(), q).await.is_err() { return; }` drops the `Ok(rx)` when the pump later launches it. No `Text`/`Done`/`Usage`/`Error` for any queued run ever reaches the webview (approvals still do, via `GuiApprover`). The UI knows only "queued — starts when a slot frees" (`App.svelte:294`) and there is no polling (`setInterval` absent).
User-visible: the 5th concurrent thread looks dead; output appears only when the user re-opens it later. PLAN §6 "process manager" and §11 "token emit p50 <100ms" are false for anything past `max_concurrent`.
Fix: one `broadcast::Sender<(SessionId, RunEvent)>` on the Orchestrator subscribed once in `setup`; `launch` fans every run's `tx` into it; drop per-call forwarding. Shared with the runtime auditor. Effort: M.

**P1-6 — `parzi kill` cannot cancel a run owned by the GUI (or vice versa).**
`crates/parzi-cli/src/main.rs:325-331` builds a fresh `Orchestrator` (empty `handles`) and calls `kill` → `orchestrator.rs:463-473` only cancels tokens it owns, then `store.set_status(Killed)`. The handler checks only its in-process token (`handler.rs:298,401,496`). The GUI's run continues and its `finish()` overwrites the status. PLAN §5 "Stop button + `parzi kill` cancel mid-tool" and §6 "CLI mirrors … kill" are false across processes — which is the only case `parzi kill` exists for.
Fix: a `sessions/<id>/kill` marker (or `status == Killed` re-read) polled by the handler at each step/tool boundary. Effort: M.

**P1-7 — Shell is not glue: 72 commands (cap 10, documented deviation 11), 1419 lines (rule <400), business logic inside.**
`main.rs:1359-1416` registers 72; PLAN §9 caps at 10; PROGRESS:47 recorded 14 as the deviation, :372 "14 → 11". Logic implemented in the shell: subsession creation branch in `send_message` (:150-190, duplicating `orch.create_subsession`), ancestor walk in `delete_thread` (:917-933, duplicating `store::subtree_ids`), `toggle_favorite`, `decode_b64`, `list_files`, `list_project_docs`, `create_project` TOML writer, `open_external_url` launcher. None of it is unit-testable (src-tauri has no tests).
Fix: move the walkers/decoder/favorites into core; call `orch.create_subsession` from `send_message`; collapse the window_* quartet and the seven dead commands; keep the count in PLAN or amend PLAN. Effort: M.

**P1-8 — No CI runs tests, clippy or fmt. Release builds untested code.**
`.github/workflows/` contains only `release.yml`, triggered on `v*` tags and `workflow_dispatch`; no `cargo test`/`clippy`/`svelte-check` step anywhere. PLAN §11 "clippy all+pedantic deny in CI", §12 "LOOP.md clippy gate" are claims without a mechanism; the workspace only sets `clippy::all/pedantic = "warn"` (root `Cargo.toml`), and `src-tauri` inherits neither `[lints]` nor `unsafe_code = "forbid"`.
Fix: `ci.yml` on push/PR: `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, `cargo check` in `src-tauri`, `npm --prefix ui run check`; make release depend on it. Effort: S.

### P2

**P2-1 — Antigravity OAuth: no `state`/PKCE, no consent gate in the GUI.**
`antigravity_oauth.rs:54-59` builds the URL without `state`/`code_challenge`; `wait_for_code` (:141-176) accepts the first GET on `127.0.0.1:51121` carrying `code=` from any path/origin. During the 5-minute window any web page can fire `<img src="http://localhost:51121/oauth-callback?code=ATTACKER">` → Parzi signs into the attacker's Google account (login CSRF; user's prompts flow through it). The CLI asks y/N with the ban warning (`cli main.rs:340-350`); the GUI (`ModelsSection.svelte:233-245`) invokes `login_antigravity` straight from a click with only a roster hint (`:44`). PLAN §3 requires "opt-in toggle + warning". Fix: random `state` + PKCE verified in the callback; require `/oauth-callback` path; `confirm()` dialog (plugin already present) or a config flag checked inside the command. Effort: S/M.

**P2-2 — `read_text_file` / `write_text_file` are unscoped.** `main.rs:1060-1080, 1155-1179`. Any absolute path readable/writable by the user, parents auto-created. Intended for `~/.parzi/projects/*/SYSTEM.md` and workspace `.md` docs. Fix: accept only paths under `~/.parzi` or a registered project root (canonicalize, then `starts_with`). Effort: S.

**P2-3 — Corrupt `config.toml` = app exits silently.** `main.rs:1325` `boot().expect("parzi home")`; `config.rs:232-243` errors on parse/version mismatch; `#![windows_subsystem = "windows"]` (:4) hides the panic. User double-clicks, nothing happens. Fix: on `Err`, show a native message box (WinAPI `MessageBoxW` or `rfd`) naming the file, rename it to `config.toml.broken`, continue with defaults. Effort: S.

**P2-4 — `parzi init` overwrites a corrupt config despite "never overwrites".** `cli main.rs:172-176` `ParziConfig::load().unwrap_or_default()` then `save()`: a parse error → defaults written over the user's MCP servers/favorites. Fix: distinguish missing (default) from unparsable (bail with path). Effort: S.

**P2-5 — Cross-process status stomping.** `cli main.rs:247` `orch.recover().ok()` in `send` flips every `Active` session (including GUI-owned) to `Idle` on disk; GUI boot (`main.rs:1327`) does the same to CLI runs; `send <id>` on a GUI-live session passes the `handles` check (empty in the CLI) and two processes append to one `events.jsonl`; `max_concurrent` is per-process. Fix: a `sessions/<id>/lock` with pid+heartbeat; `recover()` skips live pids. Effort: M.

**P2-6 — Updater cannot succeed as shipped.** Endpoint `github.com/lucas19919/Parzi/releases/latest/download/latest.json` on a PRIVATE repo (PROGRESS:536) with `releaseDraft: true` → unauthenticated GET → 404 → `check()` throws → footer silently `idle` (`updateStore.ts`), Settings shows "Update check failed". Acknowledged at PROGRESS:577 but v0.1.6 users still get no updates. Also `workflow_dispatch` with `tagName: ${{ github.ref_name }}` from a branch yields a release tagged `main`. Fix: publish the repo or host `latest.json` on a public endpoint; guard `workflow_dispatch` with an explicit `tag` input. Effort: S (config), policy decision.

**P2-7 — No single-instance guard.** No `tauri-plugin-single-instance`; every launch (or the `deleteAppData`-less installer relaunch) starts another orchestrator on the same `~/.parzi`, whose `recover()` marks the first instance's runs `Idle`, and both pumps serve one queue. PROGRESS:204 saw the symptom in dev. Fix: add the plugin, focus the existing window. Effort: S.

**P2-8 — Approval event carries no session; concurrent approvals collide.** `main.rs:35` `Approval { key, call }` — `ToolCallView` has `lane` but no session id; `App.svelte:621` keeps a single `approval` slot, so a second run's request overwrites the first, which is auto-denied after 120 s (`main.rs:68-73`). Fix: add `session` to the event; UI queue per session. Effort: S.

**P2-9 — Floating action refs with the signing key in env.** `release.yml:28-49`: `tauri-apps/tauri-action@v0`, `dtolnay/rust-toolchain@stable`, `Swatinem/rust-cache@v2`, `actions/*@v4`; `TAURI_SIGNING_PRIVATE_KEY` is exported into the tauri-action step. A compromised tag on any of them exfiltrates the update-signing key (= silent malicious updates to every install). Fix: pin by commit SHA; run `npm ci` and cargo steps before the action so only the sign step sees the secret. Effort: S.

**P2-10 — Release profile does not apply to the GUI.** `[profile.release]` (opt-level z, lto, strip, codegen-units 1) is in the root `Cargo.toml`; `src-tauri/Cargo.toml:7` declares its own `[workspace]`, so it is a separate root and uses Cargo defaults (opt-level 3, no LTO, no strip). PLAN §11 not met for the shipped binary; also no `[lints]` inheritance and no `#![deny(unsafe_code)]` in main.rs. Fix: copy the profile + lints into `src-tauri/Cargo.toml`. Effort: S.

**P2-11 — No logs anywhere.** GUI: no `tracing` subscriber at all (grep `tracing` in main.rs = 0) → every `tracing::warn!` in runtime/providers is dropped. CLI: `cli main.rs:133` stderr at WARN only. `~/.parzi/logs/` is created (`paths.rs:48`) and empty on this machine since Sep 9. PLAN §2/§10 "logs/parzi.log (rotated)" false; Copy-diagnostics has nothing to copy on a crash. Fix: `tracing-appender` daily rolling file in both binaries, INFO, secrets already absent. Effort: S.

**P2-12 — CLI id resolution is ambiguous and destructive.** `cli main.rs:486-495`: first prefix match wins silently (two sessions `3f…` → `fork`/`kill` hit whichever `list()` returns first); empty string matches the newest; `send <id>` (:283) skips `resolve_id` entirely so prefixes work for `show/fork/kill` but not `send`. Fix: collect matches, error on >1, use in `send`. Effort: S.

**P2-13 — `parzi send` exits 0 on run failure.** `cli main.rs:308-311` prints `error:` and breaks, `main` returns `Ok(())`. The "interop surface for other harnesses" cannot detect failure. Fix: return `Err` / `std::process::exit(2)`. Effort: S.

**P2-14 — MCP env secrets in plaintext config and over IPC.** `config.rs:99` `McpServerCfg.env: HashMap<String,String>` (typical: `GITHUB_PERSONAL_ACCESS_TOKEN`) is stored in `~/.parzi/config.toml` and returned whole by `get_config` (`main.rs:518`). Doctor and diagnostics stay presence-only (verified), so this is disk + IPC exposure, not logging. Fix: keyring-backed `env` values (`keyring://name` indirection). Effort: M.

**P2-15 — Uninstaller checkbox promises a deletion that never happens.** `installer/English.nsh:8` `deleteAppData` = "Also delete my threads, settings and local data". Tauri's NSIS template removes `$APPDATA\com.parzi.app` / `$LOCALAPPDATA\com.parzi.app` (WebView2 data) — Parzi's threads/settings live in `%USERPROFILE%\.parzi`, which it never touches. Fix: `hooks.nsh` `NSIS_HOOK_POSTUNINSTALL` that `RMDir /r "$PROFILE\.parzi"` when the box is ticked, or reword the string. Effort: S.

### P3

- **`app_version` returns `0.1.6-v3`** (`main.rs:84`, hardcoded suffix) — shown in sidebar, About, and issue bodies. S.
- **`read_attachments` comment "Contain to cwd (lexical)" is false** (`main.rs:297-299`): `base.join(p)` with an absolute `p` is `p`; no `..` check. Delete the comment or implement it. S.
- **`save_background_data`**: hand-rolled base64 in the shell (`:749`), no size cap on the IPC payload, no extension/magic check, `set_background` result swallowed (`:788`) → "uploaded" but unusable non-image files. Use `base64` crate in core, cap 20 MB, validate ext. S.
- **`background_file`** (`:793`): accepts `..`, skips `is_bg_file`; dead anyway — delete. S.
- **Seven dead commands** (`get_provider_health`, `reset_circuit_breaker`, `background_file`, `list_backgrounds`, `upload_background`, `migrate_tasks`, `list_packs`) and the **window_* quartet** duplicating `getCurrentWindow()` (which `Titlebar.svelte:68-96` already falls back to and capabilities already permit). S.
- **Capabilities redundancy** (`capabilities/default.json`): `core:window:default`, `core:event:allow-emit`, `core:event:allow-listen` are inside `core:default`; `dialog:default` grants save/message/ask/confirm though only `open` is used. Webview emit-forgery of `parzi://run-event` only affects the webview's own state (Rust never listens) — no privilege gain. S.
- **`create_project` writes `root` unescaped into TOML** (`:1043`): a `"` or newline yields an unparsable `parzi.toml`. Use `toml::to_string`. S.
- **`toggle_favorite` writes disk but not `orch.apply_config`** → in-memory config stale; a later Settings save from an older snapshot reverts favorites. S.
- **`kill_run` on an idle thread marks it Killed** (`orchestrator.rs:471`). S.
- **`list_mcp_tools` / `list_all_mcp_tools`** duplicate the view mapping. S.
- **cfg `RwLock` poison → `unwrap_or_default()`** (`orchestrator.rs:177-179`, `Pump::cfg_snapshot`) silently substitutes an empty config (no MCP servers) instead of failing loudly. Only `apply_config` takes the write lock and cannot panic, so practically unreachable; still, log it. S.
- **CLI nits**: `CliApprover { yes }` field is always `false` (`:249`, dead); `--yes` approves `shell.exec` with no per-tool granularity (documented, but a footgun — and `default_mode = "auto"` bypasses the approver entirely); `parzi logout <any>` deletes keyring slots `<p>`, `<p>-refresh`, `<p>-session` for any string — `parzi logout anthropic` deletes the Anthropic API key; `export` lacks PLAN's `--md` flag (it is `show` without `--json`); `--effort` help says `med`, default is `medium`. S each.
- **Asset scope excludes project roots**: `Omnibar.svelte:616-622` calls `convertFileSrc` on attachment paths outside `$HOME/.parzi/backgrounds/*` → asset protocol 403 → broken thumbnails. Either scope `$HOME/**` for images or drop thumbnails. S.
- **Per-token `app.emit`** (`main.rs:228`) with an unbounded channel and no coalescing; the UI's 60 ms debounce does not reduce IPC count. Batch `Text` deltas every ~30 ms. M.
- **Four `let _ = app.emit(...)`** — emit failures invisible (no logger to send them to anyway).
- **`run_doctor` full** builds a second `McpManager` (`doctor.rs:126-129`) → duplicate MCP server processes next to the orchestrator's. Runtime-owned. S.
- **Stray root `package.json`** (untracked): `"version": "1.0.0"`, `"license": "ISC"`, `"main": "index.js"` — an `npm init` artifact contradicting MIT/0.1.6. Delete. S.
- **`install_skill_from_git`** clones arbitrary https repos into `~/.parzi/plugins` — prompt-only packs, so no code exec; note that `discover_skill_dirs` follows the clone's contents blindly (depth 2). Acceptable.

---

## 3. Verified good

- Navigate-fallback/dev-server-probe hunk absent from `main.rs` (working tree and HEAD); `custom-protocol` feature present; `devUrl`/`beforeDevCommand` restored; `frontendDist` correct.
- `GuiApprover`: timeout (120 s) → `Deny`; keyed by fresh UUID per request; entry removed on timeout; `approve_tool` with unknown/expired key → error, not approve; kill during approval → `Deny` (`handler.rs:495-497` selects on cancel).
- State: `tokio::sync::Mutex` for `pending`/`handles`/`queue` (no poisoning; guards not held across awaits beyond single inserts); config behind `std::RwLock` read-and-clone, never across `.await`; `Arc<Orchestrator>` shared with the pump task spawned once in `setup`.
- Secrets: `save_key` writes keyring only, trims, rejects empty, maps to the provider's key slot so subscription tokens are never overwritten; `get_config` exposes no API keys (`ProviderEntry` = `default_model`, `base_url`); `doctor` is presence-only; `get_models` exposes account label only; no `tracing` call in providers/runtime mentions key/token/header (grep clean); `.gitignore` excludes `*.key`, `*.sig`, `.env*`.
- OAuth listener binds `127.0.0.1` (not `0.0.0.0`), fixed port, 5-min timeout, tokens straight to keyring; `open_browser` uses `rundll32` (correct on Windows).
- Asset protocol scope is narrow (`$HOME/.parzi/backgrounds/*`, `$RESOURCE/*`); Tauri canonicalizes existing paths so `..` cannot escape it.
- Name validation on every disk-touching name except `background_file`/`migrate_tasks`: packs, skills (≤64, ascii), project delete/SYSTEM.md, `set_background`, `palette_from_background`, `create_project`; manifest `entry` traversal rejected (`plugins.rs:150-164`).
- `install_skill_from_git`: https-only, whitespace rejected, URL follows fixed args (no option injection), `GIT_TERMINAL_PROMPT=0`, depth 1, temp dir cleaned, `spawn_blocking`.
- Bounded I/O: `get_models` 10 s per provider in a `JoinSet`, `refresh_provider` 15 s, MCP tool probes 20 s, doctor MCP 15 s; `read_text_file`/`write_text_file` 2 MiB; `list_files` depth 4/200, `list_project_docs` depth 2/80; attachments 8 × 12 k chars.
- `delete_thread`/`delete_project` kill live runs before removing directories; `delete_project` refuses `default`.
- `open_external_url` allowlist prevents open redirect (the launcher, not the allowlist, is the bug).
- Capabilities scoped to window `main`; no `shell`, `fs`, `http`, `process` plugin permissions; updater pubkey present; `dialog: false` is driven by `updateStore.ts` + `SystemSection.svelte` (check → downloadAndInstall with progress).
- Icons complete (`icon.ico`, `icon.icns`, all PNG sizes); `LICENSE` present; `ui/package-lock.json` present for `npm ci`; installer `currentUser`, no admin; `hooks.nsh` is inert cosmetics (colours, strings, DWM dark title bar) — no shell-outs.
- `release.yml`: job-level `permissions: contents: write` only; key via secret; draft release = manual publish gate; `fail-fast: false`.
- CLI: zero webview deps (`Cargo.toml`), workspace `exclude = ["src-tauri"]`; safe default `Deny` when stdin is EOF/piped; y/N prompt prints tool name, lane, args; `Text` → stdout, everything else → stderr (pipe-clean); `queue_when_busy = false` for one-shot; `doctor` exits 1 on any failing check.

---

## 4. Tests to add

1. **IPC arg-casing contract**: script (CI) that parses `generate_handler!` param names from `main.rs` and every `invoke("<cmd>", {…})` object in `api.ts`, asserting camelCase keys match — would have caught P1-3. Or a vitest with `@tauri-apps/api/mocks` `mockIPC` calling every `api.*` wrapper.
2. **Dead/phantom command diff**: same script, both directions; fail on unreferenced commands (P1-7 hygiene).
3. **Command-count gate**: fail CI when `generate_handler!` exceeds the PLAN number.
4. **Bundled resource resolution**: post-build test that `<bundle>/asuka.png` exists at the path `background_url` resolves, plus a `seed_default_background` test with a temp `HOME` and a fake resource dir (P1-4).
5. **`open_external_url`**: unit test of the allowlist (`…/Parzi/&calc` rejected or safely passed) and an integration test that a URL containing `&` and `%20` reaches the launcher intact (P1-2).
6. **GuiApprover**: timeout → Deny; unknown key → Err; double approve second → Err; two concurrent approvals get distinct keys and independent replies; kill during approval → Deny.
7. **Queue streaming**: mock provider, `max_concurrent = 1`, spawn two runs; assert the second run's `Done` reaches the shell's sink (P1-5).
8. **Cross-process kill**: run in process A, `parzi kill` from process B, assert the run ends within N s and status stays `Killed` (P1-6).
9. **CLI `resolve_id`**: ambiguous prefix → error; empty → error; `send <prefix>` works (P2-12).
10. **CLI exit codes**: provider error → non-zero; `doctor` with a failing check → 1 (P2-13).
11. **`parzi init` on corrupt `config.toml`**: refuses / backs up, never overwrites (P2-4).
12. **GUI boot with corrupt config**: dialog shown, app starts with defaults (P2-3).
13. **CSP smoke (WebDriver/CDP)**: thread markdown with `<img onerror>`, `<script>`, remote `<img>`; assert no execution and no outbound request (P1-1).
14. **`write_text_file`/`read_text_file` scope**: paths outside `~/.parzi` and registered roots rejected (P2-2).
15. **OAuth callback**: missing/mismatched `state` rejected; non-`/oauth-callback` path rejected (P2-1).
16. **Release profile**: CI asserts `src-tauri/Cargo.toml` carries `[profile.release]` (or size threshold on `parzi-app.exe`) (P2-10).
17. **Uninstaller `deleteAppData`**: ticked → `~/.parzi` removed; unticked → untouched (P2-15).
18. **Single instance**: second launch exits and focuses the first window (P2-7).
19. **`app_version` == `CARGO_PKG_VERSION`** (P3).
20. **Log file**: after a GUI/CLI run, `~/.parzi/logs/parzi.log` exists and rotates (P2-11).
