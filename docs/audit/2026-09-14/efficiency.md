# Parzi — streamlining and resource review (2026-09-14, second pass)

Done by the orchestrator directly, no subagents. Measurements were taken on the
installed v0.1.8 build that was running on this machine at the time (started
22:09), window maximized at 1295×767 logical on a 150 % display (≈1942×1150
physical pixels), AMD Radeon 780M. Code read from the working tree at ~22:25.
Nothing was changed.

Grade: the RAM, CPU, size and dependency numbers are measured. The causes
assigned to them are read from the code and are marked "likely" where I could
not toggle the effect and re-measure. The streaming-path costs are read from the
code only; no run was started, because that would spend your quota and touch
your store.

---

## 1. Measured footprint

### 1.1 GUI at idle, app in the foreground (sample 1, 6 s)

| Process | Private MB | Working set MB | CPU, % of one core |
|---|---|---|---|
| parzi-app.exe (Rust host) | 16.7 | 44 | 1.8 |
| WebView2 browser host | 36 | 141 | 2.1 |
| WebView2 GPU process | **364** | 303–529 | **6.5** |
| WebView2 renderer | 91 | 150 | 4.9 |
| WebView2 network utility | 12 | 41 | 0 |
| WebView2 storage utility | 8 | 23 | 0 |
| crashpad | 3 | 16 | 0 |
| **Total (7 processes)** | **≈ 507** | **≈ 850** | **≈ 15** |

GPU allocations of the GPU process (performance counters): 119 MB dedicated + 118 MB shared.

### 1.2 GUI at idle, app in the background (sample 2, 10 s, Discord in front)

| Process | Private MB | CPU |
|---|---|---|
| parzi-app.exe | 17 | 0.2 |
| browser host | 36 | 0.5 |
| GPU process | 266 | 0.0 |
| renderer | 70 | 0.0 |
| **Total** | **≈ 410** | **0.8** |

So the app is well behaved when hidden. The 15 % of a core and the extra ~100 MB
are the cost of being on screen: Chromium keeps compositing while the composer
has focus (caret blink) and while anything inside a blurred surface changes.

### 1.3 Against the PLAN §11 budgets

| Budget | Plan | Measured | Verdict |
|---|---|---|---|
| GUI idle | < 150 MB | ≈ 410–510 MB private, ≈ 850 MB working set | 3× over. The Rust side is 17 MB; the budget is spent in WebView2, mostly the GPU process. |
| CLI idle | < 25 MB | `parzi list` peak 9.5 MB, `--help` 4 MB | Within budget (binary from Sep 11). |
| Cold start | < 1.5 s | not measured | Unknown. Nobody has recorded it. |
| Token emit p50 | < 100 ms | not measured | Unknown, and the render path is quadratic (§3.4). |
| 10k-line thread | scrollable | no virtualization exists | Not met by construction. |

None of these numbers appear anywhere in PROGRESS.md. §6 gives a script that produces them in 15 seconds.

### 1.4 Sizes

| Artifact | Size | Note |
|---|---|---|
| `parzi-app.exe` (installed 0.1.8) | 13.9 MB | Down from 22.7 MB once the S-8 release profile was applied this evening. |
| `parzi.exe` CLI | 5.1 MB | Links `image` (png/jpeg/webp decoders), `reqwest`, `rustls`, `ring`, `keyring`, `regex`, `chrono`. The CLI decodes no images. |
| `ui/dist` | 3.0 MB | JS 1.58 MB, CSS 154 KB, **78 font files = 1.3 MB**, icons. |
| Main JS chunk | 1.58 MB | `highlight.js` full build (386 languages, 2.2 MB of source) is the bulk; markdown-it ≈ 120 KB, DOMPurify ≈ 130 KB, Svelte app the rest. The plan said "12 langs". |
| Fonts shipped | 78 files | Inter 4 weights × 4 subsets × 2 formats, JetBrains Mono 2 × 3 × 2, Instrument Serif. Only latin is ever displayed; woff is dead weight next to woff2. One `font-weight: 550` in the CSS has no face at all. |
| `~/.parzi` | 5.4 MB | 4.9 MB of it is ten wallpapers; two are 1.6 MB and 2.2 MB JPEGs that decode to 30–40 MB textures each. |
| Dependencies | 405 crates (GUI), 221 (CLI) | Duplicates: `reqwest` 0.12 + 0.13 (providers vs updater), `png` ×2, `toml` ×2, `thiserror` ×2, `base64` ×2, `dirs` ×2, `windows-sys` ×4, `syn` ×2. |
| Threads in parzi-app | 26–27 | Tauri's default tokio runtime = one worker per hardware thread, plus blocking pool, plus WebView2 host threads. |

---

## 2. Where the RAM goes

All of it is in the WebView2 GPU process and renderer, and it is a direct consequence of the visual stack. Read from `App.svelte:1243-1275`, `theme.css:246, 415`, `Sidebar.svelte:503`, `Omnibar.svelte:918`, `RightPanel.svelte:122`, `SettingsNav.svelte:77`, `App.svelte:1391` and your live `theme.toml` (wallpaper blur 0, dim 0.8, vignette 0.6, glass blur 16 px).

At ≈1942×1150 physical pixels one RGBA surface is 8.9 MB. The stack allocates, per frame it needs to repaint:

1. **A transparent, frameless window** (`tauri.conf.json:27-28`). On Windows this means a per-pixel-alpha composited surface through DirectComposition for the whole window. It exists only to draw rounded corners. Likely cost: one extra full-window surface and a slower present path. Not measured in isolation.
2. **The wallpaper `<img>` with `filter: blur(var(--parzi-bg-blur)) saturate(1.05)` and `transform: scale(1.04)`** (`App.svelte:1251-1257`). A CSS filter on a full-viewport element forces an offscreen render target for the source, the filtered result, and blur intermediates. With your blur at 0 the blur is cheap but `saturate()` still runs the offscreen pass, and the scale transform keeps it a separate layer. The wallpaper itself is decoded at the file's full resolution: nothing in the tree downscales it (`theme.rs:673-679` only thumbnails to 64×64 for the palette; the plan's "cached 1080p texture" was never built). Your current wallpaper is 2038×1147 (9.3 MB as a texture); the turtle one is 4K-class (≈35 MB).
3. **Three full-viewport overlay divs** (dim, vignette, glow), each `position:absolute; inset:0` with a gradient background. Whether they merge into one layer depends on the compositor; each that does not is another 8.9 MB.
4. **Six `backdrop-filter` regions**: sidebar 14 px, composer 16 px, right panel 18 px, settings nav 14 px, `.glass` 16 px, popovers 12 px, plus a 6 px focal dim in App. Every backdrop-filter region needs a copy of what is behind it at physical resolution plus blur intermediates, and it is re-evaluated whenever anything behind it or inside it repaints. The sidebar alone is ≈ 248×1150 logical → ≈ 370×1725 physical ≈ 2.5 MB per copy, times the blur passes. PROGRESS records that this exact stack "froze dragging" on 2026-09-09, was removed ("blur kept only on composer + small popovers"), and was then brought back the same day ("Sidebar and composer are glass again"). The measurement says the lag-kill entry was right.

Where the numbers land: 119 MB of dedicated GPU memory and 118 MB shared is consistent with roughly 25 full-resolution surfaces plus the wallpaper texture and Skia's caches. The renderer's 91 MB is the DOM, the 1.58 MB script heap, and the tile rasters for those layers.

## 3. Where the CPU goes

### 3.1 At idle, foreground (≈ 15 % of one core)

Likely causes, in order of confidence. I could not toggle them individually on the running app.

- The composer textarea is focused on the home screen (`Omnibar.svelte:389, 659`) so the caret blinks about twice a second. Each blink repaints the composer, and the composer has a 16 px backdrop-filter, so each blink re-blurs that region. Cheap alone, but it keeps the compositor awake at full rate.
- The wallpaper filter layer and the overlay layers are re-composited on every frame the compositor produces.
- Hover transitions (`Omnibar` 11, `App` 7, `Thread` 6, `Titlebar` 4) run whenever the pointer crosses the window.
- Infinite animations exist but only appear with state: `deck-ind.live` when agents are live (`Titlebar.svelte:129-130`), `.streaming-dot`/`caretflow` while streaming, `.tool-pill.run` while a tool runs, the AgentVisualizer ring and dashed edges whenever the inspector's Agents tab is open, the skeleton shimmer while loading. None of them ran during the samples; all of them will run during a real session, on top of the blur cost.

The 10 s background sample at 0.8 % proves the app is not polling: there is no timer in the UI except a 30 s clock tick per visible thread row (`ThreadRow.svelte:50`) and the one-shot update check.

### 3.2 In the Rust host at idle (1.8 % foreground, 0.2 % background)

Fine. The 26 threads are the tokio default (one worker per hardware thread) and the WebView2 host. Nothing spins.

### 3.3 Per model turn (read from `handler.rs:284-330, 424-441`, `tools.rs:101-122`)

Every turn, before the request goes out:

1. `assemble()` re-reads and re-parses the whole `events.jsonl` from disk (`store.events`), then re-runs compaction if the session has more than 96 events, then clones every system part. Sessions on this machine are 13 events at most, so today this is microseconds; it is O(events) per turn and O(events²) per run by design.
2. `tool_defs()` → `defs_with_mcp()` iterates every configured MCP server, calls `exposed_tools` (which spawns the server if it is not live) with a 12 s timeout, and rebuilds the tool list. The comment says "call once per run start"; the code calls it every turn. With one unreachable server that is 12 s of dead time and a leaked half-started process per turn (runtime report R-2). You have no MCP servers configured right now, so you have not felt it.
3. A new `reqwest::Client` is built per request in five adapters (`anthropic.rs:73`, `antigravity.rs:378`, `codex.rs:201`, `openai_compat.rs:30`, `opencode.rs:152`). Each client owns a connection pool and a rustls config; none of the pools is ever reused, so every turn pays a fresh TCP + TLS handshake to the provider.

### 3.4 Per token (read from `handler.rs:342`, `main.rs:203-229`, `App.svelte:742`, `Thread.svelte:294-296`, `md.ts`)

This is the path the §11 budget is about, and it is quadratic in the length of the answer:

1. Provider chunk → `RunEvent::Text` → one `app.emit("parzi://run-event")` per delta: JSON serialize, IPC hop, JS event dispatch. No coalescing on either side.
2. `App.svelte:742` does `live += e.text`, a reactive assignment.
3. `Thread.svelte:294-296` re-runs `splitSegments(liveText)` on the whole buffer and `renderMarkdown(seg.body)` on every segment: a full markdown-it parse, DOMPurify sanitize, and `highlight.js` on every fence that exists so far, for every token that arrives. A 4 000-token answer with three code blocks is 4 000 × (parse + sanitize + highlight of everything before it).
4. `chronologicalItems` is rebuilt and the `{#each}` is keyed by index (`Thread.svelte:198`), so finished messages are re-rendered too.

There is no 60 ms debounce, no per-message HTML cache and no virtualization (all three are in PLAN §8 as shipped). The UI report already lists this as U-4; here it is the single biggest CPU item during actual use.

### 3.5 Per event append (read from `store.rs:247-262, 384-462`)

`append()` does five file operations for one event: open+write the line, read `meta.json`, write `meta.json` (tmp + rename), then `render_md()` reads `meta.json` again, reads and parses the entire `events.jsonl`, rebuilds the whole `session.md` string and rewrites it. `session.md` is therefore O(n²) per session, and every `Usage` event adds another read-modify-write of `meta.json` (`handler.rs:355`). On Windows every one of those writes can trigger a Defender real-time scan. At 13 events per session it is invisible; at a 10k-line thread it is the disk bottleneck.

### 3.6 Per UI event (read from `App.svelte`, `store.rs:185-200`)

`loadThreads()` is called from ten places, including on `done`, `error`, `subsession_created` and after every action. Each call is one IPC and `store.list()`, which `read_dir`s the sessions folder and reads and parses every `meta.json`. 27 sessions today; it becomes 1 000 file reads per event at 1 000 sessions. The sidebar then renders every thread (no cap) and computes `depthOf` per row (O(n²) over the list).

### 3.7 Smaller things

- `palette_from_background` (`main.rs:903`) decodes the full wallpaper synchronously inside an async command, blocking a tokio worker for the duration of a 4K JPEG decode. `create_subsession` and `install_skill_from_git` correctly use `spawn_blocking`; this one does not.
- `await_settled` for `session.spawn(wait=true)` polls `meta.json` from disk four times a second for up to 180 s instead of waiting on the orchestrator's `Notify`.
- `run_doctor` builds a second `McpManager`, so a doctor run spawns every MCP server a second time next to the orchestrator's copies.
- MCP servers are only reaped when the next MCP call happens (`mcp.rs:198-210`), never on a timer, and never on app exit. Each `npx` server is a Node process of 50–100 MB. Over a day of use the idle footprint grows by whatever servers were touched.

---

## 4. Streamlining: what is carrying no weight

Line counts: Rust 15 114 (crates + shell), UI 11 636 (Svelte + TS + CSS), 31 Svelte components.

| Item | Weight | Why it can go or shrink |
|---|---|---|
| `highlight.js` full build | ≈ 1.0–1.2 MB of the 1.58 MB chunk | Register `lib/core` + the 12 languages the plan names. Also the biggest single cold-start cost: the renderer parses this file on every launch. |
| 78 font files | 1.3 MB of dist, 64 of the files never load | Latin + latin-ext only, woff2 only, the four Inter weights actually used, two Mono weights, one Serif. ≈ 14 files, ≈ 250 KB. Delete `font-weight: 550`. |
| Default wallpaper as a 609 KB PNG | installer + every user's home | Ship ≤ 1920 px WebP (≈ 150–250 KB) once B6 replaces the image; downscale user uploads at `set_background` time to the largest display, which is the "cached 1080p texture" PLAN §9 promised. |
| Two Cargo workspaces (`src-tauri` detached) | two lockfiles, two profiles, two `cargo test` runs, drift on every bump | The stated reason ("GUI deps never leak into the CLI") is already guaranteed by crate boundaries; workspace members do not inherit each other's dependencies. One workspace, one lockfile, one `[profile.release]`, one `[lints]`. |
| `reqwest` 0.12 in providers next to 0.13 in the updater | two HTTP client crates compiled into the GUI | Bump providers to 0.13. `hyper`/`rustls`/`ring` are already shared, so the saving is the reqwest layer only, but the drift is free to remove. |
| `image` with png/jpeg/webp in `parzi-core` | ≈ 1 MB of decoders in the CLI, which never decodes | Behind a `palette` feature enabled only by the shell; or move `extract_palette` into the shell crate. |
| 71 IPC commands | each is a Rust handler + serde types + a TS wrapper + a TS type | Seven are dead (`get_provider_health`, `reset_circuit_breaker`, `background_file`, `list_backgrounds`, `upload_background`, `migrate_tasks`, `list_packs`), four duplicate granted window capabilities, two doctor variants and two MCP tool-list variants can merge, and the shell-side logic (subsession creation, ancestor walk, base64, file walkers, TOML writer) belongs in crates. Target ≈ 30. |
| Seven SSE parse loops | ≈ 400 lines, seven copies of the same bugs (P-2, P-4, P-5) | One byte-buffered line reader in `types.rs`; adapters keep only their event mapping. |
| Three provider rosters in the UI (`Omnibar:95`, `ModelsSection:21`, `providerMarks:82`) + `PROVIDERS` in Rust | four places to update per provider | Derive the UI roster from `get_models` rows. |
| `providerMarks.ts`: 13 marks for 5 providers | 8 dead SVGs, and the trademark exposure noted in the repo report | Keep five. |
| `App.svelte` 1 339 lines, `Omnibar.svelte` 1 128, `ConnectorsSection.svelte` 975 | 18 files over the plan's 400-line rule | `runStore` (pure `onEvent` reducer), `threadStore`, `paletteStore`; move the 15 MCP presets (≈ 300 lines of data) to a JSON file loaded on demand. |
| Unwired features | code that ships and does nothing | Skills page (357 lines UI + ≈ 470 lines of install code) consumed by nothing; `mcp-pack` plugin kind accepted and never loaded; "Build" mode identical to "Chat"; `migrate_tasks` shim for a concept removed on 09-12; "Scheduled Tasks" sidebar row with no implementation; `t3` remnants in NOTICES, marks and examples; `CliApprover.yes` always false. Wire or delete each; none should sit half-built. |
| `session.md` rewritten per event | O(n²) disk traffic | It is "the rendered view for humans and other harnesses". Render it on demand (`parzi show`, `export`, `get_thread`) or append the fragment for the new event; never rebuild. |
| Tokio runtime shape | 26 threads in the GUI, multi-thread runtime in the CLI | `tauri::async_runtime::set` with 2–4 workers; `#[tokio::main(flavor = "current_thread")]` in the CLI. A few MB and a dozen threads, mostly hygiene. |

Rough total if all of it is done: dist 3.0 MB → ≈ 1.2 MB, main chunk 1.58 MB → ≈ 0.5 MB, CLI binary ≈ −1 MB, and on the order of 3 000–4 000 lines removed from the tree.

---

## 5. The moves, in order of payoff per hour

Sizes on the same scale as `AUDIT.md`. "Verify" is what to measure after; §6 has the script.

| # | Move | Expected effect | Size | Verify |
|---|---|---|---|---|
| E1 | **Compositor diet.** Wallpaper `<img>` gets its `filter`/`transform` only when blur > 0 (a class, not an always-on filter); merge dim + vignette + glow into one element with three background layers; `backdrop-filter` only on the composer and popovers, solid translucent fills for sidebar, right panel and settings nav (what the 09-09 lag-kill entry did before it was reverted); no `box-shadow` on scrolling rows; run infinite animations only while their state is true (already so) and gate them on `prefers-reduced-motion` (already so). | GPU process −150 to −250 MB private; foreground idle CPU from ≈ 15 % toward ≈ 2 %; smoother drag and scroll. | S–M | GPU process private MB and the 10 s CPU sample, foreground, home screen, before and after. |
| E2 | **Wallpaper texture cap.** At `set_background` and at first-run seeding, decode once and downscale to the largest attached display's physical size, save as WebP; refuse > 4K sources instead of decoding them. Ship the default at ≤ 1920 px. | −10 to −40 MB in GPU/renderer per large wallpaper; installer −400 KB; `palette_from_background` becomes fast. | S | Renderer + GPU private MB with the turtle wallpaper selected, before and after. |
| E3 | **Bundle diet.** `highlight.js/lib/core` + 12 languages; latin/latin-ext woff2 only for the weights used; drop `font-weight: 550`. | JS 1.58 → ≈ 0.5 MB, dist 3.0 → ≈ 1.2 MB; faster cold start (measure it while you are there). | S | `npm run build` output; cold-start stopwatch from process start to first paint. |
| E4 | **Streaming path.** Coalesce `Text` deltas in the Rust forwarder every 30–50 ms into one emit; in the UI flush the live buffer on a 60 ms timer, render only the last segment incrementally, cache HTML per finished message, key `{#each}` by id, virtualize past 500 blocks. | Streaming CPU flat instead of quadratic; the 10k-line budget becomes reachable; token-emit p50 measurable. | M | Renderer CPU during a 4 000-token answer with three fences; scroll a 10k-line transcript. |
| E5 | **Store I/O.** Keep the run's events in memory (append-through) so `assemble()` does not re-read the file; render `session.md` on demand; coalesce `meta.json` writes to turn boundaries; unique tmp names + fsync (core report C-5). | Per-event cost O(1); 5 file operations per event → 1; no O(n²) rewrite. | M | Count file writes per turn with Process Monitor or `strace`-style logging; a 10k-event append benchmark. |
| E6 | **Per-run caches.** `defs_with_mcp` once per run (invalidate on `apply_config`); one `reqwest::Client` per provider instance (connection reuse). | Removes up to 12 s per turn per unreachable server and a TLS handshake per turn; fixes the per-turn process leak. | S | Time-to-first-token on a second turn; process count after ten turns with one bad MCP server configured. |
| E7 | **Sidebar refresh.** Debounce `loadThreads()` to one call per 100 ms; later an in-memory index in the orchestrator pushed as deltas. Cap rendered rows (the plan's "More"). | O(sessions) file reads per event → one per burst. | S | `list_threads` call count during a run (add a counter in dev). |
| E8 | **Process hygiene** (runtime report R-2, P2-4). `kill_on_drop`, an idle reaper on a timer, `McpManager::shutdown()` on exit, one manager shared with doctor. | Idle footprint stops growing over a day; no orphan `node` processes after quit. | M | Process list after quit; after a doctor run. |
| E9 | **Runtime shape.** 2–4 tokio workers in the GUI, `current_thread` in the CLI; `palette_from_background` under `spawn_blocking`; `await_settled` on the `Notify`. | Threads 26 → ≈ 12; a few MB; no blocked worker during decode; no 4 Hz disk polling. | S | Thread count; CPU during a `session.spawn(wait=true)`. |
| E10 | **Dependency and workspace hygiene.** One workspace; `reqwest` 0.13 everywhere; `image` behind a feature; delete the seven dead commands and the window quartet. | One lockfile, one profile, CLI −1 MB, fewer duplicate crates, faster clean builds. | S–M | `cargo tree -d` duplicate list; CLI binary size. |
| E11 | **Architecture cuts** (§4). One SSE reader, one roster, stores out of `App.svelte`, presets as data, wire-or-delete the unwired features. | ≈ 3–4k lines gone; the 400-line rule becomes enforceable. | M–L | `wc -l` gate in CI. |
| E12 | **Transparent-window experiment.** Build once with `transparent: false` and Windows 11 native rounded corners (`DWMWA_WINDOW_CORNER_PREFERENCE`), measure. Keep whichever wins. | Unknown; possibly a full-window surface and a faster present path. Cheap to find out. | S | Same GPU/CPU sample. |

Not recommended: `--disable-gpu` or software compositing (moves the cost to the CPU and makes blur far worse); stripping the design (the identity is the product); an in-process GPU flag (unsupported in WebView2 and unstable).

E1–E3 are an evening. They do not touch the approval gate or the sandbox, so they can run as a light lane beside the security work in `AUDIT.md` Phase 1. E4 and E5 belong with the UI and core lanes in Phases 2–3.

---

## 6. Measure it every release

Fifteen seconds, PowerShell, with the app open on the home screen and in the foreground. Paste the output into PROGRESS with each tag; the §11 budgets are meaningless until this exists.

```powershell
$ids = @((Get-Process -Name parzi-app).Id) + @((Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'msedgewebview2.exe' -and $_.CommandLine -match 'parzi' }).ProcessId)
$t0 = @{}; foreach ($i in $ids) { $p = Get-Process -Id $i; $t0[$i] = $p.TotalProcessorTime.TotalMilliseconds }
Start-Sleep -Seconds 10
$priv = 0; $cpu = 0
foreach ($i in $ids) { $p = Get-Process -Id $i; $priv += $p.PrivateMemorySize64; $cpu += $p.TotalProcessorTime.TotalMilliseconds - $t0[$i]
  $cl = (Get-CimInstance Win32_Process -Filter "ProcessId=$i").CommandLine; $t = if ($cl -match '--type=(\S+)') { $Matches[1] } else { $p.ProcessName }
  "{0,-16} cpu {1,5:N1}%  private {2,6:N0} MB" -f $t, ($p.TotalProcessorTime.TotalMilliseconds - $t0[$i])/100, ($p.PrivateMemorySize64/1MB) }
"TOTAL private {0:N0} MB   idle cpu {1:N1}% of one core   processes {2}" -f ($priv/1MB), ($cpu/100), $ids.Count
```

Budget proposal to replace PLAN §11's "GUI idle < 150 MB", which WebView2 cannot meet: **Rust host < 40 MB private, WebView2 total < 250 MB private, foreground idle CPU < 3 % of one core, background idle < 1 %, dist < 1.5 MB, cold start < 1.5 s.** Today: 17 / 490 / 15 / 0.8 / 3.0 / unmeasured.

---

## 7. What is already efficient (leave it)

- The Rust host: 17 MB private, no polling, no timers, event-driven end to end.
- The CLI: 9.5 MB peak, 53 ms for `list`. Within budget even with the unused decoders linked in.
- The event log: append-only JSONL, single line per event, tiny on disk (30 KB for 27 sessions).
- The catalog cache and the `catalog_refresh` kill switch: no network on the default path; `get_models` probes providers in parallel with a 10 s cap.
- MCP servers are lazy: nothing is spawned until a tool list is needed.
- `prefers-reduced-motion` already disables every infinite animation (`theme.css:160-166`).
- The UI never polls: the only interval is a 30 s clock per visible row, and the update check is a one-shot.
- Icons are inline SVG paths; no icon font, no image sprites.
- No telemetry, no analytics, no background network.
