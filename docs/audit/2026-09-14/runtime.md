# Audit: `parzi-runtime` (handler, orchestrator, tools, mcp, plugins, circuit breaker, doctor)

Static review of the working tree (uncommitted diff included) against PLAN.md §5/§6/§7/§10/§11 and the claims in PROGRESS.md. No cargo/npm run. Line numbers are from the working tree on 2026-09-14.

Summary: 3 blockers, 10 serious, 16 moderate, 11 nits. The PLAN P4/P5 acceptance tests (kill-mid-tool, fork-equality, MCP spawn→list→call→idle-kill) do not exist; PROGRESS P4's "evidence: compiles; kill/fork paths exercised via CLI" is not a test. The three blockers: (1) the fs sandbox is bypassed by any absolute/rooted path, (2) every child, harness-queued or messaged session runs with `AutoApprover`, so lane `ask` mode is bypassed after one approved `session.spawn`, (3) run handles are never removed when a run finishes, so after `max_concurrent` finished runs every new run queues forever and a second message to any finished thread is refused.

---

## P0 — blockers

### P0-1 fs sandbox: absolute / rooted / drive-relative paths escape the lane root
- **File**: `crates/parzi-runtime/src/tools.rs:344-367`
- **What**: `resolve()` does `base.join(path)` and then only counts `..` components. `Path::join` replaces `base` entirely when `path` is absolute (`C:\Users\x\.ssh\id_rsa`, `/etc/passwd`), rooted (`\foo`), UNC (`\\server\share`), or drive-relative (`C:foo`). No component is `ParentDir`, so `depth` never goes negative and the path is accepted. There is no canonicalisation and no `starts_with(root)` comparison at all, so junctions/symlinks inside the lane, `\\?\` prefixes, and 8.3 short names are equally unhandled.
- **Evidence**:
  ```rust
  let joined = base.join(path);
  // Lexical containment: `a/b/../../x` must not escape `a`.
  let mut depth = 0i32;
  for c in joined.components() {
      match c {
          std::path::Component::ParentDir => depth -= 1,
          std::path::Component::Normal(_) => depth += 1,
          _ => {}
      }
  ```
- **Consequence**: any lane that allows `fs.read` can read every file the user can; `fs.write` can overwrite any file (incl. `~/.parzi/config.toml`, `.ssh/authorized_keys`). PROGRESS P5 "sandbox escapes rejected" is true only for `..`. Test `path_escape_rejected` only covers `../../secret`.
- **Fix**: reject if `path.is_absolute()` or the first component is `Prefix`/`RootDir`; then canonicalise the base and the deepest existing ancestor of the joined path and require `starts_with` (both canonicalised, so `\\?\` prefixes match). Refuse when `cwd` is empty (see P1-7). Tests: `C:\Windows\win.ini`, `\foo`, `C:foo`, `\\?\C:\x`, `//server/share`, junction.
- **Effort**: S

### P0-2 Lane `ask` mode is bypassed for every child / harness-queued / messaged session (AutoApprover)
- **Files**: `orchestrator.rs:829` (`spawn_session` → `approver: None`), `:901` (`send_message` → `approver: None`), `:535` (`create_subsession` → `approver: None`), `:646` (`q.approver.clone().unwrap_or_else(|| Arc::new(AutoApprover))`), `handler.rs:495-499`.
- **What**: a `QueuedRun` without an approver is launched with `AutoApprover`. In `ask_approver` the `select!` races the approver against the UI reply channel; `AutoApprover::approve` resolves immediately with `Allow`, so the Ask branch is a no-op. The comment at `orchestrator.rs:85-86` ("Pumped headless ... with AutoApprover; lane Ask mode still logs denials visibly") is false — it approves, it does not deny.
- **Evidence**:
  ```rust
  q.approver.clone().unwrap_or_else(|| Arc::new(AutoApprover)),
  ...
  tokio::select! {
      _ = self.cancel.cancelled() => false,
      r = self.approver.approve(&info) => matches!(r, Approval::Allow),
      r = reply_rx => matches!(r, Ok(Approval::Allow)),
  }
  ```
- **Consequence**: user approves one `session.spawn` card (or the agent calls `session.send_message` to an idle session, or the UI creates a subsession via `create_subsession`) → the child runs `shell.exec`, `fs.write`, any MCP tool with zero prompts although the lane says `ask`. With P1-5 (child may choose any lane/model) this is a complete policy escape.
- **Fix**: fail closed — when `mode == Ask` and no interactive approver exists, use `DenyApprover` and append a System event saying why; or carry the parent's approver in `Pump` as the default (`Pump { default_approver: Arc<dyn Approver> }`, set by the Tauri layer to `GuiApprover`). Remove `AutoApprover` as a silent default.
- **Effort**: S–M

### P0-3 Run handles are never removed when a run finishes → permanent "busy"
- **Files**: `orchestrator.rs:659-670` (insert), `:464` (the only `remove`, inside `kill`), `:237/:256`, `:285`, `:308`, `:679`, `:725`.
- **What**: `launch` inserts `Handle{cancel, _task}` into `handles`; the driver task only calls `notify_one()` on completion. Nothing prunes finished handles (no `is_finished()`, no `remove` on Done). `git show HEAD` confirms it was never there. `handles.len()` is the live count everywhere.
- **Evidence**:
  ```rust
  let task = tokio::spawn(async move {
      if let Err(e) = run.run(&prompt).await { ... }
      parts.notify.notify_one();
  });
  handles.lock().await.insert(sid, Handle { cancel, _task: task });
  ```
  and `send_to`: `if self.handles.lock().await.contains_key(id) { return Err(ParziError::Store(format!("run {id} is active; kill it first"))); }`
- **Consequence**: in the GUI (long-lived `Orchestrator`): after 4 (default `max_concurrent`) completed runs every new thread parks as `Queued` forever; and a second message to any thread whose first turn completed is refused with "run … is active; kill it first". The CLI hides it because it builds a fresh `Orchestrator` per invocation. The teamwork test `spawn_wait_degrades_to_queued_when_slots_full` (`tests/teamwork.rs:199-227`) codifies the leak: it expects a slot to still be occupied 500 ms after a child that answers instantly has finished ("the bridge counts live handles").
- **Fix**: in the spawned task, after `run.run()` returns: `parts.handles.lock().await.remove(&sid_task);` before `notify_one()`; additionally prune `h._task.is_finished()` entries wherever `handles.len()` is read. Rewrite the teamwork test to hold the slot with a hanging provider.
- **Effort**: S

---

## P1 — serious

### P1-1 Cancellation does not abort an in-flight stream or tool; a killed run keeps running and overwrites `Killed`
- **Files**: `handler.rs:337` (`while let Some(ev) = rx.recv().await` — no `select!` on cancel), `:466` (`self.tools.execute(...)` unguarded), `:455-458` (`session.spawn(wait=true)` blocks up to 180 s unguarded), `:396-397` (`finish(Done)` after the fact), `orchestrator.rs:463-474` (`kill` cancels the token but never `abort()`s `_task`).
- **What**: the token is observed only at the top of the loop (`:298`), between tool calls (`:401`) and inside `ask_approver`. PLAN §5: "Stop button + `parzi kill` cancel mid-tool."
- **Consequence**: Stop during streaming waits for the provider (or forever with a hung stream — `HangProvider` in `tests/queue.rs` leaks the task); Stop during a 120 s `shell.exec` or a 30 s MCP call waits; the child process is not killed. After `kill()` sets `Killed`, the detached task continues, appends Assistant/ToolResult events to the killed session and `finish(SessionStatus::Done)` flips the status back to Done/Idle.
- **Fix**: `tokio::select!` with `cancel.cancelled()` around the stream loop and around `execute_tool`; on cancel kill the subprocess (P1-2); `kill()` should `abort()` the JoinHandle as a backstop; `finish()` must never downgrade `Killed`.
- **Effort**: M

### P1-2 Orphaned subprocesses: shell timeout, MCP init timeout (every turn), doctor, app exit
- **Files**: `tools.rs:446-462` (`timeout(c.output())`, no `kill_on_drop(true)`), `mcp.rs:158-165` (no `kill_on_drop`), `tools.rs:102-113` (`timeout(12 s, exposed_tools)`), `handler.rs:315` (`tool_defs` runs every turn, not "once per run start" as `tools.rs:101` claims), `doctor.rs:145-153`, no shutdown hook anywhere (`grep kill_on_drop` → none).
- **What**: tokio `Child` is not killed on drop by default. (a) `shell.exec` timeout drops the future → the command keeps running. (b) `defs_with_mcp` cancels `ensure_live` after `Command::spawn` but before `live.insert` when initialize takes >12 s (cold `npx -y …` download) → the half-initialised `LiveServer` is dropped → orphan node process, once per turn per slow server. (c) `doctor` on timeout leaves the child running and drops the manager. (d) On app exit nothing stops live servers (Windows does not kill children with the parent). (e) `cmd /C` on kill only kills `cmd.exe`, not the tree.
- **Consequence**: zombie `node`/`cmd` processes accumulate; a timed-out `shell.exec` still mutates the repo after the model was told "timed out".
- **Fix**: `.kill_on_drop(true)` on every `Command`; explicit `McpManager::shutdown()` called from Tauri on exit and at CLI end; kill via a Windows Job object or `taskkill /PID <pid> /T /F` on timeout; cache `defs_with_mcp` per run.
- **Effort**: M

### P1-3 MCP JSON-RPC: no id matching — any notification desyncs the connection; timeouts never resync
- **File**: `mcp.rs:111-142`
- **What**: `request()` writes one line and reads exactly one line, assuming it is the response. MCP servers legitimately emit `notifications/message` (logging), `notifications/tools/list_changed`, and progress notifications on stdout. A notification line has no `result` → `tools/list` returns `[]` and `tools/call` returns `"null"` as success; the real response is consumed by the next request → every later call gets the previous call's answer. After a read timeout (`:131-133`) the late response stays in the `BufReader` and poisons the next call; the server is not dropped.
- **Evidence**:
  ```rust
  tokio::time::timeout(timeout, sv.stdout.read_line(&mut out)).await ...
  let v: serde_json::Value = serde_json::from_str(out.trim()) ...
  Ok(v.get("result").cloned().unwrap_or(serde_json::Value::Null))
  ```
- **Consequence**: intermittent "empty tool list" / wrong tool outputs with any chatty server; the model acts on the wrong tool's result.
- **Fix**: loop reading lines until `id == expected`; skip/handle notifications (`method` present, no `id`); on timeout `stop()` and respawn; check `protocolVersion` in the initialize result.
- **Effort**: M

### P1-4 No cost/token budget stop (PLAN §5 "stop on … budget")
- **File**: `handler.rs:303-307` (only `max_steps`), `:316` (`max_tokens` is the per-response output cap).
- **What**: PROGRESS P4 claims "budget"; there is no per-run or per-session USD/token ceiling. `turns` is reset per provider slot (`:296`), so with failover the 32-step cap is per slot, not per run.
- **Consequence**: a tool-looping run on `ultra` (262 k output tokens) × 32 steps × N slots is unbounded spend; Usage events are recorded but never compared to anything.
- **Fix**: `Budget { max_cost_usd, max_tokens }` on `AgentRun`, checked after each `Usage` event and before each `chat_stream`; stop with Notice + `Done`; make `turns` run-level.
- **Effort**: S–M

### P1-5 `session.*` tools skip the lane allowlist, cross project/lane boundaries, and inject into running sessions
- **Files**: `handler.rs:451-459` (session tools dispatched before `tools.execute`, so `is_allowed` is never consulted for them), `orchestrator.rs:807-808` (`lane_name = lane.unwrap_or(caller.lane)` — any lane, unvalidated; `model` override unbounded), `:931-971` `read_session` and `:973-990` `list_sessions` (any session, any project, no approval), `:872-874` (`send_message` appends `Event::User` to a running session's transcript).
- **What**: a lane restricted to `allowed_tools=["fs.read"]` still spawns children, and a child may specify `lane: "auto-lane"` whose `parzi.toml` has `mode="auto"`, `allowed_tools=["*"]` → policy escalation (with P0-2 the child also auto-approves). Reads expose every other project's transcripts (secrets pasted in other threads). Writing a `User` event into a running session mid-tool-loop is cross-session prompt injection and can land between `ToolCall` and `ToolResult`.
- **Fix**: validate `lane` exists under `caller.project` and inherits the caller's mode unless stricter; child allowlist = intersection with the parent's; scope `read_session`/`list_sessions` to the caller's project or require approval; require approval for `send_message` into a running session; either honour `is_allowed("session.*")` or document them as always-on and drop them from `lane_policy_for`.
- **Effort**: M

### P1-6 Queued runs: live events go nowhere, prompt is lost on restart, launch errors strand sessions as `Queued`
- **Files**: `orchestrator.rs:694` (`Self::launch(...)` → `Ok(rx)` discarded), `:266/:316` (`closed_rx()` returned to the UI), `:197-206` (`recover()` only flips Active→Idle), `handler.rs:201-202` (User event appended only at `run()`), `orchestrator.rs:694-696` (launch error → `return` with status still `Queued`, nothing appended), `src-tauri/src/main.rs:1336` (comment "boot kick for sessions left Queued" — the in-memory queue is empty at boot, so `kick()` does nothing for them).
- **Consequence**: a queued run's text/tool events never reach the UI live (only the transcript); if the app exits while queued, the user's message is gone (it existed only in `QueuedRun.prompt`); a queued run whose provider is unauthenticated stays "queued" forever with an empty transcript.
- **Fix**: append the `User` event at enqueue; at boot re-enqueue `Queued` metas from the last `User` event (or mark Idle with a System note); on launch error append the error and set Idle; give the pump a per-session event broadcast the Tauri layer subscribes to.
- **Effort**: M

### P1-7 Sandbox root defaults to the app process cwd; `spawn()` never persists `cwd`
- **Files**: `tools.rs:345-349` (`cwd.is_empty()` → `std::env::current_dir()`), `orchestrator.rs:242` (`meta.cwd = cwd.to_string()` on a local copy) vs `:269` (`meta = self.store.get(&meta.id)?` reloads from disk — cwd lost), `:303` (`send_to` falls back to `meta.cwd`), `:817-818`/`:523` (children inherit `caller.cwd`, which is empty). Tauri only calls `set_cwd` in the parent_id branch (`src-tauri:176`).
- **Consequence**: `fs.*` and `shell.exec` in subsessions / follow-up turns without an explicit cwd run in whatever directory the Tauri process was started from (exe dir, `C:\Windows\System32` from some launchers). The "lane root" becomes the app install dir.
- **Fix**: `store.set_cwd(&meta.id, cwd)` in `spawn`; in `resolve`, refuse when `cwd` is empty (fail closed) or resolve to `lanes::lane_root(project, lane)`.
- **Effort**: S

### P1-8 Full parent environment inherited by `shell.exec` children and every MCP server
- **Files**: `tools.rs:433-445` (no `env_clear`, stdin inherited, stderr discarded on success), `mcp.rs:158-160` (`.envs(&cfg.env)` on top of the inherited env).
- **What**: providers read API keys from env (`keyring+env auth`), so `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, OAuth tokens in env etc. are visible to every third-party MCP server package and to every shell command the model runs. CLI: stdin inherited → a child reading stdin blocks the terminal.
- **Fix**: `env_clear()` then re-add an allowlist (`PATH`, `SYSTEMROOT`, `TEMP`, `HOME`/`USERPROFILE`, `APPDATA`, locale) plus `cfg.env`; `stdin(Stdio::null())`.
- **Effort**: S

### P1-9 Tool results are role=user text with a string "untrusted" prefix; tool calls are flattened to text
- **Files**: `parzi-core/src/context.rs:73-83`; adapters map `Role::Tool` → `"user"`: `openai_compat.rs:57`, `opencode.rs:174`, `codex.rs:150`, `antigravity.rs:145`.
- **What**: the "tagged untrusted" of PLAN §5 is literally `format!("[result:{name} untrusted]\n{output}")` sent as a user turn — the role the model is trained to obey. Prior tool calls become assistant text `[tool:name {...}]` (ids dropped), so there is no native tool_use/tool_result pairing on any provider.
- **Consequence**: injection in tool output is indistinguishable from the user; models learn to emit the fake `[tool:…]` syntax as prose instead of real tool calls. (Cross-cutting with the core/providers auditors; listed here because the handler owns the promise.)
- **Fix**: keep ids on `ToolCall`/`ToolResult`; adapters emit native tool blocks; wrap output in a delimited data block with a system-level instruction.
- **Effort**: M

### P1-10 Transcript appends and status writes are silently swallowed
- **File**: `handler.rs:211, 248, 352, 361, 379, 385, 405, 411, 420, 567, 585, 596, 606, 697` (`let _ = self.store.append/set_status`).
- **What**: PLAN §2/§5: "append-only truth … Every event persisted." A disk-full or permission error drops Assistant/ToolCall/ToolResult events and the run continues; `finish()` ignoring `set_status` leaves the thread `Active` in the UI until the next boot.
- **Fix**: propagate with `?` (turn into `RunEvent::Error` + `Killed`) for User/Assistant/ToolCall/ToolResult/status; keep best-effort only for Widget/Notice.
- **Effort**: S

---

## P2 — moderate

### P2-1 Per-tool `auto` override escalates above lane `deny` ("Lockdown")
`handler.rs:473-478`: `Some(ApprovalMode::Auto) => return true` before `self.mode` is consulted. A connector tool pinned `auto` runs in a `deny` lane (the lane allowlist is still required, but the UI's "Lockdown" segment implies nothing runs). Fix: lane `Deny` is absolute; the override may only tighten or relax `Ask`. S.

### P2-2 Two half-decisions instead of one decision function, in the wrong order
`handler.rs:461-466` asks the user (`approved`) before `tools.execute` (`tools.rs:125-143`) checks allowlist → exposure → per-tool deny. Users get approval cards for tools that will be denied anyway, and the card cannot say why. The executor knows nothing of Ask/Auto, so any other `execute` caller skips prompting. Fix: `ToolExecutor::decide(name) -> Denied(reason) | Auto | Ask` used by the handler; check allowlist/exposure first. S.

### P2-3 One global MCP mutex serialises every call across all sessions and servers
`mcp.rs:221-223, 269-271` lock `live` for the whole request; `ensure_live` (`:144-196`) holds it during spawn+initialize (up to `timeout_ms`, default 30 s). `defs_with_mcp` (every turn, P1-2) waits up to 12 s per unreachable server per turn. Fix: per-server `Mutex<LiveServer>`; cache defs per run. M.

### P2-4 Idle reap only runs on the next MCP op
`mcp.rs:198-210` ("Called on every op — no bg task needed"). "Kill after 60 s idle" (PLAN §7) is not met: an idle server lives until the next MCP call from any session or app exit (then orphaned, P1-2). Fix: reaper task/timer, or reap from `pump`. S.

### P2-5 MCP descriptions/schemas and results passed verbatim and uncapped
`mcp.rs:30-36, 236-241` (description/inputSchema straight into `ToolDef`), `:279-289` (result text joined without cap; `fs.read` caps at 24 k, MCP at nothing). Server-controlled prompt-injection surface; a large result bloats `events.jsonl` and every later `assemble()`. Fix: cap description (1 k), result (24 k), strip control chars, note truncation. S.

### P2-6 Circuit breaker trips on non-retriable errors; cooldown unclamped; only a Settings button resets
`circuit_breaker.rs:49-68` (3 failures of any kind → 45 s), `handler.rs:270-275` (records auth/400 errors, e.g. the known antigravity tool-schema 400), `router.rs:128-145` (`parse_cooldown_secs` unclamped → `retry after 86400` disables a provider in-process for a day), `orchestrator.rs:459` reset. Not persisted, so restart clears (fine). Fix: count only 5xx/transport errors; clamp cooldown (≤15 min); show cooldown in the model menu with a one-click reset. S.

### P2-7 `plugins::scan()` fails every plugin when one manifest is bad
`plugins.rs:69-81` (`?` on read/parse, `return Err` on unknown kind) contradicts its own doc ("A broken manifest disables that plugin, loudly") and takes down `slash_commands`, `themes`, `set_enabled`, `commands_for`, `save_commands`, `delete_skill` and the Skills page. Fix: per-plugin error collection, skip and surface. S.

### P2-8 Plugin entry/dir inconsistencies: unvalidated `entry` in `slash_commands`, name-vs-dir mismatch can delete the wrong pack
`plugins.rs:99-104` uses `manifest.entry` directly (`entry = "../../x.toml"` or an absolute path is read as TOML) while `commands_for`/`save_commands` use `entry_for` (`:150-163`). `delete_skill` (`:680-694`), `commands_for` (`:187`), `save_commands` (`:246`) compute `pack_dir(name)` from the manifest name, but `scan()` keys by directory: a pack whose manifest name differs from its folder makes `delete_skill("foo")` remove `plugins/foo` — possibly another plugin. Fix: use `p.dir` from `scan()`; call `entry_for` everywhere. S.

### P2-9 `mcp-pack` is accepted but nothing consumes it
`plugins.rs:75, 562`; the UI lists them (`SkillsSection.svelte:47`). PLAN §7 lists it as a v0.1 kind; PROGRESS P5 claims "plugins (manifest packs)". Fix: implement (merge pack servers into `cfg.mcp.servers` on enable) or drop from docs/UI. S–M.

### P2-10 `copy_dir` follows symlinks; no clone size cap
`plugins.rs:532-546` (`is_dir()` and `fs::copy` follow links). A cloned skill repo with a symlink to `~` copies arbitrary user files into `~/.parzi/plugins`. Fix: `symlink_metadata`, skip links; cap total bytes. S.

### P2-11 `Command::new("npx")` does not resolve `npx.cmd` on Windows
`mcp.rs:158` spawns `cfg.command` directly. Rust's `CreateProcessW` path search only appends `.exe`; `npx`/`uvx` are `.cmd` shims → "program not found" unless the user writes `npx.cmd` or `cmd /c npx`. The Connectors paste flow accepts bare `npx …` lines (`ConnectorsSection.svelte:117`). Verify with `parzi doctor` after adding a preset (not run here). Fix: on Windows resolve `.cmd/.bat` (or wrap in `cmd /c`) at spawn. S.

### P2-12 Doctor gaps vs PLAN §10 and a false-negative WebView check
`doctor.rs:84-105` = auth presence only, no provider ping; no disk/log write-permission probe, no log-rotation check; `check_config` re-loads from disk instead of validating `self.cfg`; `reg_key_version` (`:175-188`) reads only `HKLM\...\WOW6432Node` — per-user WebView2 installs register under `HKCU` → "version not detected". Secrets: none printed (details are paths, hints, counts). S each.

### P2-13 `spawn()` creates the session before the busy check → orphan sessions on "busy" reject
`orchestrator.rs:241` creates the meta, `:256-261` returns `Err("busy")` in reject mode leaving an empty Idle session on disk. Fix: check first, create after. S.

### P2-14 Mid-stream failover leaves a trailing partial assistant turn, then re-sends
`handler.rs:360-366` appends `Assistant{done:false}`; the next slot's request then ends with an assistant message (OpenAI-compat backends may 400 "last message must be user"; Anthropic treats it as prefill). Fix: append as a System note or a User "(continue)" bridge. S.

### P2-15 `kill()` calls `mcp.stop(session_id)`
`orchestrator.rs:469` passes a session id to a function keyed by server name → no-op. MCP servers are shared and never stopped per session; an MCP call in flight for a killed session finishes anyway (P1-1). Fix: remove the call; rely on cancel + P1-1. S.

### P2-16 `Handle._task` is a detached JoinHandle
`orchestrator.rs:66-69`; dropping `Orchestrator` (tests) or `kill()` never aborts the task. Combined with P1-1 this is what keeps killed runs alive. Fix: `abort()` in `kill`, `Drop` for `Handle`. S.

---

## P3 — nits

- `mcp.rs:224, 272` `expect("live after ensure")` — PLAN §11 forbids `unwrap/expect` outside tests. `Cargo.toml` sets clippy `all/pedantic = "warn"`; PLAN says deny in CI (LOOP.md denies only correctness/suspicious/complexity/perf).
- Files > 400 lines (PLAN §11): `orchestrator.rs` 1030, `handler.rs` 806, `plugins.rs` 802, `tools.rs` 466.
- `handler.rs:786` `let _ = cfg;` — unused parameter kept alive.
- `tools.rs:128-135`: MCP server names `fs`/`ui`/`session` collide with built-ins (`fs.read` on a server named `fs` is shadowed silently); a server name containing `.` breaks `split_once`.
- `orchestrator.rs:588` always-allows only 3 `ui.*` tools, not `ui.show_artifact` — harmless because `handler.rs:448` runs ui tools before any allowlist check, but inconsistent.
- `handler.rs:196-202`: with an empty slot list the user's message is never appended.
- `orchestrator.rs:677-685` pump TOCTOU: handle count checked, lock dropped, then pop → a concurrent `spawn` can exceed `max_concurrent` by one.
- `orchestrator.rs:737-754` `await_settled` polls `meta.json` 4×/s for 180 s; `handler.rs:424, 648` re-read the whole `events.jsonl` every turn / every artifact; `store.fork` re-renders `session.md` per appended event (O(n²)).
- `handler.rs:487-494` approval card carries the full `args` (a 1 MB `fs.write` content goes over the Tauri event bus).
- `handler.rs:409-414` crash between tool execution and `ToolResult` append: not replayed on recovery (good), but the side effect is unrecorded; the next turn's context ends with `[tool:…]` text and no result.
- `tools.rs:453-456` stderr is dropped when the exit code is 0 (warnings lost).

---

## Verified good

- Allowlist is deny-by-default: `default_allowed_tools: vec![]` (`config.rs:205`), `is_allowed` on an empty list is `false` (`tools.rs:71-75`); unknown mode strings parse to `Ask` (`tools.rs:17-23`, `config.rs:133-142`, `lanes.rs:102-105`); missing server config → not exposed (`mcp.rs:92-97`); `deny` beats `allow`, disabled server never exposed (`config.rs:119-131`). Tests `deny_by_default`, `exact_prefix_and_star`, `disallowed_tool_fails_closed` cover this.
- Executor order for MCP: allowlist → not-local/ui/session → exposure → per-tool `deny` → call (`tools.rs:125-145`); `defs_with_mcp` advertises only exposed ∩ allowed tools. PROGRESS "server.allow is enforced" is true.
- `..` traversal is rejected lexically including `a/b/../../x` (`tools.rs:351-365`).
- `max_steps` (32) enforced per attempt (`handler.rs:303-307`); shell timeout clamped to ≤120 s; MCP timeout ≥1 s; `fs.read` 24 k chars, `fs.list` 12 k, shell 8 k caps.
- MCP handshake: `initialize` with `protocolVersion 2024-11-05` + `notifications/initialized` (`mcp.rs:181-193`); stderr → `null` so the pipe cannot fill; tool list cached per live server; `set_configs` drops servers that vanished/disabled.
- `ask_approver` selects on `cancel` — Stop during an approval card works (`handler.rs:495-499`).
- GUI approvals: UUID keys, single-use, removed after reply, unknown key → error, 120 s → Deny (`src-tauri/src/main.rs:56-72, 315-328`); CLI approver denies on anything but `y` and when piped.
- No `.await` while holding a `std::sync` guard: every `cfg.read()`/`configs.read()` guard is consumed in the same expression (`orchestrator.rs:116, 178`; `mcp.rs:64, 68`); `apply_config` awaits `set_configs` before taking the write lock (`:188-193`). No `std::sync::Mutex` on async paths.
- `unsafe_code = "forbid"` workspace-wide (`Cargo.toml`).
- Circuit breaker: reset per provider/all wired to Settings (`src-tauri:451`); the last slot is never skipped (`handler.rs:207`); success clears state; 3 unit tests cover trip/expire/reset.
- Failover tests assert a live `RouteTransition` and the persisted transcript event, plus strict-mode no-hop (`tests/failover.rs`).
- `store.fork` truly slices `events.into_iter().take(at_step)` into a fresh id with the source lane/model and Idle status (`store.rs:297-315`).
- Plugins v0.1 execute no code; pack names validated `[A-Za-z0-9_-]{1,64}` (`plugins.rs:128-141`); `git clone` args are fixed flags then an https-only URL (no `--upload-pack` injection), `GIT_TERMINAL_PROMPT=0`, run under `spawn_blocking` in Tauri (`src-tauri:892`); `install_skill_from_git` reports per-folder skips instead of failing the batch.
- Doctor output contains no secret values (presence/hint/path/count only).
- `desanitize_tool` (`types.rs:255-265`) maps `fs_read` back to `fs.read` by exact match first, so MCP tools with native underscores are not mangled.

---

## Tests to add (none of these exist today)

Existing: `tests/allowlist.rs` (parse/deny/`..`, 5), `tests/queue.rs` (park+pump via kill, reject mode, 2), `tests/failover.rs` (hop + strict, 2), `tests/teamwork.rs` (advertise/spawn/wait/send/read/list/queued-degrade, 6); unit: circuit breaker (3), `lane_policy_for` + `sticky_spec` (2), plugins round-trip/paste/discover/url (4). Missing PLAN P4/P5 acceptance: kill-mid-tool, fork-equality, MCP spawn→list→call→idle-kill. No budget test.

1. **Kill mid-tool** (P4 acceptance): provider emits `shell.exec {cmd:"timeout /t 30" | "sleep 30"}`; `kill()`; assert the child PID is gone within 1 s, no `ToolResult` appended after kill, status stays `Killed` after the task ends, JoinHandle finishes.
2. **Kill mid-stream**: `HangProvider` + `kill()`; assert the driver task terminates (currently leaks).
3. **Fork equality** (P4 acceptance): N events, `fork(id, Some(k))`; assert `events(fork) == events(src)[..k]`, new id, same project/lane/model, status Idle, zero usage; `fork(None)` equals full.
4. **Budget stop**: mock provider emitting `Usage` over the budget → run ends with Notice, no further `chat_stream` call, status Done.
5. **MCP spawn→list→call→idle-kill** (P5 acceptance): a tiny stdio server (Rust test bin or `node -e`) — list cached (one `tools/list` on the wire), call round-trip, idle reap after `idle_kill_secs` (use `tokio::time::pause`), notification-before-response skipped, timeout restarts the server, child killed on `stop()`/drop.
6. **Sandbox**: `C:\Windows\win.ini`, `/etc/hostname`, `\\?\C:\x`, `\foo`, `C:foo`, `//server/share`, and a junction inside the lane pointing outside — all rejected; `cwd=""` refused.
7. **Approver fail-closed**: `spawn_session`/`send_message`/`create_subsession` in a lane with `mode="ask"` and no approver must not run tools (counting approver / `DenyApprover` expectation).
8. **Handles pruned**: run a completing provider `max_concurrent + 1` times sequentially; none queues; `send_to` on a finished thread succeeds; rewrite `spawn_wait_degrades_to_queued_when_slots_full` to hold the slot with a hanging provider.
9. **Lane escape**: `session.spawn` with a lane outside the caller's project or a looser mode is rejected/clamped; `read_session` across projects requires approval.
10. **Lockdown**: lane `deny` + `tool_modes = {x = "auto"}` → tool does not run.
11. **Env scrubbing**: MCP server and `shell.exec` child do not see `*_API_KEY` from the parent env.
12. **Queued persistence**: session left `Queued` on disk is re-enqueued or marked Idle with a note on `recover()`; launch failure in the pump appends the error and leaves no `Queued` ghost.
13. **Transcript integrity**: `append` failure (read-only dir) turns into `RunEvent::Error` and `Killed`, never a silently continued run.
