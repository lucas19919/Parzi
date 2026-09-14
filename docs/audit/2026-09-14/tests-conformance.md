# Parzi audit — test coverage & spec conformance (read-only, 2026-09-14)

Scope: PLAN.md §4–§12, LOOP.md §4, PROGRESS.md, README.md vs. `crates/*`, `src-tauri/`, `ui/`.
Method: every `#[test]` body read; every PLAN bullet traced to code by file:line; orchestrator gate logs read from the scratchpad (not re-run).

## 0. Headline findings (read these first)

| # | Finding | Evidence |
|---|---|---|
| 1 | **The gate `cargo test --workspace` is RED today.** `plugins::tests::discover_and_install_library` panics with `PermissionDenied` at `crates/parzi-runtime/src/plugins.rs:784` (`delete_skill(n).unwrap()`). | `scratchpad/gate-test.log:141-166`, `gate-status.log` (`test exit=101`) |
| 2 | **15 of 80 tests never ran in the gate.** `cargo test` stops at the first failing target; the four runtime integration binaries (allowlist 5, failover 2, queue 2, teamwork 6) come after the failing lib target and are absent from the log. Actual gate tally: 64 passed / 1 failed / 15 not run. | `gate-test.log` has no `Running tests\allowlist.rs` etc. line |
| 3 | **PROGRESS.md's last verified claim is stale.** "Verified: workspace tests green … `npm run check` 0 errors" (PROGRESS.md:653-655, 2026-09-14) — today's gate: tests red, `svelte-check found 2 errors` (`gate-npm-check.log:74,81,653`; `gate-status.log: npm-check exit=1`). The newest entry "Footer update button" (PROGRESS.md:659-661) records no verification and introduced the 2 TS errors (`updateStore` `"idle"` vs `"available"/"checking"`). | gate logs |
| 4 | **Test-isolation rule (§11 "Store tests use tempdir") is violated across crates.** Session/plugin tests hit the real `~/.parzi` (`paths::parzi_dir()` = `dirs::home_dir()`, `crates/parzi-core/src/paths.rs:6-7`, no env override). That is the direct cause of #1: three plugin tests scan/delete the same live `~/.parzi/plugins` in parallel. | `logic.rs:163`, `failover.rs:77`, `queue.rs:47`, `teamwork.rs:49`, `plugins.rs:706` |
| 5 | **P4's headline acceptance ("kill mid-tool") has no test — and the code would fail one.** The stream loop `while let Some(ev) = rx.recv().await` (`handler.rs:337-373`) never selects on `cancel`; tool execution (`handler.rs:466`) has no cancel/timeout wrapper; `kill()` (`orchestrator.rs:463-474`) cancels the token and *drops* the `JoinHandle` (`orchestrator.rs:66-69`, detach, not abort). A killed run keeps streaming; if the turn ends without tool calls, `finish(Done)` at `handler.rs:396` overwrites `Killed`. `queue.rs:87-92` "proves" kill only by reading the status that `kill()` itself wrote. | handler.rs, orchestrator.rs |
| 6 | **IPC contract diverged 7×.** PLAN §9: "10 commands max". Code: 72 `#[tauri::command]` fns (`grep-rs-commands.txt`), all invoked from `ui/src/lib/api.ts`. PROGRESS.md:372 ("14 -> 11 commands") was already false when written. | `src-tauri/src/main.rs:1343-1418` |
| 7 | **No CI runs tests or clippy.** `.github/workflows/release.yml` is the only workflow (tag-triggered build+sign). §11 "clippy all+pedantic deny in CI" is unimplemented; pedantic is `warn` (`Cargo.toml:16-18`), ~370 advisory warnings outstanding (`gate-clippy-advisory-summary.log`). | `.github/` |
| 8 | **UI has zero automated tests.** No vitest/playwright/testing-library in `ui/package.json`; root `package.json` `"test": "exit 1"`. PROGRESS.md:432-434 cites "Playwright screenshots … with a mocked Tauri bridge" — that harness is not in the repo. | `ui/package.json` |
| 9 | **Several LOOP §4 acceptance tests were never written** (P1 round-trip config/theme, P2 mocked SSE, P2 catalog cache, P3 auth_status matrix, P4 kill/fork/budget, P5 spawn→list→call→idle-kill, P6 scripted CLI, P7 low-mem, P9 toml round-trip via UI commands, P9 diagnostics, P10 10k-line perf). Phases were marked `accepted` on `cargo check` alone (PROGRESS.md:12, 19, 24). | §B below |
| 10 | Minor bug found in passing: `orchestrator.rs:469` `self.mcp.stop(id)` passes the **session id** as an MCP **server name** — a no-op. | orchestrator.rs:469, mcp.rs:298-303 |

Test totals (static count): parzi-core 21 (9 unit + 12 `tests/logic.rs`), parzi-providers 35 (4 unit + 31 integration), parzi-runtime 24 (9 unit + 15 integration), parzi-cli 0, src-tauri 0, ui 0. **Total 80.**

---

## A. Test inventory

Legend: PLAN column = which §12/LOOP §4 acceptance the test actually serves ("none" = useful but not an acceptance item). ⚠ = name overclaims what the body asserts.

### crates/parzi-core

| File:line | Test | What it actually asserts | PLAN |
|---|---|---|---|
| src/config.rs:341 | `config::tests::migrate_folds_retired_provider_ids` | After `migrate()`: anthropic/claude-code→claude, ollama/t3 dropped, xai added, `auto_order` remapped, `keys_in_auto=false`. Parses from a TOML string; never calls `load()`/`save()`. | §2 migrate (partial) |
| src/config.rs:375 | `config::tests::default_roster_is_five_providers` | `Default` has 5 providers, default_provider "claude", auto_order fixed list. | none |
| src/theme.rs:757 | `theme::css_tests::fonts_are_quoted_and_generics_stay_bare` | `css_font_list` quoting/escaping. | none |
| src/theme.rs:768 | `theme::css_tests::accent_ink_flips_on_luminance` | `accent_ink` picks dark/light ink; junk → dark. | none |
| src/theme.rs:777 | `theme::css_tests::css_vars_carry_percent_twins_and_shadow_flag` | `to_css_vars()` contains specific `--parzi-*` strings; shadow flag flips. | P7 theme vars (partial) |
| src/theme.rs:793 | `theme::css_tests::numbers_print_without_float_noise` | `num()` formatting. | none |
| src/theme.rs:810 | `theme::palette_tests::vivid_art_yields_vivid_accent` | `extract_palette` of a solid red PNG → `#DC1E1E` (uses tempdir — the only tempdir user). | none |
| src/theme.rs:820 | `theme::palette_tests::monochrome_art_falls_back_to_indigo` | grey PNG → accent `#7C8CFF`. | none |
| src/theme.rs:830 | `theme::palette_tests::unreadable_file_errors` | missing file → Err. | none |
| tests/logic.rs:12 | `assembles_newest_first_under_budget` | 20 user msgs, budget 60: last kept message contains "message number 19"; `estimated_tokens ≤ 100`. Does not assert pinned system survives, nor `estimate()==chars/4`. | P1 counter (partial) |
| tests/logic.rs:34 | ⚠ `files_cap_and_truncate` | One 100k-char file, budget 100 → *some* `Role::System` message exists. Asserts neither the 8-file cap, the 12k cut, nor a truncation marker. | P1 @files (weak) |
| tests/logic.rs:49 | `compact_keeps_tail_and_folds_head` | 30 events, keep 20 → len 21 and `out[0]` is `Checkpoint`. Does not check the tail is the newest 20 or the digest content. | P10 compact (partial) |
| tests/logic.rs:57 | `widget_validation_fails_safe` | valid progress Ok; unknown type Err; version 2 Err; 60 table rows Err. | P8 widget schema ✓ |
| tests/logic.rs:70 | `diagram_caps_hold` | 201 nodes Err; 2 nodes + 1 edge Ok. | P8 diagram cap ✓ |
| tests/logic.rs:84 | `widget_markdown_validates_and_chart_caps_hold` | markdown Ok; blank Err; 201 points Err. | P8 ✓ |
| tests/logic.rs:95 | `artifact_validation_versions_and_dedups` | slugify, validate, bad kind/lang Err, `next_version`, `is_same_content`. | none (artifacts not in PLAN) |
| tests/logic.rs:125 | `legacy_tasks_migrate_to_lanes` | missing project → 0, no error. | none |
| tests/logic.rs:133 | `session_meta_parent_id_defaults_to_none_for_legacy_files` | serde default/skip of `parent_id`. | §2 meta.json schema (partial) |
| tests/logic.rs:161 | `subsession_hierarchy_lists_and_reparents` | **Real `~/.parzi/sessions`**: create/create_with_parent/list_children/set_parent + guards; cleans up. | none |
| tests/logic.rs:197 | `purge_cascade_kills_descendants_at_any_depth` | `cascade_kill_ids` pure logic. | none |
| tests/logic.rs:234 | `theme_emits_css_vars_with_asuka_default` | default image path; css contains accent + `--parzi-glass-blur:18px`. | P7 theme vars (partial) |

### crates/parzi-providers

| File:line | Test | What it actually asserts | PLAN |
|---|---|---|---|
| src/anthropic.rs:302 | `anthropic::tests::current_models_take_effort_not_budget` | `takes_budget(model)` truth table. | none |
| src/antigravity.rs:514 | `antigravity::tests::rate_limit_message_hops_with_exact_cooldown` | `fail_message(429, retry 45)` → retriable, cooldown 45. | none (failover) |
| src/antigravity.rs:521 | `antigravity::tests::overload_message_hops_without_header` | 503/529 → retriable. | none |
| src/antigravity.rs:529 | `antigravity::tests::auth_and_transport_failures_stay_put` | 401/400/transport → not retriable. | none |
| tests/antigravity_schema.rs:6 | `schema_keeps_allowlist_only` | `clean_schema` drops title/$ref/additionalProperties/default. | §3 antigravity port (P3 adapter unit) |
| tests/antigravity_schema.rs:28 | `const_becomes_enum` | `{"const":"x"}` → `{"enum":["x"]}`. | §3 |
| tests/antigravity_schema.rs:35 | `empty_object_gets_placeholder` | empty object gets `properties`. | §3 |
| tests/antigravity_schema.rs:42 | `oauth_headers_carry_version` | UA/x-goog/client-metadata present; UA contains `AG_VERSION`. | none |
| tests/antigravity_schema.rs:55 | `auth_url_is_google_oauth` | URL prefix + required params present. | none |
| tests/antigravity_schema.rs:73 | `catalog_capabilities_sane` | ids/names non-empty; opus flags; legacy < half; one default each for claude/codex. | none |
| tests/antigravity_schema.rs:100 | `catalog_limits_match_spec` | specific context limits / default flag. | none |
| tests/antigravity_schema.rs:115 | `effort_variants_map` | `with_effort` mapping table. | none |
| tests/catalog_sort.rs:25 | `sort_models_default_first_alpha_legacy_last` | ordering. | none |
| tests/catalog_sort.rs:46 | `catalog_lookup_accepts_aliases_and_rejects_retired` | alias lookups; retired ids empty. | none |
| tests/catalog_sort.rs:57 | `every_catalog_has_exactly_one_default` | one `is_default` per roster provider. | none |
| tests/catalog_sort.rs:65 | `claude_ids_are_bare_aliases` | id prefix, no date suffix. | none |
| tests/opencode.rs:13 | `catalog_is_the_real_zen_go_roster` | ≥35 models, no placeholder, kimi-k3 default + limits, prices, free tier 0, vision flags. | none |
| tests/opencode.rs:45 | `every_wire_kind_is_covered` | `route_for(id)` → (base, kind) table. | none |
| tests/opencode.rs:87 | `legacy_placeholder_resolves_to_flagship` | `opencode-default`→`kimi-k3`. | none |
| tests/opencode.rs:93 | `effort_picks_are_real_catalog_ids` | picks exist in catalog; hint text. | none |
| tests/opencode.rs:110 | `quota_errors_fail_over_but_auth_does_not` | retriable/cooldown classification strings. | none |
| tests/opencode.rs:134 | `tool_names_sanitize_round_trip` | `fs.read`↔`fs_read`. | none |
| tests/opencode.rs:160 | ⚠ `exact_key_resolution_prefers_env_without_logging` | sets `OPENCODE_API_KEY`, `resolve_key()` returns it. **Asserts nothing about logging.** | P3 auth (weak) |
| tests/router.rs:15 | `auto_order_normalises_aliases_and_fills_roster` | alias fold + roster fill. | none |
| tests/router.rs:31 | `auto_chain_never_spends_keys_by_default` | sets `XAI_API_KEY`/`ANTHROPIC_API_KEY` (**never unset**); all chain reasons "subscription"; xai absent. | P3 status matrix (indirect) |
| tests/router.rs:46 | `auto_chain_takes_keys_when_opted_in` | xai joins with reason "api key (opt-in)", after subscriptions. | P3 (indirect) |
| tests/router.rs:68 | `auto_chain_picks_effort_models` | sets `ANTIGRAVITY_ACCESS_TOKEN`; low/high variant ids; `pick(claude,…)`. | none |
| tests/router.rs:82 | `tier_fallback_chain_preserves_explicit_pick_first` | slot0 explicit; rest "subscription fallback". | none |
| tests/router.rs:97 | `tier_fallback_chain_maps_legacy_ids` | claude-code→claude; no repeat. | none |
| tests/router.rs:105 | `roster_and_billing_classes` | `PROVIDERS` const; `Billing::as_str`; `canonical_id`; `key_entry`. | none |
| tests/router.rs:120 | `retriable_classification` | string classification. | none |
| tests/router.rs:131 | `effort_options_are_provider_aware` | options per provider; `thinking_budget` table. | none |
| tests/router.rs:159 | `cooldown_parser_extracts_retry_windows` | `parse_cooldown_secs` table. | none |
| tests/router.rs:177 | `families_collapse_variants` | antigravity family grouping. | none |
| tests/router.rs:194 | `every_pick_exists_in_its_catalog` | `pick()` ids exist. | none |

Note: router.rs and opencode.rs mutate process env without restoring; tests inside one binary run in parallel, so e.g. `tier_fallback_chain_maps_legacy_ids` (router.rs:97) can observe `XAI_API_KEY` from router.rs:32/48/83. Passing today is order-luck.

### crates/parzi-runtime

| File:line | Test | What it actually asserts | PLAN |
|---|---|---|---|
| src/orchestrator.rs:998 | `orchestrator::tests::lane_policy_equips_session_harness_tools` | default lane allowlist contains 4 `session.*` names. (Does not check `ui.*`.) | none |
| src/orchestrator.rs:1012 | `orchestrator::tests::sticky_auto_holds_the_resolved_route` | `sticky_spec` table. | none |
| src/circuit_breaker.rs:136 | `circuit_breaker::tests::rate_limit_trips_and_expires` | trip/clear on success. | none |
| src/circuit_breaker.rs:150 | `circuit_breaker::tests::repeated_failures_trip_cooldown` | 3 failures trip. | none |
| src/circuit_breaker.rs:160 | `circuit_breaker::tests::reset_scopes_to_provider_or_all` | reset scoping. | none |
| src/plugins.rs:704 | `plugins::tests::skill_pack_round_trip` | **Real `~/.parzi/plugins`**: create pack, save/read commands, bad name Err, cleanup. | §7 commands pack (partial) |
| src/plugins.rs:733 | `plugins::tests::paste_skill_install` | TOML paste + SKILL.md paste install; bad input Err. Real home. | none |
| src/plugins.rs:755 | `plugins::tests::discover_and_install_library` | temp lib dir → 2 skills discovered, installed into real home, re-install Errs, deleted. **FAILS in gate** (`:784` PermissionDenied — sibling tests `scan()`/delete the same live dir). | none |
| src/plugins.rs:790 | `plugins::tests::git_url_normalize` | URL normalisation. | none |
| tests/allowlist.rs:18 | `deny_by_default` | empty allowlist denies `fs.read`/anything. | P5 allowlist-deny ✓ |
| tests/allowlist.rs:25 | `exact_prefix_and_star` | exact / `x.*` / `*` patterns. | P5 ✓ |
| tests/allowlist.rs:35 | `mode_parses` | `ApprovalMode::parse` incl. bogus→Ask. | §5 approvals (partial) |
| tests/allowlist.rs:43 | `disallowed_tool_fails_closed` | `execute()` returns ok=false when not allowed. | P5 ✓ |
| tests/allowlist.rs:52 | `path_escape_rejected` | `../../secret` → ok=false. | §5 sandbox |
| tests/failover.rs:73 | `explicit_pick_429_hops_with_route_transition` | Real store; Flaky(429)→Good; live `RouteTransition` + persisted event; `Done`. | closest to §6 "retry" (router failover, not user retry) |
| tests/failover.rs:127 | `strict_mode_single_slot_halts_on_429` | `auto_failover=false` → no hop. | none |
| tests/queue.rs:72 | ⚠ `busy_spawn_queues_then_pump_starts_it` | max=1: first Active, second Queued, `kill(first)` → second Active. **Only store status is asserted; the HangProvider run task is never observed to stop** (it is parked in `rx.recv()` with a forgotten sender, `handler.rs:337`). | §6 limits (partial); not a kill test |
| tests/queue.rs:105 | `queue_reject_mode_errors_loudly` | `queue_when_busy=false` → Err contains "busy". | §6 max_concurrent (partial; limit=1) |
| tests/teamwork.rs:63 | `session_tools_advertised_and_recognized` | `session_defs` names; executor `defs()` include `session.spawn`. | none |
| tests/teamwork.rs:86 | `spawn_subsession_nests_but_full_session_stays_top_level` | parent_id set/unset; model inherited. | none |
| tests/teamwork.rs:119 | `spawn_wait_collects_child_reply` | wait=true returns child text. | none |
| tests/teamwork.rs:143 | `send_message_continues_target_and_can_wait` | reply + User event in target transcript. | none |
| tests/teamwork.rs:172 | `read_and_list_inspect_sessions` | read/list JSON contains ids. | none |
| tests/teamwork.rs:199 | `spawn_wait_degrades_to_queued_when_slots_full` | max=1 → `status:"queued"`. | none |

### crates/parzi-cli, src-tauri, ui — **0 tests each.**

---

## B. Acceptance matrix (PLAN §12 / LOOP §4)

Status: PROVEN (a test asserts it) / CLAIMED (PROGRESS says accepted, no test) / PARTIAL / MISSING.

| Phase | Acceptance | Status | Evidence |
|---|---|---|---|
| P1 | round-trip config (save→load→equal) | **MISSING** | `ParziConfig::save` (`config.rs:244-246`) / `load` (`:232`) exist; no test calls either. `config.rs:341` parses a string only. PROGRESS.md:12 accepted on `cargo check`. |
| P1 | round-trip theme | **MISSING** | `Theme::save/load` (`theme.rs:197-199`, `:187`); tests only cover `to_css_vars`/helpers (`theme.rs:757-799`). |
| P1 | counter test | PARTIAL | `logic.rs:12` bounds `estimated_tokens ≤ 100`; no direct assertion that `estimate()` = `chars/4` (`context.rs:41-43`). |
| P2 | mocked SSE stream test | **MISSING** | No test feeds SSE bytes through `openai_compat.rs:232` / `anthropic.rs:223` / `codex.rs:228` / `opencode_wire.rs`. `failover.rs`/`teamwork.rs` stub the `Provider` trait and bypass SSE entirely. PROGRESS.md:19 evidence = `cargo check`. |
| P2 | catalog cache test | **MISSING** | `cached_models`/`store_models`/TTL (`catalog.rs:507-553`) untested. `catalog_sort.rs` tests static lists. PROGRESS.md:69 "24h TTL" is a claim. |
| P3 | `auth_status` matrix (no live creds) | PARTIAL | No test constructs adapters and asserts `Ok/Missing/Expired` (`anthropic.rs:153-166`, `codex.rs:284-291`, `antigravity.rs:328-336`, `opencode.rs:297-304`, `openai_compat.rs:160-165`). `router.rs:31-79` exercises it indirectly via env vars; `Expired` never exercised. P3 human gate (creds read-only) never recorded in PROGRESS. |
| P4 | kill-mid-tool test | **MISSING** (and code would fail it — see §0 #5) | `handler.rs:337-373` no cancel select; `:466` no cancel/timeout; `orchestrator.rs:66-69` JoinHandle detached; `:396` overwrites Killed with Done. PROGRESS.md:24 "kill/fork paths exercised via CLI". |
| P4 | fork-equality test | **MISSING** | `store.fork` (`store.rs:297-312`) and `orch.fork` (`orchestrator.rs:476-478`) have no test. |
| P4 | budget-stop test | **MISSING** | `max_steps` stop at `handler.rs:303-307` untested; token budget stop does not exist (see §C). |
| P5 | spawn→list→call→idle-kill test | **MISSING** | `McpManager` (`mcp.rs:144-303`) has no test that spawns a process. `allowlist.rs` constructs it with an empty map only. |
| P5 | allowlist-deny test | **PROVEN** | `allowlist.rs:18-22`, `:43-49`. |
| P6 | scripted `list/show/export/send/fork/kill/doctor` run | CLAIMED | parzi-cli has 0 tests. PROGRESS.md:38-40 records a manual `init`+`doctor` run only. |
| P7 | low-mem toggle | **MISSING** | No `--low-mem`/`low_mem` anywhere in crates, src-tauri, ui (grep empty). |
| P7 | screenshot parity (human gate) | OPEN | PROGRESS.md:42, 271. |
| P8 | widget schema test | **PROVEN** | `logic.rs:57-67`, `:84-92`. |
| P8 | invalid-JSON-falls-back test | PARTIAL | Core: bad type/version → Err (`logic.rs:57`). UI fallback to md/code block (`md.ts:117-124`) untested (no UI runner). Handler returns an error string to the model (`handler.rs:602`), not a code block. |
| P8 | diagram cap test | **PROVEN** | `logic.rs:70-81` (`MAX_NODES=200`, `widgets.rs:20`). |
| P9 | toml round-trip via UI commands | **MISSING** | `save_theme/get_theme/save_config/get_config` (`src-tauri/src/main.rs`) untested; src-tauri has 0 tests. |
| P9 | diagnostics copy test | CLAIMED | `SystemSection.svelte:101-141` exists; no test. |
| P10 | compact test | PARTIAL | `logic.rs:49-54` asserts length + first-is-Checkpoint only. |
| P10 | 10k-line perf | **MISSING** | No perf test; `Thread.svelte` renders every item in one `{#each}` (no virtualization). |
| P10 | release size check | CLAIMED | PROGRESS.md:61 "parzi.exe = 4.9MB", :577 installer 6.8MB; no script/CI check; profile set (`Cargo.toml:20-24`). |
| P10 | full `cargo test`, full clippy green | **RED today** | §0 #1-#2; clippy deny-gate clean (`gate-status.log: clippy exit=0`). |

---

## C. Feature conformance (PLAN §4–§11)

Status: DONE-VERIFIED (test) / DONE-UNVERIFIED (code present, no test) / PARTIAL / MISSING / DIVERGED.

### §4 Context window

| Item | Status | Where |
|---|---|---|
| `budget = context_limit − 20% − 10%` | **DIVERGED** | `handler.rs:431-432` hard-codes `limit = 100_000` with a `// TODO` (also `grep-todos.txt`). No model lookup, no reserves. |
| pinned never truncated | PARTIAL | System parts always joined into `system` (`context.rs:47`) — never cut. But the *latest user message* is not pinned: history walk breaks at `context.rs:96-98` before pushing if it alone exceeds budget. `logic.rs:12` doesn't assert this. |
| fill newest-first | DONE-VERIFIED | `context.rs:63-103`; `logic.rs:12`. |
| @files max 8, 12k each, `〈…〉` on cut | PARTIAL | Caps enforced at call sites only: CLI `main.rs:251-262`, Tauri `main.rs:286-305`, Omnibar `:638`. Core `ContextBuilder` has no per-file cap; marker is `<file truncated: budget exceeded>` (`context.rs:53`), not `〈…〉`, and nothing marks a 12k cut. |
| count = chars/4 | DONE-VERIFIED (weakly) | `context.rs:41-43`; `logic.rs:30`. |
| 80% auto-compact → checkpoint | **DIVERGED** | Trigger is `events.len() > 96` (`handler.rs:426`), an event count, not 80% of token budget. |
| keep last 20 raw | DONE-VERIFIED | `handler.rs:427`, `context.rs:123-149`, `logic.rs:49`. |
| bar meter amber at 80% | not audited (UI) | — |

### §5 Agent handler

| Item | Status | Where |
|---|---|---|
| `RunState {Idle,Streaming,AwaitingApproval,ExecutingTool,Done,Killed}` | **DIVERGED** | No such enum. State is `SessionStatus {Active,Idle,Done,Killed,Queued}` (`store.rs:8-18`) + private `RunEnd {Done,Retryable}` (`handler.rs:52-56`). Streaming/AwaitingApproval/ExecutingTool are not observable states. |
| `max_steps(32)` | DONE-UNVERIFIED | default `config.rs:162-164`, enforced `handler.rs:303-307`; no test. |
| 30s tool timeout | PARTIAL | No handler-level wrapper (`handler.rs:465-467`). Per-tool: `shell.exec` default 30s, cap 120s (`tools.rs:428-432`); MCP per-server `timeout_ms` default 30s (`config.rs:165-167`, `mcp.rs:131`); `fs.*` none. |
| tool results tagged untrusted | DONE-UNVERIFIED | `context.rs:82` `[result:{name} untrusted]`; system prompt `handler.rs:783`. No test. |
| allowlist check + ask → AwaitingApproval | DONE-VERIFIED (allowlist) / DONE-UNVERIFIED (ask) | `tools.rs:124-127`; approval `handler.rs:470-500`; GUI approver 120s→Deny `src-tauri/main.rs:68-73`. |
| every event persisted as it happens | DONE-UNVERIFIED | `store.rs:239-252` append; each append also rewrites meta.json + session.md (3 writes/event; perf note for 10k lines). |
| Stop button + `parzi kill` cancel **mid-tool** | **PARTIAL / defect** | See §0 #5. Cancel is honoured at loop top (`handler.rs:298`), between tool calls (`:401`) and inside approval wait (`:496`) — not mid-stream, not mid-tool. |
| stop on budget | MISSING | No token-budget stop; only `max_steps`. |

### §6 Orchestrator + viewer

| Item | Status | Where |
|---|---|---|
| `max_concurrent: 4` | DONE-VERIFIED (limit=1 in tests) | `config.rs:171-173`; `orchestrator.rs:256`; `queue.rs:105`. |
| spawn / focus / fork(at_step) / kill / retry | PARTIAL | spawn `orchestrator.rs:226`; fork `:476` + `store.rs:297-312` (at_step honoured); kill `:463`; **retry: MISSING** (no `retry` fn; only internal failover `RunEnd::Retryable`); focus is UI-only. |
| viewer table `● │ lane/thread │ model logo+name │ tokens/$ │ last tool │ elapsed` | PARTIAL | `AgentVisualizer.svelte:200-211` shows tokens·cost + Focus/Fork/Kill; `RunInfo` (`orchestrator.rs:55-64`) carries no last-tool/elapsed; UI derives last tool from events (PROGRESS.md:427); elapsed not found. |
| rebuild from meta.json on boot | DONE-UNVERIFIED | `store.rs:176-191` reads only meta.json; `recover()` `orchestrator.rs:197-206`. |
| CLI mirrors list/show/fork/kill | DONE-UNVERIFIED | `cli/main.rs:21-100`. |

### §7 MCP + tools + plugins

| Item | Status | Where |
|---|---|---|
| `rmcp` 3.2.0 client | DIVERGED (documented) | hand-rolled NDJSON JSON-RPC, `mcp.rs:1-5`; PROGRESS.md:29-30. Streamable HTTP: MISSING. |
| lazy spawn | DONE-UNVERIFIED | `mcp.rs:144-196`. |
| kill after 60s idle | PARTIAL | `reap_idle` runs only on the next `list_tools`/`call_tool` (`mcp.rs:199-210`, `:222`, `:270`); no timer → an idle server lives until the next MCP op, possibly forever. Default 60 (`config.rs:174-176`), floor 10s (`mcp.rs:59`). |
| cached `list_tools` | DONE-UNVERIFIED | `mcp.rs:226-228`, `:245`. |
| lane allowlist deny-default | DONE-VERIFIED | `tools.rs:71-75`; `allowlist.rs`. |
| plugins: commands / theme / mcp-pack | PARTIAL | manifest accepts 3 kinds (`plugins.rs:74-79`, `:562`); `commands` and `theme` are consumed (`:97`, `:296`); **`mcp-pack` is never loaded into MCP config** — only listed in `SkillsSection.svelte:47`. |
| `ui.show_markdown/widget/diagram` tools, "also over MCP" | PARTIAL | tools `tools.rs:205-268`, handled `handler.rs:578-620` (+`ui.show_artifact`); not exposed over MCP. |
| schema-checked in core, fail-safe to code block | PARTIAL | core check ✓ (`widgets.rs`); on failure the handler returns an error string to the model (`handler.rs:591,602,612`), UI falls back only for fenced text (`md.ts:122-124`). |

### §8 Markdown + widgets

| Item | Status | Where |
|---|---|---|
| markdown-it GFM | DONE-UNVERIFIED | `md.ts:5-20` (`html:false`, linkify); task lists via custom rule `:87-95`. |
| DOMPurify **strict**, `parzi://` images only | **DIVERGED** | `md.ts:97-101` uses default DOMPurify config + `ALLOWED_URI_REGEXP` that allows `https?:` **and** `parzi:` and relative URLs → remote `https` images render. Not "parzi:// only". |
| shiki, 12 langs | **DIVERGED** (documented) | full `highlight.js` (`md.ts:3`); PROGRESS.md:55 KNOWN 1.1MB → now 1,535 kB chunk (`gate-npm-build.log`). |
| 60ms stream debounce | MISSING | The only 60ms timer is a settings-anchor scroll retry (`App.svelte:73`). Token events re-render per chunk. |
| unclosed-fence guard | MISSING | nothing in `md.ts`; relies on markdown-it default (fence runs to EOF). |
| virtualize >500 blocks | MISSING | `Thread.svelte` plain `{#each chronologicalItems}`. |
| copy buttons | DONE-UNVERIFIED | `md.ts:80` `data-copy`; `Thread.svelte:61-67`. |
| diff red/green | DONE-UNVERIFIED | `md.ts:44-58`. |
| 7 widget types | DONE-VERIFIED (8) | `widgets.rs:8-17` adds `markdown`; `logic.rs`. |
| diagram ≤200 nodes | DONE-VERIFIED | `widgets.rs:20`, `logic.rs:70`. |
| lane SYSTEM docs 10-line usage example | PARTIAL | usage lives in the global system prompt (`handler.rs:770-785`) and tool descriptions, not lane SYSTEM.md. |

### §9 UI contract

| Item | Status | Where |
|---|---|---|
| ONE window, transparent, frameless | DONE-UNVERIFIED | `tauri.conf.json:19-31`. |
| `--low-mem` solid bg | **MISSING** | grep empty. |
| glass bar only (blur 18px) | not audited | `theme.rs` emits `--parzi-glass-blur:18px` (`logic.rs:239`). |
| sidebar search | DONE-UNVERIFIED | `Sidebar.svelte:64,111`. |
| Palette Ctrl+K, Ctrl+N, Esc | DONE-UNVERIFIED | `App.svelte:781-788`. |
| 5 settings pages | DIVERGED (7) | `SettingsNav.svelte:26-32`: general/appearance/models/connectors/skills/context/system. |
| IPC ≤10 commands | **DIVERGED (72)** | `src-tauri/src/main.rs:1343-1418`; `grep-rs-commands.txt`. |
| events token/done/widget/approval/status | DONE-UNVERIFIED | `main.rs:67` `parzi://run-event`. |

### §10 CLI + doctor + updates

| Item | Status | Where |
|---|---|---|
| `list/show/export/send/fork/kill/doctor` | DONE-UNVERIFIED | `cli/main.rs:21-100` (+init/models/health/reset-cooldowns/login/logout). `export --md` flag absent: `export` = `show` (`:216-218`). |
| doctor: config schema / key presence / provider ping / MCP spawn / disk+log perms / webview | PARTIAL | config `doctor.rs:70`, auth presence `:84-105`, MCP `:135-156`, webview `:158-171`. **No provider ping** (auth_status only), **no disk/log perm check** (only `ensure_dirs` `:63`). |
| same fn used by GUI About | DONE-UNVERIFIED | `run_doctor*` commands. |
| signed updater, stable channel | DONE-UNVERIFIED | `tauri.conf.json:72-80`, single endpoint; keys per PROGRESS.md:272-276; feed not yet public (PROGRESS.md:577). |
| logs `~/.parzi/logs/`, rotation | **MISSING** | `logs_dir()` exists (`paths.rs:36-37`) and is created, but nothing writes there; CLI logs to stderr at WARN (`cli/main.rs:133`); no file appender, no rotation, no subscriber in src-tauri. |

### §11 Quality bars

| Bar | Status | Where |
|---|---|---|
| `#![deny(unsafe_code)]` | PARTIAL | workspace `unsafe_code = "forbid"` (`Cargo.toml:13-14`) via `[lints] workspace = true` in the 4 members. **src-tauri is excluded from the workspace and has no `[lints]`** (`src-tauri/Cargo.toml`) → GUI crate has no forbid. No file-level attributes anywhere (only `windows_subsystem` at `src-tauri/src/main.rs:4`). |
| clippy all+pedantic **deny in CI** | **MISSING** | no CI test/clippy job; pedantic is `warn` (`Cargo.toml:16-18`); LOOP §1 downgraded to advisory; ~370 advisory warnings, 7 fns >100 lines (`gate-clippy-advisory-summary.log`). |
| rustfmt | not gated | no CI. |
| no `unwrap` outside tests | PARTIAL | violations in §E. |
| timeouts on all I/O | PARTIAL | HTTP clients 120–180s (`openai_compat.rs:27`, `anthropic.rs:72`, `codex.rs:173`, `antigravity.rs:378`, `opencode.rs:179`); MCP per request (`mcp.rs:131`); shell (`tools.rs:446`); `fs.*` none; tool execution in handler none; approval 120s GUI only. |
| secrets never logged | DONE-UNVERIFIED | no key/token in `tracing!`/`println!` greps; doctor prints presence only (`doctor.rs:2`); CLI approver echoes tool args to stderr (`cli/main.rs:111`) — could include secrets an agent puts in args. The one test named "…without_logging" checks nothing about logging. |
| files <400 lines | **VIOLATED ×18** | §E. |
| provider adapters mocked | PARTIAL | trait-level fakes only; no wire-level mocks. |
| store tests use tempdir | **VIOLATED** | §0 #4. |
| budgets (CLI <25MB, GUI <150MB, cold start <1.5s, p50 <100ms, 10k lines) | **UNMEASURED** | nothing in repo measures any of them. |
| release `opt-level="z"`, lto, strip | DONE | `Cargo.toml:20-24`. |

---

## D. Unverified / human-gated claims in PROGRESS.md (deduplicated)

| Date | Line(s) | Claim / admission | State today |
|---|---|---|---|
| 09-09 | 12 | P1 accepted with `cargo check` only (LOOP §4 wants round-trip test) | still no test |
| 09-09 | 19 | P2+P3 accepted on `cargo check` + 4 schema tests (no mocked SSE, no auth matrix) | still no test |
| 09-09 | 24 | P4 "compiles; kill/fork paths exercised via CLI (P6)" | no test; defect §0 #5 |
| 09-09 | 38-40 | P6 evidence = manual `init`+`doctor` run | no scripted session |
| 09-09 | 42, 271 | P7 PENDING HUMAN GATE (screenshot parity) | open |
| — | (absent) | P3 human gate "confirm read-only creds" (LOOP §0/§4) | never recorded |
| 09-09 | 47 | DEVIATION 14 commands vs 10 cap | now 72 |
| 09-09 | 55 | KNOWN full highlight.js bundle 1.1MB | now 1.5MB chunk |
| 09-09 | 94 | "verify drag is smooth now" (asks human) | unrecorded |
| 09-09 | 214-215 | Google sign-in "still needs YOUR live click" | unrecorded |
| 09-09 | 279, 312, 331 | Antigravity live-token test needed (`parzi login → doctor → send`) | 09-14 line 652: "antigravity tool-schema 400 on this box is pre-existing and untouched" → tool calls on antigravity known broken |
| 09-09 | 331 | t3 live-token test | provider retired 09-13 (line 480-482); moot |
| 09-09 | 330-331 | "PR/Automation rows + worktree pill (cut)" | cut |
| 09-12 | 372 | "IPC: 14 -> 11 commands" | false (72) |
| 09-13 | 432-434 | "Playwright screenshots … mocked Tauri bridge" | harness not in repo |
| 09-13 | 494 | Codex ChatGPT-backend path "not yet exercised live" | open |
| 09-13 | 524-525 | "Not verified live: Codex-subscription completion … Claude OAuth completion with new effort body" | open |
| 09-14 | 272-276 | P9 updater keys done; "Still manual: … publish the draft release" | feed not served (line 577: drafts don't serve latest.json; repo private) |
| 09-14 | 277-278 | Windows SmartScreen / Authenticode | open |
| 09-14 | 537 | parallel lane hunk rode along; "1 unused-parens warning is theirs" | — |
| 09-14 | 559-563 | OPEN: src-tauri `cargo check` fails on icon.ico | resolved (line 580-583; `gate-tauri-check` exit 0) |
| 09-14 | 570 | local MSI installs per-machine (Error 1925 without admin) | friend build = NSIS only |
| 09-14 | 602 | lane WIP main.rs still contains navigate hunk | unresolved note |
| 09-14 | 607 | installer "interactive click-through still untested" | open |
| 09-14 | 612 | Deviation: dark theme inlined; Component 4 SKIPPED | — |
| 09-14 | 648-649 | DEVIATION: replaces PLAN §3 `opencode serve` line; Zen edge sheds intermittently | — |
| 09-14 | 651-652 | OPEN: per-model `reasoningEffort` not sent; Go $ caps not surfaced | open |
| 09-14 | 653-655 | "Verified: workspace tests green … npm run check 0 errors" | **contradicted by today's gate** |
| 09-14 | 659-661 | Footer update button — no verification line; co-mingled with lane WIP | introduced the 2 svelte-check errors |
| 09-09 | 29-30 | DEVIATION rmcp → hand-rolled client | documented |

Also observed at audit time: `src-tauri/tauri.conf.json:4` reads `0.1.7` while `Cargo.toml`, `src-tauri/Cargo.toml`, `ui/package.json` read `0.1.6` (`gate-repo-facts.log` showed 0.1.6 everywhere at 20:09 — a parallel lane bumped it mid-audit). Root `package.json` says `1.0.0` / `ISC` (repo is MIT).

---

## E. Quality-bar violations

### E1. Source files > 400 lines (PLAN §11 "Files <400 lines")

| Lines | File |
|---|---|
| 1419 | src-tauri/src/main.rs |
| 1339 | ui/src/App.svelte |
| 1128 | ui/src/lib/Omnibar.svelte |
| 1030 | crates/parzi-runtime/src/orchestrator.rs |
| 975 | ui/src/lib/settings/ConnectorsSection.svelte |
| 834 | crates/parzi-core/src/theme.rs |
| 806 | crates/parzi-runtime/src/handler.rs |
| 802 | crates/parzi-runtime/src/plugins.rs |
| 633 | ui/src/lib/Sidebar.svelte |
| 581 | ui/src/lib/settings/AppearanceSection.svelte |
| 566 | ui/src/lib/settings/ModelsSection.svelte |
| 556 | crates/parzi-providers/src/catalog.rs |
| 538 | crates/parzi-providers/src/antigravity.rs |
| 525 | ui/src/lib/Thread.svelte |
| 515 | crates/parzi-core/src/store.rs |
| 496 | crates/parzi-cli/src/main.rs |
| 466 | crates/parzi-runtime/src/tools.rs |
| 430 | ui/src/theme.css |

18 files (9 Rust, 9 UI). Borderline: `opencode_wire.rs` 395, `config.rs` 384.

### E2. `.unwrap()` / `.expect(` / `unreachable!` outside test code

| File:line | Call | Note |
|---|---|---|
| crates/parzi-providers/src/antigravity_oauth.rs:44 | `.parse().unwrap()` | header value from constant; infallible in practice, still violates the bar |
| crates/parzi-providers/src/antigravity_oauth.rs:45 | `.parse().unwrap()` | same |
| crates/parzi-providers/src/antigravity_oauth.rs:50 | `.parse().unwrap()` | same |
| crates/parzi-runtime/src/mcp.rs:224 | `.expect("live after ensure")` | logically guarded by `ensure_live` |
| crates/parzi-runtime/src/mcp.rs:272 | `.expect("live after ensure")` | same |
| src-tauri/src/main.rs:1325 | `boot().expect("parzi home")` | in `main()`; a missing home panics the GUI instead of a dialog |
| src-tauri/src/main.rs:1418 | `.expect("parzi failed to start")` | Tauri run |
| crates/parzi-providers/src/lib.rs:87 | `unreachable!(...)` | in `provider()` match after `canonical_id` |

Per-file count (non-test): antigravity_oauth.rs 3, mcp.rs 2, src-tauri/main.rs 2, providers/lib.rs 1 (`unreachable!`). All other hits in `grep-unwrap.txt` (config.rs:357, theme.rs:806-831, plugins.rs:706-796) are inside `#[cfg(test)]` modules — allowed.

### E3. Crate-level lint attributes

- No `#![deny(...)]` / `#![forbid(...)]` in any `.rs` file. Only `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` at `src-tauri/src/main.rs:4`.
- `unsafe_code = "forbid"` and `clippy all/pedantic = "warn"` live in `Cargo.toml:12-18` `[workspace.lints]`, inherited by the 4 workspace crates via `[lints] workspace = true`. `src-tauri/Cargo.toml` (excluded from workspace) inherits nothing.

### E4. Other gate signals (from logs)

- clippy deny-set (correctness/suspicious/complexity/perf): clean, both workspace and src-tauri.
- rustc warnings: `handler.rs:336` unused `mut`; `orchestrator.rs:323` `slots_for` and `:554` `lane_policy` dead; `teamwork.rs:12` unused import.
- svelte-check: 2 errors, 10 warnings (`gate-npm-check.log:653`).
- Vite: main chunk 1,535 kB (`gate-npm-build.log`).
- `cargo audit` / `cargo deny` not installed (`gate-repo-facts.log`); `npm audit --omit=dev`: 0 vulnerabilities.
- Test hygiene: env mutation without restore (`router.rs:32-33,48,69,83`; `opencode.rs:161-163`); real-home writes (§0 #4); `Box::leak` in `failover.rs:67` (test-only, acceptable).

---

## F. Tests to add — risk-ranked top 15

Effort: S ≤ 1h, M ≤ half day, L ≥ day. Every Rust test below should first land **T0**, otherwise they will collide in `~/.parzi` like today's failure.

| # | Crate · name | Setup | Asserts | Why it matters | Effort |
|---|---|---|---|---|---|
| T0 | parzi-core · `paths::PARZI_HOME` override + `tests/common::TempHome` guard | Add `PARZI_HOME` env (or `paths::set_root_for_tests`) read in `paths::parzi_dir()` (`paths.rs:6-7`); a RAII guard that creates a tempdir and points the store/plugins/config at it; serialize env-touching tests with a `static Mutex`. | Every existing store/plugin test passes with `~/.parzi` absent/read-only. | Fixes the red gate (§0 #1), the §11 tempdir rule, and env leakage in router/opencode tests. Prerequisite for T1–T15. | M |
| T1 | parzi-runtime · `kill_cancels_mid_tool` (P4) | `FakeProvider` emits one `ToolCall{shell.exec}` (or a `SlowLocalTool` seam) whose execution blocks on a `Notify` for 10s; spawn via `Orchestrator`, wait for `RunEvent::ToolCall`, call `orch.kill(id)`. | Within 500ms: `RunEvent::Error("cancelled")` received, `store.get(id).status == Killed` and **stays** Killed after the tool's own timer would have fired; no `ToolResult`/`Done` afterwards. | Today this fails: `handler.rs:337` and `:466` ignore cancel; `:396` overwrites Killed. This is PLAN §5's stop guarantee. | M (needs `tokio::select!` on cancel in handler + `JoinHandle::abort` or cooperative cancel in `kill`) |
| T2 | parzi-runtime · `kill_cancels_hung_stream` | `HangProvider` (queue.rs pattern) + `kill`. | Handler task finishes (join the `JoinHandle` with a 1s timeout) — not just status. | Proves the leak in `queue.rs:72` is closed; prevents zombie tasks per killed run. | S after T1 |
| T3 | parzi-core · `fork_at_step_equals_prefix` (P4) | Session with 12 mixed events; `store.fork(id, Some(7))` and `fork(id, None)`. | Forked `events()` == source `events()[..7]` (serde-equal), title suffixed "(fork)", new id, model/project/lane copied; `fork(Some(99))` clamps to len. | P4 acceptance never written; `store.rs:297-312`. | S |
| T4 | parzi-runtime · `max_steps_stops_run` (P4 budget-stop) | Provider that always returns a `ToolCall{fs.list}`; `cfg.lanes.max_steps = 3`, `AutoApprover`. | Exactly 3 `ToolCall` events; final `RunEvent::Error` contains "stopped after 3 steps"; status `Done`. | Only guard against runaway loops (`handler.rs:303`). | S |
| T5 | parzi-core · `config_and_theme_round_trip` (P1) | With T0: `ParziConfig::default()` mutated (mcp server with allow/deny/tool_modes, favorites, routing) → `save()` → `load()`; same for `Theme` with floats like 0.6000000238. | Loaded == saved (derive `PartialEq` or compare `toml::to_string`); `theme.toml` has no float noise; `.tmp` file absent after save (`error.rs:29-37`); `version=1` present; wrong version → `check_version` Err. | LOOP §4 P1 acceptance; `save/load` have zero coverage. | S |
| T6 | parzi-providers · `sse_parsers_handle_split_frames` (P2 mocked SSE) | Extract the line-loop bodies at `openai_compat.rs:232`, `anthropic.rs:223`, `codex.rs:228`, `opencode_wire.rs` into `fn feed(&mut self, bytes) -> Vec<StreamEvent>`; feed fixture transcripts (text deltas, tool_call arg fragments, usage, `[DONE]`, co-located usage+delta chunk from PROGRESS.md:643) split at arbitrary byte boundaries. | Concatenated `Text` equals fixture; `ToolCall.args` parses; `Usage` totals; no event after `[DONE]`. | Every provider bug found "by live testing" in PROGRESS.md was a parser bug; nothing pins them. | M |
| T7 | parzi-providers · `auth_status_matrix` (P3) | With T0 + env guard: for each roster provider construct with (a) no creds, (b) env key, (c) credential file with future expiry, (d) credential file with past expiry (anthropic path `anthropic.rs:153-166`). | Exact `AuthStatus` variant and `Billing` per cell; hint strings non-empty; nothing read outside the temp home. | LOOP §4 P3 acceptance; `Expired` is never exercised today. | M |
| T8 | parzi-runtime · `mcp_spawn_list_call_idle_kill` (P5) | Tiny NDJSON JSON-RPC echo server as a test binary (`tests/bin/mock_mcp.rs`: answers `initialize`, `tools/list` with 1 tool, `tools/call` echo); `McpManager::new(cfg, idle=10)`. | `list_tools` returns 1 tool and a second call does not hit the child (count requests); `call_tool` returns echo; after `tokio::time::advance(11s)` + any op the child pid is gone; `set_configs` with `enabled=false` stops it; `is_tool_exposed` honours allow/deny. | `mcp.rs` has zero process-level coverage; the idle-kill design (`reap_idle` on next op only) needs a decision — test documents it. | M |
| T9 | parzi-core · `context_pins_latest_user_and_marks_file_cuts` (§4) | History with a 5k-char latest user msg, budget 1k; 9 files of 20k chars. | Latest user message present regardless of budget (currently fails, `context.rs:96-98`); ≤8 files; each snippet ≤12k with an explicit cut marker; system parts intact. Add `assert_eq!(estimate("abcd"),1)`. | Turns `files_cap_and_truncate` from a name into a proof; moves the 8/12k rule into core. | S |
| T10 | parzi-core · `compact_triggers_at_80pct_and_keeps_newest_20` (P10) | 100 events with known text; call the handler's assemble path (extract `fn history_for(events, limit)` from `handler.rs:423-439`). | Compaction fires when estimated tokens ≥ 0.8·limit (not at 96 events); tail == newest 20 by content; digest lists folded users/tools in order. | Aligns code with §4; today's test only checks length. | S |
| T11 | parzi-cli · `scripted_session` (P6) | `assert_cmd` + T0 `PARZI_HOME`; fake provider via a `parzi` feature flag or `PARZI_PROVIDER=stub` env that routes to a `GoodProvider`. | `init` creates tree; `send new "hi" --yes` prints text and creates a session; `list --json` shows it; `show`/`export` print transcript; `fork --at 1` creates a second; `kill` sets Killed; `doctor --json` exits 0 with `dirs/config/theme` ok. | P6 has 0 tests; this is the interop surface other harnesses depend on. | M |
| T12 | src-tauri · `ipc_toml_round_trip` (P9) | Move command bodies into plain fns (`fn save_theme_impl(theme) -> Result<String>`); unit test in src-tauri with T0. | `save_theme` → `get_theme` equal; `save_config` → `get_config` equal and `apply_config` pushed MCP configs; `save_user_css` >64KB rejected; `theme_css` ends with user.css. | src-tauri has 0 tests and 72 commands. | M |
| T13 | parzi-runtime · `allowlist_and_approval_interplay` | `ToolExecutor` + `DenyApprover`/`AutoApprover`; MCP `tool_modes` overrides. | `Ask` lane + Deny approver → tool denied and `ToolResult{ok:false}` persisted; `tool_modes: deny` beats lane `auto`; `tool_modes: auto` skips approver; `ui.*` never asks; `session.spawn` asks in Ask mode. | Security-relevant path `handler.rs:470-500` untested. | S |
| T14 | parzi-core · `store_append_is_crash_safe_and_10k_fast` (P10 perf) | Append 10,000 events; measure; then truncate `events.jsonl` mid-line and call `events()`. | 10k appends < N s (set from a baseline; today each append rewrites meta.json + session.md — `store.rs:239-252`); `list()` reads only meta; a torn last line is skipped/reported, not a panic. | §11 "10k-line thread" and §2 crash-safety. | M |
| T15 | parzi-runtime · `orchestrator_kill_does_not_misuse_mcp_stop` + `retry_relaunches_idle_session` | Spy `McpManager`; session in `Idle` after error. | `kill(id)` never calls `mcp.stop(session_id)` (`orchestrator.rs:469`); a `retry(id)` API re-launches with the last user prompt (implements §6 "retry"). | Fixes §0 #10; fills the missing §6 verb. | S |

### Minimum UI test harness

| Piece | What | Effort |
|---|---|---|
| vitest (jsdom) in `ui/` | `npm i -D vitest jsdom @testing-library/svelte`; `"test": "vitest run"` in `ui/package.json`. | S |
| `ui/src/lib/md.test.ts` | `renderMarkdown`: `<script>`/`onerror` stripped; `javascript:` href dropped; `![](https://…)` image **decision test** (PLAN says parzi-only — encode whichever policy is chosen); `parzi://` allowed; fenced diff gets `dl-add/dl-del`; >30 lines gets `clamped` + expand button; `splitSegments` invalid JSON falls back to md (P8 acceptance); unclosed fence renders without throwing. | S |
| `ui/src/lib/api.test.ts` | Mock `@tauri-apps/api/core.invoke`; for every `api.*` method assert the invoked command name and arg keys; add a snapshot of the command list and compare it to `grep -o` of `#[tauri::command]` fns (contract test: 72 today, fails on drift). | S |
| Smoke e2e | Playwright against `vite dev` with the mocked bridge PROGRESS.md:433 describes (check it in under `ui/e2e/`): boot → sidebar renders → send → live text appears → Ctrl+K opens palette → Esc closes. | M |

### CI job (`.github/workflows/ci.yml`)

```yaml
name: CI
on: [push, pull_request]
jobs:
  rust:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { components: clippy, rustfmt }
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all -- --check
      - run: cargo test --workspace --no-fail-fast
        env: { PARZI_HOME: ${{ runner.temp }}/parzi-home }
      - run: cargo clippy --workspace --all-targets -- -D clippy::correctness -D clippy::suspicious -D clippy::complexity -D clippy::perf -D warnings
      - run: cargo clippy --workspace --all-targets -- -W clippy::pedantic   # advisory, until count is 0 then flip to -D
      - run: cd src-tauri && cargo clippy --all-targets -- -D clippy::correctness -D clippy::suspicious -D clippy::complexity -D clippy::perf -D warnings
      - run: cd src-tauri && cargo test
  ui:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with: { node-version: 24, cache: npm, cache-dependency-path: ui/package-lock.json }
      - run: npm --prefix ui ci
      - run: npm --prefix ui run check
      - run: npm --prefix ui test
      - run: npm --prefix ui run build
      - run: node -e "const fs=require('fs');const s=fs.statSync('ui/dist/assets/'+fs.readdirSync('ui/dist/assets').find(f=>/^index-.*\.js$/.test(f))).size;if(s>1_200_000)process.exit(1)"  # bundle budget
  gates:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: |
          bad=$(find crates src-tauri/src ui/src -type f \( -name '*.rs' -o -name '*.ts' -o -name '*.svelte' \) -not -path '*/target/*' | xargs wc -l | awk '$1>400 && $2!="total"'); [ -z "$bad" ] || { echo "$bad"; exit 1; }
      - run: |
          hits=$(grep -rnE '\.unwrap\(\)|\.expect\(' crates src-tauri/src --include='*.rs' | grep -v '/tests/' | grep -vE 'config.rs:3[4-8][0-9]|theme.rs:8[0-3][0-9]|plugins.rs:7[0-9][0-9]'); [ -z "$hits" ] || { echo "$hits"; exit 1; }
      - run: cargo install cargo-audit --locked && cargo audit && (cd src-tauri && cargo audit)
```

Notes: `--no-fail-fast` is mandatory so a lib failure can no longer hide 15 integration tests; add `[lints] workspace = true` (or an explicit `[lints.rust] unsafe_code="forbid"`) to `src-tauri/Cargo.toml`; the file-length and unwrap gates above encode LOOP §1 REVIEW, which today is not automated anywhere. Once T0 lands, the `PARZI_HOME` env makes the whole Rust suite hermetic on CI.
