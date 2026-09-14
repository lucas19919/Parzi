# Audit: `parzi-core` (static review, working tree 2026-09-14)

Scope: `crates/parzi-core/{Cargo.toml, src/*.rs, tests/logic.rs}` read in full, plus the
call sites in `src-tauri/src/main.rs`, `crates/parzi-runtime/src/{handler,orchestrator,tools}.rs`
and `crates/parzi-cli/src/main.rs` that feed core. No cargo/npm run. Nothing edited.

Line counts: theme.rs 834, store.rs 515, config.rs 384, lanes.rs 260, paths.rs 175,
artifacts.rs 154, context.rs 154, widgets.rs 151, error.rs 37, lib.rs 14, tests/logic.rs 240.

Headline: the crate is small, panic-free, dependency-clean and mostly sane, but three of the
contracts PLAN.md puts on core are not actually met: (1) every path that takes a user/model
supplied name joins it into `~/.parzi` unvalidated, and the one guard that exists
(`lanes::delete_project`) can be bypassed with `.` or `C:`; (2) a corrupt or newer
`config.toml` panics the GUI at boot; (3) `events.jsonl` is read all-or-nothing, so one torn
or unknown line makes the whole session unreadable. The context builder does not implement
§4 (no budget formula, no 80% checkpoint, latest user message is not pinned). The P1
"round-trip save→load→equal" tests that LOOP.md requires do not exist.

---

## Findings

### P0 / P1

**F1 — P1 — `lanes::delete_project` escapes with `.` and with a bare drive letter**
`crates/parzi-core/src/lanes.rs:146-161`
```rust
if clean.contains('/') || clean.contains('\\') || clean.contains("..") { return Err(..) }
let dir = crate::paths::projects_dir()?.join(clean);
if dir.exists() { std::fs::remove_dir_all(&dir)?; }
```
`"."` passes every check (`".."` is not a substring of `"."`), `join(".")` is the projects
dir itself, `exists()` is true, `remove_dir_all` wipes every workspace. `"C:"` also passes:
on Windows `Path::join` with a path that has a prefix but no root *replaces* the base
(`PathBuf::push` docs), `Path::new("C:").exists()` resolves to the current directory of
drive C, and `remove_dir_all` runs there. The Tauri command `delete_project`
(`src-tauri/src/main.rs:943-968`) forwards `name.trim()` with no further check, so any JS in
the webview can trigger this. `create_project` in Tauri validates `[A-Za-z0-9_-]` but core's
delete does not apply the same rule.
User-visible: every project folder (or the app's cwd on C:) deleted by one IPC call.
Fix: one `paths::safe_name(&str) -> Result<&str>` in core (non-empty, `[A-Za-z0-9_-]{1,64}`,
reject Windows reserved names CON/PRN/AUX/NUL/COM1-9/LPT1-9), used by `delete_project`,
`migrate_tasks_to_lanes`, `check_pack_name`, `set_background`, and by Tauri
`create_project`/`save_project_system`/`list_project_docs`. Additionally assert
`dir.starts_with(projects_dir)` after `join` as a belt. Effort: S.

**F2 — P1 — Session ids are never validated; model-controlled ids reach `Path::join`**
`crates/parzi-core/src/store.rs:120-122`
```rust
fn dir(&self, id: &str) -> PathBuf { self.root.join(id) }
```
Every `SessionStore` method takes `id: &str` and joins it raw. Ids arrive from (a) Tauri
commands (`delete_thread`, `rename_thread`, `toggle_pin`, `set_parent`, `fork`) and (b) the
model itself via `session.read_session` / `session.send_message`
(`crates/parzi-runtime/src/handler.rs:534-553` → orchestrator → `store.get/events/append`).
`grep -rn "Uuid::parse_str\|valid_session_id"` over the workspace: nothing. Consequences:
`append("../../Desktop/x", ..)` creates `events.jsonl` there (`OpenOptions::create(true)`
runs before the `get()` that would fail, store.rs:240-246); `events("<abs path>")` reads any
`events.jsonl`; `delete_thread` is guarded only by "a `meta.json` exists at the joined path".
Fix: `SessionStore::dir` returns `Result<PathBuf>` after `Uuid::parse_str(id)` (ids are
always v4 UUIDs, store.rs:147) and rejects everything else; the CLI's id-prefix resolution
happens before calling core, so it is unaffected. Effort: S.

**F3 — P1 — Corrupt / newer / version-less `config.toml` panics the GUI at boot**
`crates/parzi-core/src/config.rs:232-259` + `src-tauri/src/main.rs:1325`
```rust
let mut cfg: Self = toml::from_str(&text)?;   // any syntax error -> Err
cfg.check_version()?;                          // version != 1 -> Err
...
let (cfg, store) = boot().expect("parzi home"); // Tauri main: panic, no window
```
`pub version: u32` has no `#[serde(default)]`, so a file without `version` is also fatal.
`toml::from_str` also errors when both `auto_order` and its alias `preferred_subscriptions`
are present ("duplicate field"). The CLI (`boot()`) fails the same way; `parzi init`
(`cli/main.rs:174`) does the opposite and silently *overwrites* the corrupt file with
defaults (`load().unwrap_or_default(); cfg.save()`), losing the user's providers/MCP
servers. The brief's question "does a corrupt config brick the app or fall back": it bricks.
Fix in core: `ParziConfig::load_or_recover() -> (Self, Option<String>)` that on parse error
renames the file to `config.toml.broken-<ts>`, returns `Default` plus the message; for
`version > CONFIG_VERSION` return an error with a clear message but let the GUI open in
read-only settings mode instead of `expect`. Give `version` a `default = CONFIG_VERSION`.
Effort: S (core) + S (Tauri/CLI wiring).

**F4 — P1 — `events.jsonl`: torn or unknown line makes the whole session unreadable; append is not single-write**
`crates/parzi-core/src/store.rs:229-251`
```rust
.map(|l| serde_json::from_str(l).map_err(ParziError::Json)).collect()   // all-or-nothing
...
writeln!(f, "{}", serde_json::to_string(event)?)?;   // write_fmt on a raw File
```
`writeln!` on an unbuffered `File` issues at least two `write()` syscalls (payload, then
`\n`); a crash between them leaves a line without newline, the next append glues a second
JSON object onto it, and from then on `events()` returns `Err` for the session: `assemble()`
in the handler fails every run, `fork` fails, `render_md` silently renders an empty
transcript (`unwrap_or_default()` at store.rs:377). The same total failure happens the first
time a newer Parzi appends a `kind` this build does not know (`#[serde(tag="kind")]` has no
catch-all), which contradicts PLAN §2 "never mutate; add new kind". PROGRESS P4 claims
"crash-safe appends"; nothing in core makes them so. No fsync anywhere either.
Fix: build `format!("{json}\n")` and `write_all` once; on read, parse per line, skip
malformed/unknown lines with a `tracing::warn!` (core already depends on `tracing` and never
uses it), and only treat the *last* line as recoverable garbage. Add a test that writes a
half line and asserts the earlier events still load. Effort: S.

**F5 — P1 — Context builder: latest user message is not pinned; one oversized newest item starves everything**
`crates/parzi-core/src/context.rs:628-667`
```rust
for e in self.history.iter().rev() {
    ...
    if used + cost > token_limit { break; }
```
PLAN §4 pins "latest user msg". Here the newest event is the first candidate; if
`system + files + newest` exceeds the limit the loop breaks with `picked` empty and the
provider receives zero messages (or only the `[context]` file block). Because the loop
`break`s rather than skipping, a single large `Assistant`/`ToolResult` at the tail also
drops every older message. Fix: always include the latest `User` event (truncate it with
`〈…〉` if it alone exceeds the budget, never drop); `continue` past items that don't fit
(or stop only after the first N misses). Effort: S.

### P2

**F6 — P2 — PLAN §4 budget formula and 80% auto-compact are not in core (or anywhere)**
`crates/parzi-core/src/context.rs` has no `Budget`, no `context_limit - 20% - 10%`, no
80% trigger. Runtime hardcodes `let limit = 100_000u64; // TODO` and compacts by *event
count* (`events.len() > 96 → compact(.., 20)`, `handler.rs:425-431`), not by budget.
`@file` caps (8 files / 12k chars) are implemented twice outside core
(`src-tauri/src/main.rs:287-306`, `cli/main.rs:251-262`), no `〈…〉` marker, contradicting
"one algorithm for every adapter". `estimate` is a fixed fn, not the swappable `count()`.
Fix: `pub struct Budget { limit, reserve_out, reserve_tools }`, `Budget::for_model(ctx)`,
`ContextBuilder::with_files(..)` that enforces the caps, `assemble()` returns
`needs_compact: bool` when `used > 0.8*limit`. Effort: M.

**F7 — P2 — `render_md` rewrites `session.md` on every append, non-atomically; `Widget` rendering opens an unclosed fence**
`store.rs:238-251` (append → get → write_meta → render_md), `store.rs:375-454`.
Every event (2 per tool call, plus usage/meta writes) re-reads all events and rewrites the
whole markdown: O(n²) per session, and `std::fs::write(session.md)` at store.rs:452 is the
one write path in the store that is *not* atomic (PLAN §2: "all writes atomic"). `fork`
appends one-by-one so it is O(n²) too. Line 408:
```rust
Event::Widget { fence, .. } => md.push_str(&format!("```{fence}\n\n")),
```
opens a fence and never closes it; everything after the first widget in `session.md`
renders inside a code block for other harnesses. Fix: close the fence and dump the payload
(`"```{fence}\n{json}\n```\n\n"`); render md lazily (on `Done`/status change or debounced)
via `atomic_write`; fork should copy the file and re-render once. Effort: S (fence) / M
(cadence).

**F8 — P2 — `atomic_write` is not crash-safe or concurrency-safe**
`crates/parzi-core/src/error.rs:26-37`
```rust
let tmp = path.with_extension("tmp");
std::fs::write(&tmp, bytes)?;
std::fs::rename(&tmp, path)?;
```
No `sync_all` before rename (power loss can leave a zero-length `meta.json`/`config.toml`
after the rename is journaled). Tmp name is deterministic (`config.tmp`, `meta.tmp`,
`theme.tmp`), so GUI + CLI writing the same file at once race on the same tmp. On Windows a
`rename` onto a file another process holds open (editor, AV scan) fails with a sharing
violation and there is no retry. Fix: `File::create` + `write_all` + `sync_all`, tmp name
`<name>.<pid>.<nanos>.tmp`, 3× retry with backoff on `PermissionDenied` for the rename.
Effort: S.

**F9 — P2 — `store.list()` scans every session dir on every call and silently drops unparsable metas**
`store.rs:176-191`. `list()` is called by the sidebar, and again by `list_children`,
`purge_finished`, `delete_thread`, `delete_project_threads`, and by Tauri `delete_thread`
before calling core's `delete_thread` (which lists again). There is no meta index, so
"rebuild from meta.json on boot" (PLAN §6) is really "re-read all N files on every list".
A `meta.json` that fails to parse (`if let Ok(m)`) makes the session vanish from the UI
with no log line. Fix: keep an in-memory `HashMap<id, SessionMeta>` in `SessionStore`
populated once (with `tracing::warn!` per bad file) and updated by every mutator; expose a
`refresh()` for external writers (CLI). Effort: M.

**F10 — P2 — `ParziConfig::migrate` silently deletes user provider entries and half-migrates favourites**
`config.rs:276-279`
```rust
let Some(canon) = alias(&id) else { continue };   // ollama/openrouter/... dropped
```
Any `[providers.X]` whose id is not in the 5-provider roster (`ollama`, `openrouter`,
`mistral`, …, including a user's custom `base_url`) is discarded on load and lost on the
next save. `favorite_models` keeps aliased prefixes (`anthropic/claude-sonnet-4` is retained
but not rewritten to `claude/…`), so favourites no longer match `providers`. When two alias
entries map to one canonical slot and no canonical entry exists, which `base_url` survives
depends on `HashMap` iteration order. The unit test asserts the drops as desired behaviour;
that may be the decision, but it should be a logged, one-time migration that backs the old
file up, not a silent load-time filter. Fix: log dropped ids, rewrite favourite prefixes
through `alias`, prefer the alias entry deterministically (sorted). Effort: S.

**F11 — P2 — Built-in theme packs cannot self-heal; seeded with a non-atomic write**
`paths.rs:134-204` writes each pack's `theme.toml` with `std::fs::write` and skips the
pack forever once the marker file exists. A torn/edited/unparsable built-in `theme.toml`
is skipped by `list_pack_infos` (theme.rs:475-477), `delete_pack` refuses built-ins
(theme.rs:524), so the user has no way to get it back short of deleting the folder by
hand. `BUILTIN_PACKS` (theme.rs:396) and the seed list (paths.rs:135) are two copies of the
same 7 names. Fix: seed via `atomic_write`, re-seed when the file fails to parse, derive
one list from the other. Effort: S.

**F12 — P2 — Widget/diagram validation has no total-size cap; only three of eight types are checked**
`widgets.rs:478-565`. `table` rows ≤ 50, `chart-*` points ≤ 200, `markdown` ≤ 24k chars,
diagram nodes ≤ 200 / edges ≤ 400 (PLAN §8 met on nodes). But `stat`, `progress`, `list`,
`kanban` are unbounded, a table row or a node label can be megabytes, diagram edges may
reference nodes that do not exist and node ids need not be unique. The payload is stored in
`events.jsonl` and re-rendered on every `render_md`. Fix: `serde_json::to_string(v).len()
<= 64_000` guard first, then per-type limits; validate edge endpoints and id uniqueness.
Effort: S.

**F13 — P2 — Store test writes into the user's real `~/.parzi/sessions`**
`tests/logic.rs:160-194` calls `SessionStore::open()` (home dir) and cleans up only on the
happy path. PLAN §11: "Store tests use tempdir". A panic mid-test leaves junk sessions in
the live sidebar; running the suite while the app is open races the orchestrator's
`list()`. Fix: `SessionStore::open_at(root: PathBuf)` (also what F9 and the CLI need) and
use `tempfile::tempdir()` in the test. Effort: S.

**F14 — P2 — `Theme.background.image` is never validated; used as a path in four places**
`theme.rs:507, 557, 676` and `src-tauri/src/main.rs:686, 819`. `save_theme(theme: Theme)`
accepts a whole `Theme` from the webview; `normalized()` clamps numbers but not the image
string. An absolute or `..` image path is then joined (`Path::join` replaces on absolute),
so `save_pack` copies an arbitrary file into a shareable pack, `palette_from_background`
decodes an arbitrary file, `background_url` returns it (the asset-protocol scope
`$HOME/.parzi/backgrounds/*` blocks display, which is the only thing saving this). Fix: in
`Theme::save`/`normalized`, require `image == ""` or `backgrounds/<safe_name>.<png|jpg|jpeg|webp>`.
Effort: S.

**F15 — P2 — `image::open` in `extract_palette` runs with default decoder limits**
`theme.rs:676-679`. `is_bg_file` (20 MB, regular file, no symlink) is applied for saved
backgrounds but `extract_palette` itself takes any path, and `image::open` uses the crate's
default `Limits` (no width/height cap, 512 MiB allocation cap). A 20 MB PNG can legally
decode to hundreds of MB before `thumbnail(64,64)`. Fix: use `ImageReader::open` +
`limits.max_image_width/height = 16384`, `max_alloc = 128 MiB`, and reject files over
`BG_MAX_BYTES` via `symlink_metadata` first. Effort: S.

### P3

**F16 — P3 — theme.rs is 834 lines (limit 400) and holds five concerns**
Theme struct + CSS, user.css, packs, backgrounds, palette extraction. Split into
`theme/{mod,css,packs,backgrounds,palette}.rs`. Effort: S.

**F17 — P3 — `css_value` filter is incomplete**
`theme.rs:278-282` strips `; { } \n` only. `/*` in any colour comments out every later
variable (`accent = "red /*"` kills text/dim/glass vars until the next `*/`, which never
comes); `\r`, `url(`, `var(`, `<`, `>` pass. Injection into HTML is prevented because the UI
assigns via `style.textContent` (`ui/src/lib/theme.ts:99`), so this is theme defacement
only, but a shared pack can do it. Fix: allow-list `[#A-Za-z0-9(),.%\- ]` (hex, rgb(),
hsl(), color-mix(), oklch()) and drop anything else. Effort: S.

**F18 — P3 — `read_user_css` has no size cap; `apply_pack` copies pack `user.css` unchecked and non-atomically**
`theme.rs:365-372, 565-568`. `write_user_css` enforces 64 KB but a pack's `user.css` is
`fs::copy`'d straight over the live file with no limit. `user.css` is by design arbitrary
CSS; with no CSP in `src-tauri/tauri.conf.json` (`security` has only `assetProtocol`) a
third-party pack's `url()` can beacon. Core fix: apply `USER_CSS_MAX` on both paths and
write through `atomic_write`; CSP belongs to the Tauri auditor. Effort: S.

**F19 — P3 — `check_pack_name`, `delete_pack`, `save_pack` vs Windows case-insensitivity**
`theme.rs:410-421, 497, 522-535`. `Dracula` is not in `BUILTIN_PACKS` but resolves to the
`dracula` folder on NTFS, so the built-in guard is bypassed (it re-seeds at next boot, so
low impact). `save_pack` has no built-in guard at all. Unicode `is_alphanumeric` also
allows names NTFS can't create. Fix: compare with `eq_ignore_ascii_case`, ASCII-only names.
Effort: S.

**F20 — P3 — `migrate_tasks_to_lanes(project)` joins an unvalidated project name; `subfolder` unvalidated; `let _ =` swallows the lane-root write**
`lanes.rs:187-259`. `project` comes from Tauri `migrate_tasks` (`main.rs:1286`) raw; task
`id` is checked for `..`/slashes but not drive prefixes; `subfolder` from `task.json` is
not checked for `..` before it becomes the lane's sandbox `root`; line 254 discards the
`parzi.toml` write error so a lane can be created with its root silently missing.
`format!("root = {full:?}")` uses Rust `Debug` as a TOML string encoder. Effort: S.

**F21 — P3 — `read_lane_file` and `list_pack_infos` swallow parse errors silently**
`lanes.rs:63-68`, `theme.rs:472-477`. A typo in `parzi.toml` silently resets the lane to
`mode=ask`, no root, no model; a bad pack just disappears from the picker. `tracing` is a
declared dependency of core and is used nowhere (`grep tracing:: crates/parzi-core/src` →
none). Fix: `warn!` with the path on every swallowed error. Effort: S.

**F22 — P3 — `SessionStatus` transitions are unchecked; `set_parent` allows cycles**
`store.rs:274-279, 206-220`. `Done → Active`, `Killed → Queued` are accepted; `A→B` then
`B→A` parenting is accepted (the walkers use visited sets so no hang, but the UI tree is
cyclic). `add_usage`/`append` are read-modify-write with no lock, so a usage event landing
while the handler appends can lose a token count. Effort: S.

**F23 — P3 — `ArtifactV1.id` is a required field although empty ids are handled**
`artifacts.rs:291-304`. `id: String` has no `#[serde(default)]`, so an artifact without an
`id` key fails with "missing field" before the `slugify_id(&a.title)` fallback at line 351
can run. `artifact_kind` of `html`/`svg` is accepted unsanitized (UI must sandbox; out of
scope here but worth flagging to the UI auditor). Effort: S.

**F24 — P3 — `LaneDefaults.default_mode` / `LaneFile.mode` are free strings**
`config.rs:80-81`, `lanes.rs:11-13`. `auto|ask|deny` is documented, never validated; an
unknown value's behaviour depends on the runtime's match arms. Fix: an enum with
`#[serde(rename_all="lowercase")]` and `#[serde(other)] Ask`. Effort: S.

**F25 — P3 — Save drops unknown fields and comments; no theme version**
`ParziConfig`/`Theme` do not use `deny_unknown_fields` (good: forward-tolerant on read),
but `save()` re-serialises the struct, so fields from a newer version and the user's TOML
comments are lost on the first settings save. `theme.toml` has no `version` at all. Effort:
M if you want round-trip preservation (`toml_edit`), S to document.

---

## Verified good (do not refactor)

- Dependency rule holds: `crates/parzi-core/Cargo.toml` has no providers/runtime/tauri/
  tokio/reqwest; `grep parzi_|tauri|tokio` in `src/` is empty. No `unsafe`.
- No `unwrap`/`expect`/`panic!`/`todo!` outside `#[cfg(test)]` in core.
- `meta.json`, `config.toml`, `theme.toml`, `user.css`, pack `theme.toml`, lane `SYSTEM.md`
  and `parzi.toml` all go through `atomic_write` (only `session.md` and first-run pack
  seeding do not).
- `config.toml` carries `version = 1`, `check_version()` rejects other versions, `migrate()`
  exists and has a unit test with real alias/order assertions (`config.rs:341-372`).
- `McpServerCfg::is_tool_exposed` / `tool_mode` are fail-safe (deny wins, unknown mode → ask).
- Events are single-line JSON (serde escapes newlines), so the JSONL framing is right in
  the happy path; `Event` variants have `#[serde(default)]` on the fields that were added
  later (`done`, `ok`, `ms`, `cooldown_secs`, `parent_id`).
- `subtree_ids` / `cascade_kill_ids` are pure, cycle-safe and tested (`tests/logic.rs:197`).
- Widget/diagram/artifact validators fail closed (return `Err`, callers fall back to a code
  block); version fields are checked; node cap 200 matches PLAN §8; tests assert the caps.
- `css_font_list` quotes family names and strips `" \ ; { }`; `accent_ink` luminance
  math is correct; `num()`/`pct()` output is unit-tested; the UI injects the generated CSS
  via `textContent`, so no HTML breakout.
- `is_bg_file` uses `symlink_metadata` (symlinks/junctions rejected), extension allow-list,
  20 MB cap; `set_background` rejects `/ \ ..`; `check_pack_name` is a strict allow-list.
- `parzi_dir()` handles `home_dir() == None` as a typed error, not a panic; all path helpers
  return `Result`.
- `ContextBuilder::compact` keeps the last N raw and folds the rest into one `Checkpoint`;
  tool results are tagged `untrusted` in the assembled context.
- `Theme::normalized()` clamps every numeric field before save and CSS emission.

## Tests to add (each is a gate the current suite does not have)

1. `config_roundtrip`: `ParziConfig::default()` (plus one MCP server, one provider with
   `base_url`) → `save()` → `load()` → `assert_eq!` on the TOML re-serialisation (needs
   `PartialEq` or compare `toml::to_string`). Use an env override or `save_to(path)` so the
   test uses a tempdir, not `~/.parzi`. LOOP.md P1 gate; missing today.
2. `theme_roundtrip`: same for `Theme`, including that `normalized()` is idempotent.
3. `config_corrupt_falls_back`: write `version = 1\n[providers` (unterminated) → loader
   returns defaults + a warning and the broken file is renamed, not deleted.
4. `config_missing_version_and_newer_version`: both must not panic the caller.
5. `events_partial_line_recovery`: append 3 events, append a half line by hand, assert
   `events()` returns 3; then append a 4th and assert 4.
6. `events_unknown_kind_skipped`: a line `{"kind":"future","x":1}` does not break the read.
7. `session_id_rejected`: `store.get("../x")`, `store.append("C:evil", ..)`,
   `store.delete_thread(".")` all return `Err` and create nothing.
8. `delete_project_rejects_dot_and_drive`: `"."`, `".."`, `"C:"`, `"CON"`, `"a/b"`.
9. `context_pins_latest_user`: `system` of 90 tokens, limit 100, newest user message of
   50 tokens → the user message is still present (truncated), never empty `messages`.
10. `context_skips_oversized_tail`: newest event is 10× the limit, older ones small →
    older ones still fill.
11. `context_budget_formula`: `Budget::for_model(128_000)` == 128_000 − 20% − 10%; and
    `needs_compact` flips at 80%.
12. `files_capped_at_8_and_12k` with the `〈…〉` marker (today the test only asserts a System
    message exists).
13. `render_md_closes_widget_fence`: a `Widget` event followed by a `User` event renders
    the user heading outside a code block.
14. `atomic_write_concurrent`: two threads writing the same path 100× never leave a file
    that fails to parse.
15. `builtin_pack_reseeds_when_corrupt`.
16. `theme_rejects_absolute_background_image`.
17. Counter test PLAN §12 asked for: `estimate("")==1`, `estimate("abcd")==1`,
    `estimate(8-byte)==2`, and `estimated_tokens == system + files + picked` exactly.
18. Move `subsession_hierarchy_lists_and_reparents` onto `SessionStore::open_at(tempdir)`.

## Cross-area notes (not core, surfaced while tracing calls)

- `src-tauri/src/main.rs:1325` `boot().expect(..)` — see F3.
- `src-tauri/src/main.rs:800-809` `background_file(name)` has no traversal check
  (`set_background` in core does).
- `src-tauri/tauri.conf.json` has no `csp`; relevant to the `user.css` threat model (F18).
- `crates/parzi-runtime/src/handler.rs` wraps every `store.append` in `let _ =` — a failed
  append (disk full, F4-style error) is invisible; PLAN §5 says every event is persisted.
- PROGRESS.md P1 row cites "cargo check green" as evidence; LOOP.md §4 requires the
  round-trip test for P1 acceptance. The test does not exist.
- Working tree moved during the audit: `config.rs:181` changed from
  `("codex", "gpt-5.3-codex")` to `("codex", "gpt-5.5")` between my first read and the
  end of the review (another lane). Load/migrate logic was re-read afterwards and is
  unchanged; all findings above are against the current file. Note `migrate()` only
  rewrites `default_model` for *aliased* ids, so existing configs keep `gpt-5.3-codex`
  until the user changes it — fine if intended, but the catalog must still resolve it.
