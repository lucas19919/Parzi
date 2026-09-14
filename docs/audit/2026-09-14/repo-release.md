# Parzi audit — build, release, CI, repo hygiene, licensing, doc drift

Date 2026-09-14, snapshot ~20:20. HEAD `4d80a72` (tag `v0.1.7`, pushed, CI run 34879855873 **in progress** at time of writing). Read-only; no builds run.

**Caveat:** the tree moved under the audit. Between my first `git status` and the last one: commit `4d80a72` landed (footer update button + bump to 0.1.7, sweeping other lanes' hunks with it), tag `v0.1.7` was pushed, and `anthropic.rs`, `codex.rs`, `antigravity.rs`, `openai_compat.rs` gained new hunks (the OpenCode lane is still typing). Numbers below are from the post-commit snapshot: 51 modified/deleted, 9 untracked, +2177/-464.

---

## 0. The five things that matter

| # | Sev | Finding |
|---|---|---|
| 1 | **P0** | `assets/backgrounds/asuka.png` is Evangelion promotional art (Asuka Langley Soryu, Gainax/khara). It is the default wallpaper, bundled into every installer via `bundle.resources`, seeded into `~/.parzi/backgrounds/`, hardcoded in `theme.rs`, `paths.rs`, built-in packs, tests, and advertised in the README. Three copies in git (identical md5 in `examples/`, plus `ui/design/asuka.jpg`). It is in the initial commit. **The repo cannot go public and the installer cannot be distributed beyond friends until this is replaced and purged from history.** |
| 2 | **P1** | HEAD/`v0.1.7` is internally inconsistent. `SystemSection.svelte:148` (committed) calls `api.openExternalUrl`; `api.ts` and `main.rs` at HEAD do not have it. `App.svelte` at HEAD dropped the `logos` plumbing but `ProviderLogo.svelte`/`providerMarks.ts` are untracked, so v0.1.7 renders **no provider marks anywhere**. `PROGRESS.md` at HEAD claims the OpenCode Zen/Go overhaul is "verified" while `opencode.rs` is untracked — v0.1.7 ships the old `localhost:4096/v1` adapter that the same entry says "could never work". CI will go green anyway because `vite build` does not type-check. |
| 3 | **P1** | Auto-update is non-functional by construction: private repo (`gh repo view` → `isPrivate: true`, `latestRelease: null`), every release is a **Draft**, endpoint is `releases/latest/download/latest.json`. `updateStore.ts` swallows the 404 into `idle`, so nobody will ever see it fail. All the signing/`createUpdaterArtifacts`/updater-plugin work is inert. |
| 4 | **P1** | No CI on push/PR. `.github/workflows/` = `release.yml` only. PLAN §11 says "clippy all+pedantic deny in CI"; it isn't. Every gate is run by hand by whichever lane is active — which is exactly how #2 happened. |
| 5 | **P1** | `THIRD_PARTY_NOTICES.md` is wrong both ways: it attributes `crates/parzi-providers/src/t3.rs` (never existed in any commit) and `ui/src/assets/providers/` (being deleted), and omits lobehub icons (MIT, SVG bodies copied verbatim into `providerMarks.ts`), CLIProxyAPI (`handler.rs:758`), ui-ux-pro-max (`design-system/parzi/MASTER.md`), and the three OFL fonts bundled into `dist`. The file is not in `bundle.resources`, so it doesn't ship anyway. |

---

## 1. Uncommitted-work inventory

Snapshot after `4d80a72`. "Shared" = files also touched by another lane in the same working tree.

| # | Lane (apparent) | Files | Δ | State | Shared with | Documented? |
|---|---|---|---|---|---|---|
| 1 | **Provider logos** (claim file: COMPLETE 13:35) | D 14× `ui/src/assets/providers/*`; ?? `ProviderLogo.svelte`, `providerMarks.ts`; M `Omnibar`, `ProjectOverview`, `Settings`, `ThreadRow`, `inspector/AgentVisualizer`, `inspector/RightPanel`, `settings/ModelsSection`, `theme.css` (−8: tile rule). `App.svelte` part was already swept into `4d80a72`. | ~+60/−120 + 2 new (≈430 lines) | **Complete.** `grep assets/providers ui/src` → 0 hits; no leftover `logos` props. Scope creep: `ThreadRow` also lost `modelLabel`/`.st-model` (the claim said "small edits only"). | `App.svelte` (footer lane — already committed together) | `.lane_claim_provider_logos.md` only; no PROGRESS entry |
| 2 | **OpenCode Zen/Go** | ?? `opencode.rs` (321), `opencode_wire.rs` (395), `tests/opencode.rs` (165, 7 tests); M `catalog.rs` +261, `compat_providers.rs` −37, `lib.rs`, `router.rs`, `types.rs` (+`session`, `sanitize_tool`), `config.rs` (default remap), `handler.rs` (`session`), `orchestrator.rs` (`sticky_spec` + persist on launch), `cli/main.rs`; **still landing during audit:** `anthropic.rs` (effort ladder xhigh/max, sanitize), `codex.rs` (model remaps, sanitize), `openai_compat.rs` (sanitize), `antigravity.rs` (const→enum fix) | ≈+1,200 | Coherent; its own PROGRESS entry claims 72+7 tests green (unverifiable here). **Not finished — hunks appeared at 20:05–20:15.** | `config.rs`, `handler.rs`, `orchestrator.rs`, `Cargo.lock` with lane 3 | PROGRESS §"OpenCode Zen/Go" — **already committed in `4d80a72` without the code** |
| 3 | **MCP connectors rework** | M `config.rs` (`deny`, `tool_modes`, `is_tool_exposed`), `mcp.rs` +57 (RwLock configs, `set_configs`, exposure), `tools.rs` +50 (`approval_override`, `defs_with_mcp`, gates), `handler.rs` (`defs_with_mcp`, override, `ask_approver`), `orchestrator.rs` (`Arc<RwLock<ParziConfig>>`, `apply_config`, `config()` by value), `src-tauri/main.rs` (`save_config` hot-apply, `list_mcp_tools`, `list_all_mcp_tools`, `.config()` call-site fixes), `api.ts`, `ConnectorsSection.svelte` +559 (now 975 lines); ?? `mcpPresets.ts` (15 presets) | ≈+950 | Complete per PROGRESS §"MCP connectors rework"; its OPEN item (icon.ico decode) was resolved by `c77c42c`. | `config/handler/orchestrator/main/api.ts` with lanes 2, 4, 5; `cli/main.rs` change is a consequence of this lane's `config()` signature | PROGRESS (committed 09-14) |
| 4 | **Skills install** | M `plugins.rs` +467 (`install_pasted_skill`, `install_skill_from_git` = `git clone --depth 1`, `delete_skill`, `normalize_git_url`, tests), `main.rs` (3 commands + `PluginView.commands`), `api.ts`, `SkillsSection.svelte` (325 lines changed) | ≈+700 | Looks complete (inline tests, temp cleanup, ssh refused, `GIT_TERMINAL_PROMPT=0`). | `main.rs`, `api.ts` with 3, 5 | **No PROGRESS entry at all** |
| 5 | **Report-an-issue** | M `main.rs` `open_external_url` (allowlist + `cmd /C start`), `api.ts` `openExternalUrl`. The `SystemSection.svelte` caller is **already in HEAD**. | +45 | **Half-committed → HEAD broken** (see finding 2). Has a command-injection hole (P1-5). | `main.rs`, `api.ts` | PROGRESS §"Footer update button" calls it "an uncommitted lane button" |
| 6 | **Favicon / mark** | M `index.html` (`?v=4` busters), `favicon.svg` (redraw), `favicon-32x32.png`, `SettingsNav` (→ `/mark.svg`); ?? `ui/public/mark.svg` | small | Complete | — | none |
| 7 | **Appearance polish** | M `AppearanceSection` (grid auto-fit), `ColorField` (ellipsis) | +5 | Complete, trivial | — | none |
| 8 | **Lock residue** | M `Cargo.lock`, `src-tauri/Cargo.lock` (parzi-* 0.1.0→0.1.6; toml is already 0.1.7) | 8 | Stale; will change again on next `cargo` run | — | — |
| 9 | **Junk** | ?? `package.json` (root), `.lane_claim_provider_logos.md` | — | Delete / ignore | — | — |

**Conflicting edits in the same file:** `config.rs` (2+3), `handler.rs` (2+3), `orchestrator.rs` (2+3), `src-tauri/main.rs` (3+4+5), `api.ts` (3+4+5), `Cargo.lock` (2,3), `PROGRESS.md` (2 — already committed by the footer lane). No textual conflicts (each lane's hunks are additive), but they are interleaved, so a per-lane commit needs `git add -p` on those five files.

**Risk of committing the tree as one blob:** ~2,600 changed lines across five features in one message; `git bisect` becomes useless; the OpenCode lane is mid-edit so the blob freezes a half-state; a new subprocess (`git clone` on a user URL) and a cmd-injection path (P1-5) go in unreviewed; PROGRESS already describes code HEAD doesn't have, and a blob makes that permanent.

**Recommended commit strategy**

1. **Now:** do not publish the v0.1.7 draft. Let CI finish, leave it a draft, tag v0.1.8 after the steps below.
2. Fix P1-5 (`open_external_url`) before it is committed.
3. Commit order (each: `cargo test --workspace`, `cargo clippy` gate, `npm run check`, `npm run build`):
   - **a. `runtime: MCP exposure, per-tool modes, hot-apply config`** — lane 3 (`config.rs` MCP hunks, `mcp.rs`, `tools.rs`, `handler.rs` MCP hunks, `orchestrator.rs` RwLock hunks, `main.rs` MCP + `.config()` fixes, `cli/main.rs`, `api.ts` MCP types, `ConnectorsSection`, `mcpPresets.ts`). Foundation the others adapt to.
   - **b. `skills: paste / git install / delete`** — lane 4 + lane 5's fixed `open_external_url` + `api.ts` (this repairs HEAD).
   - **c. `providers: OpenCode Zen/Go, tool-name sanitizing, effort ladder`** — lane 2, **only once that session says it's done**; includes `config.rs` remap, `handler.rs` `session`, `orchestrator.rs` `sticky_spec`.
   - **d. `ui: inline provider marks, tab mark, appearance polish`** — lanes 1 + 6 + 7 (deletions + 2 new files + 10 svelte/css). Update NOTICES in the same commit (lobehub MIT).
   - **e. `chore: lockfiles for 0.1.7`** — run `cargo update -w` in root and `src-tauri`, `npm i --package-lock-only` in `ui/`.
4. **Drop:** root `package.json`. **Ignore:** `.lane_claim_*` (add to `.gitignore`, or move claims under `.claude/`).

---

## 2. Findings (all)

Format: severity · location · what's wrong · evidence · consequence · fix · effort.

### P0

**P0-1 · Bundled copyrighted character art**
- Where: `assets/backgrounds/asuka.png` (622,815 B), `examples/backgrounds/asuka.png` (byte-identical, md5 `a0592410…`), `ui/design/asuka.jpg`; `src-tauri/tauri.conf.json` `bundle.resources: ["../assets/backgrounds/asuka.png"]`; `crates/parzi-core/src/paths.rs:58-78` (seed), `theme.rs:56,117` (default), `paths.rs:90` (moody-midnight pack), `tests/logic.rs:234`, `README.md` ("Default background: Asuka"), `examples/theme.toml`.
- Wrong: Evangelion key art (Asuka in plugsuit on Unit-02), rights held by khara/Gainax. Not licensable under this repo's MIT. Redistributed in every NSIS/MSI installer.
- Consequence: DMCA/takedown exposure the moment the repo or a release is public; taints the MIT grant (the LICENSE claims to cover "the Software" incl. bundled files); any store/marketplace listing gets rejected.
- Fix: replace with an owned or CC0/own-generated image (or a procedural gradient — `theme.rs` already supports `image = ""`); change every default/test/pack/README reference; remove the three copies; since it's in the initial commit `0e077eb`, rewrite history (`git filter-repo --path assets/backgrounds/asuka.png --path examples/backgrounds/asuka.png --path ui/design/asuka.jpg --invert-paths`) **before** the repo goes public and force-push; re-tag.
- Effort: **M** (the code change is S; history rewrite + retagging is the M).

### P1

**P1-1 · HEAD / v0.1.7 references code that isn't committed**
- Where: commit `4d80a72`; `ui/src/lib/settings/SystemSection.svelte:148` (`await api.openExternalUrl(url)`); `ui/src/lib/api.ts` at HEAD has no `openExternalUrl`; `src-tauri/src/main.rs` at HEAD has no `open_external_url` (only in worktree). `App.svelte` at HEAD: zero `logos`/`ProviderLogo` references while `Omnibar/ThreadRow/...` at HEAD still take `logos` (default `{}`). `PROGRESS.md` at HEAD line 624 "OpenCode Zen/Go routing overhaul … Verified" with no `opencode.rs` in HEAD.
- Evidence: `git show HEAD:… | grep`, `gh run list` shows v0.1.7 building from `4d80a72`.
- Consequence: `svelte-check` fails at HEAD (TS2339). `vite build` passes (no type-check) → v0.1.7 installer will exist, with a Report-issue button that throws "command open_external_url not found", no provider logos in sidebar/omnibar/models, and a PROGRESS that lies about the shipped OpenCode adapter.
- Fix: keep v0.1.7 draft unpublished; commit per §1; add `npm run check` to CI so this class can't recur; the footer lane's habit of `git add` on shared files (its own note: "co-mingles with lane WIP") needs a `git add -p` rule in the lane protocol.
- Effort: **S**.

**P1-2 · Updater can never fetch a feed**
- Where: `src-tauri/tauri.conf.json` `plugins.updater.endpoints[0] = https://github.com/lucas19919/Parzi/releases/latest/download/latest.json`; `release.yml` `releaseDraft: true`; `ui/src/lib/updateStore.ts` (catch → `idle`).
- Evidence: `gh repo view` → `isPrivate: true, latestRelease: null`; `gh release list` → 5 releases, all `Draft`. `releases/latest/download/…` only resolves for the newest **published, non-prerelease** release, and only anonymously on a public repo.
- Consequence: `check()` throws (404/redirect to login) on every boot; footer stays idle; "friend builds" can never update themselves; the minisign keypair, `createUpdaterArtifacts`, `.sig` uploads and the Settings › Updates card are all dead weight until this is fixed.
- Fix (pick one): (a) public repo + publish releases (blocked by P0-1); (b) public feed only — a second public repo `lucas19919/parzi-releases` (or Pages/R2) that `release.yml` uploads `latest.json` + installers to, endpoint pointed there, code stays private; (c) for friends today: ship without updater and say so. In all cases surface `error` (not `idle`) in Settings › System so a broken feed is visible.
- Effort: **S–M**.

**P1-3 · No push/PR CI**
- Where: `.github/workflows/` contains only `release.yml` (tag-triggered).
- Wrong: no `cargo test`, no clippy gate, no `fmt --check`, no `svelte-check`, no `vite build`, no `cargo check` of `src-tauri`, no `cargo audit`/`deny`. PLAN §11 and LOOP §1 describe gates that exist only as lane discipline.
- Consequence: P1-1; every lane re-runs 10 minutes of gates by hand and reports "green" in prose that nobody can verify.
- Fix: `ci.yml` below. Effort: **S**.

**P1-4 · THIRD_PARTY_NOTICES.md does not match the tree**
- Where: `THIRD_PARTY_NOTICES.md`.
- Wrong: (1) attributes `crates/parzi-providers/src/t3.rs` — `git log --all -- …/t3.rs` is empty; the roster was cut to five before the initial commit (PROGRESS §"Model picker + provider routing rework"). (2) attributes `ui/src/assets/providers/` marks "matching T3's Icons.tsx" — directory is being deleted. (3) Missing: **lobehub icons** (`providerMarks.ts:2` "Bodies are the inner markup of the lobehub icon set (MIT)" — MIT requires the copyright notice to travel with copies; SVG path data is copied verbatim); **CLIProxyAPI** (`handler.rs:758` "pattern from MIT CLIProxyAPI"); **ui-ux-pro-max** (`design-system/parzi/MASTER.md:4`, PROGRESS §166); **@fontsource/inter, jetbrains-mono, instrument-serif** — OFL 1.1 fonts compiled into `ui/dist` and hence the installer; OFL §1 requires the license text to accompany distributed font software. (4) The file is not in `bundle.resources` (only `../LICENSE` via `licenseFile`), so end users never receive it.
- Fix: rewrite with the real list (antigravity-auth, t3code patterns, lobehub icons + their copyright line, CLIProxyAPI, ui-ux-pro-max, three OFL notices); add `"../THIRD_PARTY_NOTICES.md"` to `bundle.resources`; link it from Settings › System.
- Effort: **S**.

**P1-5 · `open_external_url` is command-injectable (cross-area: security)**
- Where: worktree `src-tauri/src/main.rs` `open_external_url` (uncommitted): `Command::new("cmd").args(["/C","start","",&url])`; guard = length, no whitespace, prefix in `["https://github.com/lucas19919/Parzi/", "https://github.com/parzi/parzi/"]`.
- Wrong: cmd metacharacters (`&`, `|`, `^`, `<`, `>`, `%`) are not whitespace, and Rust's Windows arg quoting only quotes args containing space/tab/quote — so `https://github.com/lucas19919/Parzi/issues&calc` reaches `cmd` unquoted and runs `calc`. Any script that can call `invoke` (an XSS in the markdown/widget renderer, a rogue MCP tool output rendered unsafely) becomes RCE.
- Fix: never go through `cmd`. Use `tauri-plugin-opener` (`open_url`) or `ShellExecuteW`, validate with `url::Url::parse` + host allowlist. Also drop `https://github.com/parzi/parzi/` — not an org the project owns.
- Effort: **S**.

### P2

**P2-1 · Root `package.json` is `npm init -y` junk** — untracked, created 17:37 today; `"version": "1.0.0"`, `"main": "index.js"` (no such file), `"license": "ISC"` (contradicts `LICENSE` = MIT and `Cargo.toml` `license = "MIT"`), `"type": "commonjs"`, test script `exit 1`, `directories.example`. Probably a by-product of the ajv schema-validation step (PROGRESS §592). Consequence if committed: two licenses declared, tooling (cargo-deny/FOSSA/GitHub license detection) confused, `npm` at root thinks it's a package. Fix: delete; add `/package.json` and `/node_modules/` to `.gitignore` to stop it recurring. **S**.

**P2-2 · Version fields disagree** — `Cargo.toml`, `src-tauri/Cargo.toml`, `tauri.conf.json`, `ui/package.json` = 0.1.7 (committed). `Cargo.lock` and `src-tauri/Cargo.lock` `parzi-*` = 0.1.6 (worktree, uncommitted; at HEAD they say 0.1.0 — the lock has lagged every bump since v0.1.1). `ui/package-lock.json` root `version` = 0.1.0. Root `package.json` = 1.0.0. README has no version. About page reads `app_version` at runtime (fine). Consequence: every `cargo`/`npm` invocation dirties a lockfile; "which version is this" needs four files. Fix: a `scripts/bump.ps1` (or `cargo-release`) that edits all four + runs `cargo update -w` in both roots + `npm i --package-lock-only`; CI job that asserts the four agree with the tag. **S**.

**P2-3 · Non-reproducible release pipeline** — `dtolnay/rust-toolchain@stable` (floating channel; local is 1.98.1 with no `rust-toolchain.toml`), `tauri-apps/tauri-action@v0` (floating major — v0.1.2 already broke on a config-schema change), `Swatinem/rust-cache@v2`, `actions/checkout@v4`, `setup-node@v4`, `node-version: 24`. Fix: `rust-toolchain.toml` (`channel = "1.98.1"`), pin actions to full SHAs with a version comment, Dependabot for actions. **S**.

**P2-4 · No Authenticode / SmartScreen** — `release.yml` header says "Ship signed Windows installers"; only the **updater** (minisign) signature exists. `tauri.conf.json` has no `certificateThumbprint`/`signCommand`. Every friend gets "Windows protected your PC". PROGRESS gate acknowledges it. Fix: Azure Trusted Signing (cheapest, no HSM) → `bundle.windows.signCommand`; or an OV cert. **M + cost**.

**P2-5 · CLI is never released** — `release.yml` only runs `tauri-action`; `target/release/parzi.exe` is built by nobody. README's quick start is CLI-first and PLAN §1 says other harnesses integrate via CLI. Fix: a `cli` job (`cargo build --release -p parzi-cli`, upload `parzi.exe` to the same release). **S**.

**P2-6 · Documentation drift, three files**
- `CHANGES_SUMMARY.md` (Sep 9): describes `TaskPlanningView.svelte` (never existed in git), `TaskConfig`/`create_task`/`list_tasks` IPC (gone; Tasks removed 2026-09-12, only `migrate_tasks` shim remains at `main.rs:1285`), "19/19 tests". Delete or move to `docs/history/`.
- `PROGRESS.md`: 47 KB, 660 lines, append-only, no current-state view. "Human gates still open" is at line 270 of 660 and says "`git init` (no repo yet)" while the repo has 8 pushed tags; P7 screenshot gate is still unchecked after 7 CI releases. 16 U+FFFD replacement characters are **committed** (lines 567-620; a lane wrote the file with the ANSI codepage — `Set-Content` without `-Encoding utf8`). Fix: `STATUS.md` (≤15 lines: version, what ships, open gates, known broken), move PROGRESS to `docs/`, fix the mojibake, keep the gate list at the top.
- `README.md`: doesn't say Windows-only; doesn't mention the updater is inert; "Default background: Asuka" (P0-1). Quick-start commands themselves are correct (verified: `[[bin]] name = "parzi"`, `Init`, `Doctor`, `Send { target, message, --model, --yes }`).
- Effort: **S**.

**P2-7 · `examples/config.toml` is stale** — `default_provider = "anthropic"`, tables for `openai`, `anthropic`, `openrouter`, `ollama`, `claude-code`, `t3` (roster is `claude, codex, antigravity, opencode, xai`; `canonical_id("t3")` → `None`). A user who copies it gets a config that half-resolves. `examples/theme.toml` points at asuka. Fix: regenerate from `ParziConfig::default()` / `Theme::default()` in a test that diffs against the example. **S**.

**P2-8 · The 400-line rule is fiction** — PLAN §11 / LOOP REVIEW gate "files <400 lines". Violators (`wc -l`): `src-tauri/src/main.rs` 1419, `App.svelte` 1339, `Omnibar.svelte` 1128, `orchestrator.rs` 1030, `ConnectorsSection.svelte` 975 (+559 this lane), `theme.rs` 834, `handler.rs` 806, `plugins.rs` 802 (+467 this lane), `Sidebar.svelte` 633, `AppearanceSection.svelte` 581, `ModelsSection.svelte` 566, `catalog.rs` 556, `antigravity.rs` 538, `Thread.svelte` 525, `store.rs` 515, `cli/main.rs` 496, `tools.rs` 466. Either enforce in CI (and split `main.rs`/`App.svelte` first) or delete the rule from PLAN/LOOP so the "REVIEW" step stops being theatre. **S** to delete, **L** to comply.

**P2-9 · No GUI logging, no rotation, silent startup crash** — PLAN §2 `logs/parzi.log (rotated)`, §10 "Logs ~/.parzi/logs/, rotation". Reality: `grep -r rotat|tracing_appender` → nothing. `src-tauri/src/main.rs` installs **no** tracing subscriber (all `tracing::` calls in core/runtime are dropped); CLI installs a WARN stderr subscriber only. `main.rs:4` `windows_subsystem = "windows"` + `main.rs:1325` `boot().expect("parzi home")` → a startup failure (locked config, bad TOML, missing home) is a **silent exit with no dialog and no log**. No crash reporting. Fix: `tracing_appender::rolling::daily` into `~/.parzi/logs/` with a 5-file cap, `std::panic::set_hook` → `tauri_plugin_dialog` message box + log line, "Copy diagnostics" appends the log tail. **M**.

**P2-10 · Fresh-install background seeding is unverified** — `tauri.conf.json` bundles `../assets/backgrounds/asuka.png`; Tauri 2 places parent-relative resources under `$RESOURCE/_up_/assets/backgrounds/asuka.png`. `main.rs:687` resolves `$RESOURCE/asuka.png` (wrong depth); `paths.rs:66-69` looks in CWD `assets/backgrounds/` and `<exe_dir>/asuka.png`. PROGRESS §612 says "Component 4 (asuka `_up_` lookup) SKIPPED — installed-app screenshot proves the wallpaper already loads" — on a machine whose `~/.parzi/backgrounds/asuka.png` was seeded by dev runs days earlier. On a clean user profile the theme points at a missing file. Whatever replaces the image (P0-1) inherits this path bug. Fix: resolve `_up_/assets/backgrounds/<name>` via `BaseDirectory::Resource`, or move the asset under `src-tauri/resources/` so it lands at `$RESOURCE/<name>`; test on a clean Windows user account or VM. **S** (fix) / **S** (verify).

**P2-11 · Provider brand marks** — `providerMarks.ts` carries 13 marks (claude, openai, xai, opencode, antigravity, ollama, gemini, google, openrouter, meta, deepseek, mistral, t3); the roster is 5, so 8 are dead code. Nominative use (identifying the service you route to) is the standard defense and is what most clients do, but Anthropic/OpenAI/Google brand guidelines restrict logo use in third-party UIs; the `t3` mark identifies a service Parzi no longer talks to. Trim to the five live marks; keep the lobehub notice (P1-4). **S**.

**P2-12 · One-click MCP presets are a supply-chain surface (cross-area: security)** — `mcpPresets.ts` installs 15 servers via `npx -y` / `uvx` from Settings. Three are marked community (`mcp-server-sqlite` via `uvx`, playwright, gmail). Several marked `official: true` (`postgres`, `puppeteer`, `slack`, `gdrive`, `gitlab`, `brave-search`) had their reference implementations archived/moved out of `modelcontextprotocol/servers` upstream in 2025 — verify each before labelling; an archived package pinned by `-y` resolves to an unmaintained last version with whatever transitive deps it had. Fix: pin exact versions in `args`, re-verify the `official` flags, show the resolved package+version in the install confirm. **S**.

### P3

**P3-1 · No `.gitattributes`; every git command prints ~40 CRLF warnings** — global `core.autocrlf=true`, no repo attributes. Index is clean (`git ls-files --eol`: 120 `i/lf`, 0 `i/crlf`, 60 `-text` binaries) so nothing is actually wrong on disk; `PROGRESS.md` is `w/mixed`. Fix: `.gitattributes` with `* text=auto eol=lf` and `*.png *.bmp *.ico *.icns *.jpg *.woff2 binary`; `git add --renormalize .`. **S**.

**P3-2 · `.gitignore` gaps** — covers `target/`, `src-tauri/target/`, `node_modules/`, `ui/dist/`, `*.key`, `*.sig`, vite logs, `.env` (all verified with `git check-ignore`). Missing: `.lane_claim_*`, `src-tauri/gen/` (4 generated ACL schema files are **tracked**), root `/package.json`/`/node_modules/`, `*.log`. **S**.

**P3-3 · Tracked binary/generated ballast** — 3.1 MB of 7.5 MB tracked is binaries: `src-tauri/icons/android/**` and `ios/**` (never built; 40 files), `src-tauri/parzi-icon-src.png` 627 KB (a serif "P" raster — obsolete since the circle-lines mark in `c77c42c`, so "icon source" is now misleading), `installer/dialog.bmp` 615 KB (WiX needs BMP; fine), duplicate asuka (P0-1). Remove android/ios, remove or replace `parzi-icon-src.png` with `icons/icon.svg` as the declared master. **S**.

**P3-4 · Two lockfiles** — shared deps agree (tokio 1.53.1, serde_json 1.0.151, keyring 4.2.0, uuid 1.26.0, serde 1.0.229) — good. But `src-tauri/Cargo.lock` has **both** `reqwest 0.12.28` (providers) and `0.13.5` (tauri-plugin-updater) → two HTTP/TLS stacks compiled into the GUI binary. 591 vs 305 packages. Bump workspace `reqwest` to 0.13 when the providers' features allow. **S**.

**P3-5 · `LICENSE`: "Copyright (c) 2026 Parzi"** — "Parzi" is not a legal person; `tauri.conf.json` `copyright`/`publisher` likewise. Use the author's name. **S**.

**P3-6 · No changelog** — `releaseBody` is the static "See the assets below…"; tags v0.1.0–v0.1.7 in one day with commit messages as the only history. Add `CHANGELOG.md` (keep-a-changelog) and read the section into `releaseBody`. **S**.

**P3-7 · Windows-only, not stated** — matrix is `windows-latest` only; `bundle.targets: "all"` → NSIS (per-user, the "friend build") + MSI (per-machine, admin, Error 1925 without it — PROGRESS documents this). README and About don't say "Windows only". `main.rs` has macOS/Linux `open` branches that nobody builds. **S** (doc).

**P3-8 · `unwrap`/`expect` outside tests** — rule says none; 7 real sites: `antigravity_oauth.rs:44,45,50` (static header strings), `mcp.rs:224,272` (`expect("live after ensure")` invariant), `src-tauri/main.rs:1325,1418` (boot; see P2-9 for why the first one matters). Everything else is under `#[cfg(test)]`. **S**.

**P3-9 · 29 GB of build dirs on disk** — `target/` 19 GB + `src-tauri/target/` 9.9 GB (ignored correctly). Parallel lanes each rebuild; set `CARGO_TARGET_DIR` shared, `cargo clean` periodically. **S**.

**P3-10 · Design artefacts inside the Vite root** — `ui/design/*.dc.html`, `canvas.json`, `build-standalone.mjs`, `parzi-redesign*.html`, `asuka.jpg` are tracked under `ui/` (not bundled, but a stray design-tool workspace in the app dir). `design-system/parzi/MASTER.md` is the real SoT for agents and is fine. Move `ui/design/` to `docs/design/`. **S**.

**P3-11 · `ui/package-lock.json` root version 0.1.0** — `npm ci` tolerates it; cosmetic until someone reads it. Covered by P2-2's bump script.

---

## 3. Spec vs reality (PLAN.md "frozen")

| Topic | PLAN.md says | Reality (evidence) |
|---|---|---|
| Crates | 3 lib + 1 bin, "no more"; `src-tauri` "wires, never implements" | 4 workspace crates + detached `parzi-app`; `src-tauri/src/main.rs` = 1,419 lines, 72 `#[tauri::command]`, implements tool-browser aggregation, skill install, palette extraction, background upload, git branch probing |
| IPC cap | 10 max | 14 (P7, documented) → 66 at v0.1.6 → **72** in worktree |
| MCP | `rmcp` 3.2.0 client + Streamable HTTP v0.2 | hand-rolled NDJSON JSON-RPC in `mcp.rs`; `rmcp` absent from both lockfiles (documented deviation, P5); no HTTP transport |
| Providers | 10 adapters (openai, anthropic, xai/grok-cli, openrouter, ollama, opencode, codex, claude-code, antigravity) | roster **5** (`PROVIDERS` in `lib.rs:27`); README says five (✓); PROGRESS P2+P3 says "router for 9 ids"; `examples/config.toml` lists 10 incl. `t3`; `providerMarks.ts` has 13; NOTICES attributes a `t3.rs` that never existed |
| opencode | `opencode serve` endpoint | hosted Zen/Go bases (uncommitted lane; PROGRESS entry already committed) |
| Settings | 5 flat pages: Appearance / Models / Connectors / Plugins / Projects+About | **7** sections: General, Appearance, Models, Connectors, Skills, Context, System (`SettingsNav.svelte:26-32`) |
| Concepts not in PLAN | — | Tasks (added 09-09, removed 09-12, `migrate_tasks` shim lives on), Skills (commands packs → paste/git install), Context section, Artifacts (`artifacts.rs`, `ArtifactCard.svelte`), circuit breaker + failover + queue, sticky auto-routing, inspector deck (Docs + Agents), theme packs, palette-from-background, sub-sessions/reparent/pin, "Scheduled Tasks" sidebar row (PROGRESS lines 220/345 — `grep -ri schedul` finds **no implementation**) |
| Background | `[background].image`, sketch uses `rooftop.png` | asuka.png hardcoded as default in `theme.rs:117`, `paths.rs:60`, built-in pack, test |
| Logos | `ui/src/assets/providers/*.svg`, "never touch Rust for icons" | moving to inline SVG in `providerMarks.ts`; Rust untouched (✓) |
| Files <400 lines | rule + REVIEW gate | 17 violators (P2-8) |
| No `unwrap` outside tests | rule | 7 sites (P3-8) |
| Clippy | "all+pedantic deny in CI" | workspace lints are `warn`; LOOP gate is correctness/suspicious/complexity/perf `-D`, run by hand; **no CI** |
| Logs | `~/.parzi/logs/parzi.log (rotated)` | none in GUI (no subscriber); CLI stderr WARN (P2-9) |
| Updater | "Tauri signed updater, stable channel only" | wired and signed; feed unreachable (P1-2) |
| Budgets (CLI <25 MB, GUI <150 MB, cold start <1.5 s) | rule | no measurement anywhere in PROGRESS |
| Quick start | — | `cargo build -p parzi-cli` → `target/debug/parzi.exe` ✓ (`[[bin]] name = "parzi"`); `init` ✓, `doctor` ✓, `send new "hello" --model auto --yes` ✓; `cargo tauri dev` needs `tauri-cli` ✓ (documented). Grok → `compat_providers::xai` ✓ keys-only |
| Human gates | P3 creds, P7 screenshot, P9 signing halt the loop | P7 still open after 7 releases; P9 half-open (SmartScreen); list buried at PROGRESS:270 and says "no repo yet" |

---

## 4. Proposed `ci.yml`

```yaml
name: CI
on:
  push: { branches: [main] }
  pull_request:
concurrency: { group: ci-${{ github.ref }}, cancel-in-progress: true }
jobs:
  rust:
    runs-on: windows-latest            # matches the only release target; WebView2 present
    steps:
      - uses: actions/checkout@<sha>   # v4
      - uses: dtolnay/rust-toolchain@<sha>
        with: { toolchain: 1.98.1, components: "clippy, rustfmt" }   # or read rust-toolchain.toml
      - uses: Swatinem/rust-cache@<sha>
        with: { workspaces: ". -> target\nsrc-tauri -> target" }
      - run: cargo fmt --all --check
      - run: cargo clippy --workspace --all-targets -- -D clippy::correctness -D clippy::suspicious -D clippy::complexity -D clippy::perf   # LOOP §1 gate, verbatim
      - run: cargo test --workspace
      - run: cargo check --manifest-path src-tauri/Cargo.toml   # generate_context!, icon decode, config schema
      - run: cargo install cargo-deny --locked && cargo deny check advisories licenses bans   # add deny.toml (allow MIT/Apache-2.0/OFL-1.1/BSD/ISC/Zlib/Unicode)
  ui:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@<sha>
      - uses: actions/setup-node@<sha>
        with: { node-version: 24, cache: npm, cache-dependency-path: ui/package-lock.json }
      - run: npm --prefix ui ci
      - run: npm --prefix ui run check      # svelte-check — would have caught P1-1
      - run: npm --prefix ui run build
      - run: npm --prefix ui audit --audit-level=high
  hygiene:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@<sha>
      - run: |                                # versions agree
          v=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
          grep -q "\"version\": \"$v\"" src-tauri/tauri.conf.json ui/package.json
          grep -q "^version = \"$v\"" src-tauri/Cargo.toml
      - run: test ! -f package.json          # no root npm junk
      - run: '! git ls-files | grep -E "^(ui/dist|target|src-tauri/gen)/"'
```

Also in `release.yml`: pin action SHAs, add the `cli` job (P2-5), read `releaseBody` from `CHANGELOG.md`, and gate the release job on the `CI` workflow passing for the tagged commit (`workflow_run` or `needs`).

---

## 5. What a user gets today

- **Nobody outside can download anything.** Repo is private; releases are drafts. "Friend build" = Lucas downloads `Parzi_<v>_x64-setup.exe` from the draft and sends the file by hand (PROGRESS §577, §594).
- **Install experience:** unsigned Authenticode → SmartScreen "More info → Run anyway"; NSIS per-user into `%LOCALAPPDATA%\Parzi` (no admin, verified silent install/uninstall in PROGRESS); the MSI needs admin and is per-machine.
- **After install:** app loads (v0.1.4 fix verified by CDP); wallpaper only if seeding works on a clean profile (P2-10, unverified); updater silently does nothing forever (P1-2); v0.1.7 specifically ships a dead Report-issue button and no provider logos (P1-1).
- **Nothing else:** no macOS/Linux, no changelog, no crash reporting, no log file, no telemetry (fine), CLI never published.
- **Last good artefact:** `v0.1.6` (CI success 18:03). `v0.1.7` is building from a broken HEAD.

---

## 6. Verified good

- `.gitignore` really ignores `target/`, `src-tauri/target/`, `node_modules/`, `ui/dist/`, `*.key`, `*.key.pub`, `*.sig`, `ui/vite-dev*.log`, `.env*` (`git check-ignore -v` on each); none of them are tracked.
- Index line endings are uniformly LF (`git ls-files --eol`: 0 `i/crlf`, 0 `i/mixed`); binaries are `-text`. The CRLF noise is only the global `autocrlf` warning.
- Shared dependency versions are identical in both lockfiles: tokio 1.53.1, serde_json 1.0.151, keyring 4.2.0, uuid 1.26.0, serde 1.0.229.
- No secrets in the tree; only the minisign **public** key is in `tauri.conf.json`; the private key is a CI secret and `*.key` is ignored.
- `custom-protocol` feature is present in `src-tauri/Cargo.toml` (the v0.1.4 blank-window fix); the nav-race hunk is gone from the worktree `main.rs`.
- README quick-start commands all exist with the documented flags; bin name is `parzi`.
- Attribution comments exist at the point of use: `antigravity.rs:3,9`, `antigravity_oauth.rs:3`, `catalog.rs:3,322,503`, `handler.rs:758`, `providerMarks.ts:2`.
- `unsafe_code = "forbid"` at workspace level; release profile `opt-level="z"`, `lto`, `strip`, `codegen-units=1` match PLAN §11.
- Provider-logos lane left no dangling references (`grep assets/providers ui/src` → 0; no leftover `logos` props).
- `install_skill_from_git` refuses `git@`/`ssh://`, sets `GIT_TERMINAL_PROMPT=0`, clones `--depth 1 --single-branch`, cleans its temp dir on every path.
- `open_external_url` does at least allowlist by prefix (the bypass is P1-5, but the intent is right).
- Capabilities file (`src-tauri/capabilities/default.json`) is minimal: window chrome, dialog, updater, events.
- All 8 tags are pushed; 7 of 8 release runs green (v0.1.2 failed on config schema and was fixed in v0.1.3).
- `rmcp` is absent from both lockfiles — consistent with the documented P5 deviation.
