# Parzi native UI on egui — framework plan (2026-09-15)

Goal: replace `ui/` (Svelte in WebView2) and `src-tauri/` with one Rust binary on
egui/eframe that reads better, starts faster, idles at zero CPU and well under
100 MB, and deletes the IPC and HTML attack surface. The crates (`core`,
`providers`, `runtime`, `cli`) stay as they are; the fix plan in `AUDIT.md`
still runs first, because a new renderer does not fix a broken approval gate.

Research date: 2026-09-15, egui/eframe 0.36.2 (2026-09-08). Sources at the end.

---

## 0. What the research changed

My earlier answers described egui as it was in 2025. Three of those claims are no longer true:

| Earlier claim | State in egui 0.36 (2026) |
|---|---|
| Text is unhinted and soft | 0.34 (March 2026) replaced `ab_glyph` with `skrifa` + `vello_cpu`; **hinting is on by default** (`TextOptions::font_hinting = true`), with per-font override in `FontTweak`. |
| No ligatures, basic kerning | 0.35 (June 2026) shapes text with **`harfrust`**: real kerning and ligatures. **Subpixel binning** (up to four horizontal offsets per glyph) is on by default. |
| Emoji are monochrome | Colour emoji from system fonts (Segoe UI Emoji on Windows) merged 2026-09-06 behind `color_fonts` + `egui_system_fonts`; lands in **0.37**. You said you do not need them; the bundled monochrome fonts become opt-in, saving 725 KB. |

Also confirmed:

- Variable fonts: `TextFormat.coords` and `FontTweak.coords` set variation axes, so one Inter variable file gives every weight.
- Rich text: `LayoutJob` sections carry font, colour, background, italics, underline, strikethrough, vertical align, **`line_height`**, letter spacing and variation coords. `extra_text_line_spacing` exists globally (0.36).
- Virtualization: `ScrollArea::show_viewport` (paint only the visible rect) and `show_rows` (uniform rows); **`stick_to_bottom`** for streaming.
- Frameless: `ViewportBuilder::with_decorations(false).with_transparent(true)`; `ViewportCommand::StartDrag`, `BeginResize`, `Maximized`, `Minimized`, `Close`. The `custom_window_frame` example is the template. Windows caveat: transparency needs the graphics context's support; use `App::clear_color` for the transparent clear.
- Text selection: per-label selection with Ctrl+C works; **selection across labels is still an open follow-up**. Copy-whole-message buttons cover it.
- IME: composition visuals overhauled in 0.35; Windows IME works through winit.
- Accessibility: `accesskit` is a default eframe feature.
- Custom GPU work: `egui_wgpu::CallbackTrait` draws into egui's own render pass; it **cannot read the backbuffer**, so live backdrop blur of dynamic content is not available through it. Static-backdrop glass (blur the wallpaper once) is the design.
- Idle: egui repaints only on input or animation; a small app measures ≈ 30 MB RSS (issue #3689). Typical frame 1–2 ms.
- Packaging: `cargo-packager` does NSIS (`installMode: currentUser`, `headerImage`, `sidebarImage`, custom `template`, `preinstallSection`) and WiX; `cargo-packager-updater` verifies minisign signatures (`minisign-verify`), keys via `cargo packager signer generate`, endpoint JSON `{version, url|platforms, signature, format}`. Same scheme family as Tauri's updater; **whether this week's Tauri key verifies unchanged is a one-line test, not an assumption.**

What stays a real gap: live blur under popovers; cross-message text selection; the RAM number is a target until measured.

---

## 1. Decision and scope

- New workspace crate **`crates/parzi-desk`** (binary `parzi-desk`, later merged into `parzi` behind a `--gui` flag). Lives in the one workspace, so `src-tauri`'s detached workspace, second lockfile and second profile disappear with it.
- **No IPC.** The UI thread owns an `Arc<Orchestrator>` and calls it. Run events arrive on the existing `RunEvent` channel; approvals are an in-process `Approver` backed by a channel to the UI. The 71 commands, `api.ts`, `UiEvent`, CSP, DOMPurify and the nine HTML sinks stop existing.
- **Renderer:** eframe with the `wgpu` backend (default). `glow` is the fallback if a machine has no working D3D12/Vulkan.
- **Markdown:** our own renderer from `pulldown-cmark` events to `LayoutJob`s. Not `egui_commonmark` (fine, but not styleable to this design). `syntect` for code, themed from `theme.toml`.
- **Glass:** wallpaper pre-blurred once at set time; panels are translucent rounded frames. No per-frame blur.
- **Feature parity target:** sidebar, stage, composer, thread, approvals, model and slash menus, Settings (General, Appearance, Models, Connectors), inspector (Agents, Docs). Skills, Context section and theme packs only if they survive the AUDIT §6 decisions.

Not in scope: `user.css` (dies with the webview; theme.toml colours carry over), web build, macOS/Linux polish (eframe runs there, but we ship Windows).

---

## 2. Architecture

```
main thread (winit + eframe)                background thread
┌───────────────────────────────┐            ┌──────────────────────────┐
│ App { state, caches, theme }  │  commands  │ tokio runtime            │
│  ├ sidebar    ├ composer      │ ─────────▶ │  Arc<Orchestrator>       │
│  ├ transcript ├ approvals     │            │  providers, MCP, store   │
│  ├ settings   └ inspector     │ ◀───────── │  RunEvent per session    │
│ ctx.request_repaint() on msg  │  events    │  UiApprover -> channel   │
└───────────────────────────────┘            └──────────────────────────┘
```

- **Threads.** eframe owns the main thread. One `tokio::Runtime` (2–4 workers) on a background thread hosts the orchestrator exactly as `src-tauri` does today. Every `RunEvent` is forwarded to a `std::sync::mpsc` (or `crossbeam`) receiver polled at the top of each frame; the forwarder calls `ctx.request_repaint()` after each batch, which is documented as safe from any thread on eframe. Token deltas are coalesced in the forwarder every ~30 ms before the wake-up.
- **State.** `AppState` mirrors today's `App.svelte` state but typed: `threads: Vec<SessionMeta>`, `active: Option<SessionId>`, `live: HashMap<SessionId, LiveRun>`, `approvals: VecDeque<PendingApproval>` keyed by session, `composer: ComposerState`, `settings: SettingsState`. No global stores; one struct, passed down.
- **Transcript cache (the scrollback).** Per session: `Vec<Block>` where a block is one event rendered to a `LayoutJob` plus its laid-out height at the current column width, plus a prefix-sum of heights. Only the tail block is mutable while streaming. Width change (window resize) invalidates heights lazily, visible blocks first. This is what makes a 100k-line transcript cost nothing to scroll.
- **Approver.** `UiApprover` implements the runtime's `Approver` trait: it pushes `(key, ToolCallInfo, oneshot::Sender)` into the approvals queue and awaits the oneshot with the same 120 s deny timeout. One path, no second channel (this is AUDIT B1 done right).
- **Modules** (each under 400 lines, enforced in CI): `main.rs`, `app.rs` (frame loop), `state.rs`, `bridge.rs` (runtime thread, channels, approver), `theme.rs` (theme.toml → `Visuals`, fonts), `wallpaper.rs`, `chrome/{titlebar,sidebar,panels}.rs`, `transcript/{cache,markdown,code,blocks}.rs`, `composer/{editor,menus}.rs`, `approvals.rs`, `settings/{general,appearance,models,connectors}.rs`, `inspector/{agents,docs}.rs`, `widgets/{widget,diagram,artifact}.rs`.

---

## 3. Crates

| Crate | Version | Features | Why |
|---|---|---|---|
| `eframe` | 0.36 | default (`wgpu`, `accesskit`, `default_fonts`), `persistence` | window, event loop, renderer, a11y, window-state persistence |
| `egui` | 0.36 | — | UI |
| `egui_extras` | 0.36 | `image`, `svg`, `syntect` | wallpaper/thumbnail loading, SVG marks, code highlighting helpers |
| `epaint` | 0.36 | (via egui) | `LayoutJob`, `TextFormat`, `FontTweak`, `TextOptions` |
| `pulldown-cmark` | 0.13 | — | markdown events |
| `syntect` | 5 | `default-fancy` (pure Rust regex, no onig) | highlighting; theme built from `theme.toml` tokens |
| `image` | 0.25 | `png,jpeg,webp` | wallpaper decode, downscale, blur (already in core) |
| `tokio` | 1 | `rt-multi-thread,sync,time` | the runtime thread |
| `arboard` | 3 | — | clipboard (eframe has basic copy; arboard for images/rich text later) |
| `cargo-packager` (dev tool) | latest | — | NSIS/WiX |
| `cargo-packager-updater` | latest | — | in-app updates, minisign |
| `parzi-core`, `parzi-providers`, `parzi-runtime` | path | — | unchanged |

Not used: `egui_commonmark` (style control), `egui_system_fonts` (bundle our own three fonts; revisit for emoji in 0.37), `winit` directly (eframe wraps it), `taffy` (hand layout is enough for three panels).

---

## 4. Reading spec (the part that decides quality)

Fonts, bundled with `include_bytes!`:

- **Inter** variable (`Inter[opsz,wght].ttf`) as `FontFamily::Proportional`. Weights via `TextFormat.coords`: body 400, labels 500, headings 600. Inter's optical-size axis at 13–15 px improves small text; set `opsz` to the pixel size.
- **JetBrains Mono** 400 as `FontFamily::Monospace`, ligatures on (harfrust does them; provide a toggle in Appearance since many prefer them off).
- **Instrument Serif** italic as a named family for the wordmark and the hero line only.
- `FontTweak`: `hinting: Some(true)`, `subpixel_binning: Some(true)`, `y_offset_factor` tuned per family so inline code sits on the prose baseline (Inter and JetBrains Mono differ by about 0.05 em).
- Rasterization happens at the OS scale automatically; on your 150 % display glyphs are rendered at 1.5× and hinted.

Type scale (points at 1×): body 15 / line-height 23 (`TextFormat.line_height = Some(23.0)`), small 13 / 19, code 13 / 20, heading-2 18 / 26 (600), heading-1 21 / 28 (600), hero 32 serif italic. Paragraph spacing half a line. Column: 700 px measure, centred in the stage; wider windows add margin, not measure.

Colour: `theme.toml` maps onto `egui::Visuals` (`panel_fill`, `window_fill`, `widget` visuals, `selection`, `hyperlink_color`) plus our own token struct for prose, dim text (60 % alpha), code background, blockquote rule, link, diff add/remove, tool-card states. Never pure white on pure black; today's `#E6E8EF` on `#0B0B10` family stays.

Markdown to blocks (each becomes one `LayoutJob` or one custom block):

- paragraph → job with inline runs (bold via coords 600, italic via `italics`, inline code in mono on a tinted `background` with `expand_bg`, links underlined and coloured, hover shows the URL)
- heading → job with heading size/weight and extra space above
- list → hanging indent, bullet or number drawn as a separate small job; nested lists indent 20 px
- blockquote → 2 px left rule, dimmed text, 12 px inset
- code fence → custom block: frame with code background, 1 px hairline, language badge top-right, copy button, `syntect` job inside; clamp at 30 lines with an expander (same rule as today)
- table → `egui_extras::TableBuilder` or hand-drawn grid; wrap cells
- horizontal rule, images (local `parzi://` and `asset` only, via `egui_extras` loaders), task lists
- unclosed fence while streaming → treated as code until the fence closes; never flips the rest of the message into prose

Transcript behaviour: `ScrollArea::vertical().show_viewport(...)` with the prefix-sum to map the viewport rect to a block range; `stick_to_bottom(true)` only while the user is at the bottom (track it; otherwise show a "jump to latest" pill); repaint at 30 fps while streaming, zero when idle; per-message copy button; selection inside a message via `Label::selectable`.

---

## 5. Milestones, acceptance, size

Sizes in lane-days on the measured calibration (light lane ≈ 50 min of build; heavy lane ≈ 2.5 h with two reviews). Screenshot judgements are yours and are not in the sizes.

| # | Milestone | Deliverable | Accept when | Kind | Days |
|---|---|---|---|---|---|
| M0 | Skeleton | `crates/parzi-desk`: frameless transparent window, custom title bar (drag, double-click maximize, min/max/close via `ViewportCommand`), `BeginResize` on the edges, three panels (sidebar 248 px, stage, composer), fonts loaded with hinting, `theme.toml` → `Visuals`, wallpaper loaded + pre-blurred + drawn with mipmaps, window state persisted | Opens in under 300 ms cold; the 15-second script from `efficiency.md` §6 shows one process, < 80 MB private, 0 % idle; the title bar drags and snaps on Windows 11 | light | 1 |
| M1 | Transcript | markdown → `LayoutJob` renderer, code blocks with `syntect`, the scrollback cache with prefix sums, `show_viewport`, stick-to-bottom, copy buttons; renders real sessions from `~/.parzi/sessions` read-only | A side-by-side with the webview at 15 px Inter / 13 px Mono on your display, judged by you after ten minutes of reading; a synthetic 100k-line transcript scrolls at 60 fps and adds < 30 MB; unit tests for the markdown lowering (every construct above) and the prefix-sum | light | 2 |
| M2 | Live runs | `bridge.rs`: tokio thread, `Arc<Orchestrator>`, event forwarder with 30 ms coalescing, `UiApprover`; composer (multi-line edit, Enter/Shift+Enter, IME), approval card, stop, live streaming into the tail block | Ask-mode tool call shows a card and runs exactly once after Allow (the AUDIT B1 test, now against this UI); streaming a 4 000-token answer keeps the frame under 4 ms; kill mid-stream stops the tail within one frame | heavy | 2 |
| M3 | Sidebar and projects | thread list with pinned/groups/subsessions, search, new thread, rename/delete/fork, project switcher, breadcrumb | Feature parity with `Sidebar.svelte` minus the dead tree UI; `loadThreads` equivalent is a store index, not a directory scan | light | 1 |
| M4 | Menus and settings | model picker (search, Smart Auto, sections, favourites, Ctrl+1..5), effort segment, slash menu bound to real commands, `@` file chips; Settings: General, Appearance (live sliders write `theme.toml`), Models (keys via keyring, refresh), Connectors (presets, allow/deny, tool modes) | Every control writes the same TOML the CLI reads; no control without a handler (the AUDIT U-2 rule, enforced by review) | light | 3 |
| M5 | Inspector and widgets | Agents deck (runs table, focus/fork/kill), Docs reader, WidgetV1/DiagramV1/ArtifactV1 rendered natively from core's validated structs | Widget/diagram fixtures from `logic.rs` render; malformed payload shows a "bad widget" card and never panics | light | 2 |
| M6 | Packaging | `cargo-packager` NSIS per-user with the existing header/sidebar art, dark installer template ported from `hooks.nsh`, `cargo-packager-updater` with the minisign key (verified by signing and verifying one artifact with the existing key first), CI job | Silent install/uninstall on a clean user account; update check succeeds against a test feed | heavy | 1.5 |
| M7 | Cut-over | delete `ui/`, `src-tauri/`, `api.ts`; `parzi --gui`; README, PLAN §9 rewritten; THIRD_PARTY_NOTICES: fonts (OFL), egui (MIT/Apache), syntect, pulldown-cmark | CI green; the audit's H-1, H-2, H-3, H-8, S-1, S-2, S-6, U-4, U-5, U-6 are closed by deletion | light | 1 |

Total ≈ 13.5 lane-days. Chain: M0 → M1 → M2 → M7; M3, M4, M5 in parallel after M1; M6 after M0. Long pole M0+M1+M2+M7 ≈ 6 days of lanes ≈ **four to six four-hour sessions**, plus your screenshot rounds, plus one more session if M1 needs the swash escape hatch (§7). Grade: "merged with tests green and one clean-account install", not "verified by a second person".

Kill criteria: stop after M1 if the reading comparison loses; stop after M0 if the transparent frameless window misbehaves on Windows with wgpu and the opaque fallback (§7) is unacceptable to you.

---

## 6. Order relative to the audit

1. AUDIT Phase 0 (commit per lane, hermetic tests, CI) and Phase 1 lane S1 (approval gate, sandbox, handles) first. M2 depends on the fixed `Approver` contract.
2. M0 and M1 can start in parallel with Phase 1, in their own crate, touching nothing else.
3. Phase 1 lanes S2 (CSP, scoped file IPC, opener) and S5 (DOMPurify, link interception) are webview-only fixes. If M1 passes, skip them and let M7 delete the surface; if M1 fails, do them.
4. Phase 3 lane U1 (UI refactor of `App.svelte`) is cancelled by this plan either way; do not spend it.

---

## 7. Risks and their fallbacks

| Risk | Signal | Fallback |
|---|---|---|
| Transparent window + wgpu on Windows renders black corners or ghosting | M0 | `with_transparent(false)`, opaque clear colour, rounded corners via DWM (`WindowAttributesExtWindows::with_corner_preference` through `NativeOptions::window_builder`); lose 4 px of corner |
| Text still reads softer than the webview after hinting and binning | M1 judgement | Custom glyph path with `swash` in an `egui_wgpu` callback; one weekend; only if needed |
| No blur of dynamic content under popovers | design | Accepted; popovers get a darker solid fill and a hairline |
| Cross-message selection | M1 | Copy-message buttons; "Copy transcript" in the breadcrumb (already exists today) |
| Frameless resize handles fiddly | M0 | 6 px edge zones with `BeginResize`; Windows snap works through `Maximized` |
| RAM lands at 120 MB, not 80 | M0/M1 script | Still 4× better; check font atlas size (`max_texture_side`), mipmaps, and that the wallpaper texture is display-sized |
| Updater key incompatibility | M6 test | New keypair, users reinstall once (the current feed is dead anyway per AUDIT S-7) |
| egui 0.37 breaks the fonts API again | dependency bump | Pin 0.36 until M7; bump once with the emoji feature |

---

## 8. Measurement gates (run every milestone, paste into PROGRESS)

- The PowerShell script in `docs/audit/2026-09-14/efficiency.md` §6: processes, private MB, idle CPU.
- Cold start: stopwatch from process start to first frame (`eframe` `frame_nr == 1`, log the instant).
- Frame time while streaming: `ctx.input(|i| i.stable_dt)` histogram over a 4 000-token answer.
- Transcript benchmark: synthetic 100k lines, measure open time, scroll frame time, RAM delta.
- Bundle: binary size after `strip`, installer size.

Targets: one process; < 80 MB private idle (< 120 MB with a 4K wallpaper); 0 % idle CPU; < 300 ms cold start; < 4 ms frame while streaming; installer < 8 MB.

---

## 9. First evening, concretely

1. `cargo new crates/parzi-desk`, add to the workspace, deps from §3.
2. `main.rs`: `NativeOptions` with `ViewportBuilder::default().with_decorations(false).with_transparent(true).with_inner_size([1280.0, 800.0]).with_min_inner_size([960.0, 640.0])`, `persist_window: true`.
3. `theme.rs`: load `Theme` from core, build `Visuals`; `fonts.rs`: `FontDefinitions` with the three faces, `FontTweak { hinting: Some(true), subpixel_binning: Some(true), .. }`, `ctx.set_fonts(...)`; `ctx.options_mut(|o| o.text_options...)` if a global override is needed.
4. `wallpaper.rs`: `image::open` → resize to the largest monitor's physical size → `fast_blur(sigma from theme)` → `ColorImage` → `ctx.load_texture(.., TextureOptions::LINEAR.with_mipmap_mode(Some(TextureFilter::Linear)))`; paint with `Image::paint_at` under everything.
5. `titlebar.rs`: copy the `custom_window_frame` example; replace the emoji buttons with `egui::Shape` glyphs.
6. Three panels with translucent `Frame`s; a `Label` with a paragraph of Inter and a mono block; screenshot next to Parzi.
7. Run the measurement script. Write the numbers down.

---

## Sources

- egui releases and changelog: https://github.com/emilk/egui/releases · https://github.com/emilk/egui/blob/main/CHANGELOG.md · epaint changelog https://github.com/emilk/egui/blob/main/crates/epaint/CHANGELOG.md
- Text options and tweaks (0.36.2): https://docs.rs/epaint/latest/epaint/text/struct.TextOptions.html · https://docs.rs/epaint/latest/epaint/text/struct.FontTweak.html · https://docs.rs/epaint/latest/epaint/text/struct.TextFormat.html · https://docs.rs/epaint/latest/epaint/text/struct.FontDefinitions.html
- Colour emoji on native (merged 2026-09-06, targets 0.37): https://github.com/emilk/egui/pull/8512 · https://github.com/emilk/egui/pull/8493
- Scroll virtualization: https://docs.rs/egui/latest/egui/containers/scroll_area/struct.ScrollArea.html
- Frameless windows: https://docs.rs/egui/latest/egui/viewport/struct.ViewportBuilder.html · https://docs.rs/egui/latest/egui/viewport/enum.ViewportCommand.html · https://github.com/emilk/egui/blob/main/examples/custom_window_frame/src/main.rs
- Custom wgpu callbacks: https://docs.rs/egui-wgpu/latest/egui_wgpu/trait.CallbackTrait.html
- eframe options and features: https://docs.rs/eframe/latest/eframe/struct.NativeOptions.html · https://docs.rs/crate/eframe/latest/features · https://docs.rs/crate/egui_extras/latest/features · https://docs.rs/egui_extras/latest/egui_extras/syntax_highlighting/index.html
- Thread-safe repaint: https://docs.rs/egui/latest/egui/struct.Context.html
- Text selection status: https://github.com/emilk/egui/pull/7541 · https://github.com/emilk/egui/issues/3816
- IME: https://github.com/emilk/egui/blob/main/crates/egui-winit/CHANGELOG.md
- RAM data point: https://github.com/emilk/egui/issues/3689
- Markdown alternatives considered: https://github.com/lampsitter/egui_commonmark
- Packaging and updates: https://docs.crabnebula.dev/packager/ · https://docs.crabnebula.dev/packager/configuration/ · https://docs.crabnebula.dev/packager/updater/ · https://docs.rs/cargo-packager-updater/latest/cargo_packager_updater/
