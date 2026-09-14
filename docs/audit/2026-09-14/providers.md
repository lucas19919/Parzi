# Audit: `parzi-providers` (model adapters, auth, catalog, router)

Snapshot: working tree read between **2026-09-14 20:16 and 20:24 +02:00**, read-only, static review (no cargo/npm run).
Another lane was editing this crate throughout (types/codex/anthropic/openai_compat/antigravity/catalog/router/opencode
and both test files changed between 20:14:59 and 20:22:16). Every finding below was re-checked against the last
version read; line numbers are from that read, codex.rs numbers after line 130 are approximate (≈).

Files read in full: Cargo.toml, lib.rs, types.rs, anthropic.rs, claude.rs, codex.rs, opencode.rs, opencode_wire.rs,
antigravity.rs, antigravity_oauth.rs, openai_compat.rs, compat_providers.rs, catalog.rs, router.rs,
tests/{antigravity_schema,catalog_sort,opencode,router}.rs; plus the runtime call sites (handler.rs failover loop,
orchestrator.rs slot builder + `effort_tokens`, doctor.rs, CLI/Tauri login + provider commands), workspace
Cargo.toml/Cargo.lock, PLAN §3/§11/§13, PROGRESS, THIRD_PARTY_NOTICES. Claude model ids/prices were checked
against the current Anthropic reference table.

Process note (was P0 for ~7 minutes): from 20:14:59 (`sanitize_tool`/`desanitize_tool` moved to `types.rs`) until
20:22:16, `opencode.rs:189` used `sanitize_tool` without importing it and `tests/opencode.rs:135` imported it from the
old module — the crate did not build. Fixed by the lane at 20:22. Any gate run in that window is a false negative;
re-run.

---

## Findings

### P1 — serious

**F1. "Extra"/"Ultra" (and "High" on small-output models) send `max_tokens` above the model's output limit; the 400 is terminal and the run dies.**
- Source: `crates/parzi-runtime/src/orchestrator.rs:20-28` `effort_tokens`: high=65_536, extra=131_072, ultra=262_144; wired at orchestrator.rs:641 → `ChatReq.max_tokens`.
- Passed through unclamped: `anthropic.rs:106` `"max_tokens": req.max_tokens`; `antigravity.rs:187` `"maxOutputTokens": req.max_tokens`; `openai_compat.rs:151`; `codex.rs:≈177` (API-key path); `opencode_wire.rs:64` (`run_chat`). Only `run_messages` (185) and `run_responses` (317) clamp via `output_cap`.
- Catalog limits: claude-opus-5 / sonnet-5 / fable-5-1 = 128_000 (< 131_072 → **Extra already exceeds**); claude-haiku-4-5 = 64_000 (< 65_536 → **High exceeds**); every antigravity model 65_536/65_535; grok-4.3 / grok-4.1-fast 128_000; grok-code-fast-1 / grok-build-0.1 32_768 (**High exceeds**); kimi-k3 131_072; big-pickle 32_000.
- The Anthropic API rejects `max_tokens > model max` with HTTP 400 ("max_tokens: … > 128000, which is the maximum allowed …"). `router::is_retriable` (router.rs:178-204) has nothing matching that text, so `handler.rs:322-329` ends the run with an error instead of hopping. Same class of 400 on Gemini (`maxOutputTokens`) and xAI.
- User-visible: picking Extra or Ultra on Claude — the flagship and default provider — fails every send with an opaque `http 400`. The router advertises exactly this: `router.rs` `effort_options("claude")` "Extra: … 128k output", "Ultra: … 256k output".
- Fix: clamp once, centrally: `req.max_tokens.min(output_limit(provider, model))` in every adapter body builder (lift opencode's `output_cap` into `catalog`). Test that every `(provider, effort)` pick yields `max_tokens <= output_limit`. Effort: S.

**F2. Antigravity OAuth callback has no `state` (login CSRF) and no PKCE; the browser is opened before the listener binds.**
- `antigravity_oauth.rs:55-60` `auth_url()` sends `client_id/redirect_uri/response_type/scope/access_type/prompt` — no `state`, no `code_challenge`.
- `antigravity_oauth.rs:131-163` `wait_for_code` binds `127.0.0.1:51121` (good), then accepts the **first** GET on any path, extracts `code=` and returns it; nothing is compared to a nonce.
- Any web page open during the 5-minute window can `fetch("http://localhost:51121/oauth-callback?code=<attacker code>")`; Parzi exchanges the attacker's code (`exchange_code`, 110-119) and stores the attacker's Google tokens in the keyring. Every later Antigravity conversation is then sent under the attacker's account/project — a silent exfiltration channel for prompts and tool output.
- Ordering: `crates/parzi-cli/src/main.rs:371-375` and `src-tauri/src/main.rs:1291-1296` call `open_browser(&url)` **before** `wait_for_code(300)` binds; a fast redirect hits a closed port.
- Fix: random `state` + PKCE S256 (`code_challenge` in the URL, `code_verifier` in the exchange), verify `state` and path (`/oauth-callback`) in `wait_for_code`, bind before opening the browser. Effort: S–M. (`CLIENT_SECRET` at line 12 is the upstream's public installed-app secret — acceptable; PKCE is what actually protects the flow.)

**F3. `find_token` scrape can ship a *different provider's* API key to opencode.ai, then classify it as a subscription.**
- `opencode.rs:79-97` `resolve_key`: keyring → env → exact `opencode-go.key` → **`find_token(&v)`** over the whole opencode auth.json.
- `types.rs:205-223` `find_token` returns the first string under a key containing `token`/`api_key` or equal to `key`/`apikey`. opencode's auth.json is `{ "<provider>": {"type":"api","key":"…"} | {"type":"oauth","refresh":…,"access":…} }`; with no `opencode-go` entry but e.g. `"anthropic": {"type":"api","key":"sk-ant-…"}` present, the Anthropic key is returned.
- It is then sent as `Authorization: Bearer sk-ant-…` / `x-api-key: sk-ant-…` to `https://opencode.ai/zen/…` (opencode.rs:115-152); `auth_status` = Ok, `billing()` = **Subscription** (opencode.rs:306-312) so Smart Auto routes to it with `keys_in_auto=false`; the 401 is non-retriable (run dies) and the third party has already seen the key.
- Same pattern for Grok (`compat_providers.rs:28-38` scrapes `.grok/user-settings.json`) — lower risk, same shape.
- Fix: delete the `find_token` fallback in `resolve_key` (its own docstring calls it "legacy"); read the documented key path only for Grok. Effort: S.

**F4. Multi-byte UTF-8 split across TCP chunks is corrupted to U+FFFD in every adapter.**
- Every stream loop does `buf.push_str(&String::from_utf8_lossy(&chunk))` per chunk: `anthropic.rs:227`, `codex.rs:≈246`, `openai_compat.rs` `run_sse` chunk loop, `antigravity.rs:448`, `opencode_wire.rs:81,202,331`.
- A 2–4 byte character (ä, ü, ß, →, emoji, CJK) straddling a chunk boundary becomes two replacement characters. Visible text is garbled and — worse — tool-call arguments containing such characters (file paths with umlauts on a German user's machine) are silently wrong: the JSON still parses, the path differs.
- Fix: buffer `Vec<u8>`, split on `b'\n'`, decode complete lines with `std::str::from_utf8`. Do it once in a shared `sse_lines()` reader — PLAN §3 promised "Shared SSE + tool-call core"; today there are seven copies of the same loop. Effort: M as seven patches, S as one reader.

**F5. Parallel tool calls: only the last `tool_use` survives on Anthropic, Zen-messages, Codex and Zen-responses.**
- `anthropic.rs:264-279`: a second `content_block_start{tool_use}` overwrites `tool_id/tool_name` and `tool_json.clear()`; a single `ToolCall` is emitted at 299-303. Same code in `opencode_wire.rs:235-250/275-279`.
- `codex.rs` `response.output_item.done` arm (≈285-293) and `opencode_wire.rs:356-372`: each `function_call` item overwrites `fn_name/call_id/arg_buf`; one `ToolCall` at the end. (`function_call_arguments.delta` accumulation is also shared across calls; redundant since `.done` carries full `arguments`.)
- Claude 4.6+/Opus 5 emit several `tool_use` blocks per turn by default; dropped calls never get a result and the agent loops or answers with missing information. openai_compat/run_chat (index-keyed map) and antigravity (Vec) are correct.
- Fix: push the pending call into a `Vec` on each new `content_block_start` / `output_item.done`; emit all at end. Effort: S per site.

**F6. In-stream provider error frames are swallowed; a mid-stream overload never fails over.**
- Anthropic sends `event: error` + `data: {"type":"error","error":{"type":"overloaded_error",…}}` on a 200 stream; `anthropic.rs:239-296` matches on `type` and drops `"error"` in `_ => {}`. Codex `response.failed` / `{"type":"error"}` → `_`. OpenAI-compat and Zen chat: `{"error":{…}}` has no `choices` → `continue` (openai_compat run_sse; opencode_wire.rs:97-105). Gemini `{"error":{…}}` has no `candidates` → ignored (antigravity.rs:461-475).
- The stream simply ends; the handler records a finished answer (`done: true`) from the partial text; no `RouteTransition`, no error shown, no hop.
- Fix: in each parser, `if let Some(err) = v.get("error") { return Err(…) }` with the retriable class for `overloaded_error`/`rate_limit_error`. Effort: S–M.

**F7. Timeouts: a 180 s *total* timeout aborts legitimate long generations and is then classified retriable (hop + duplicated partial answer); no idle/connect timeout anywhere; OAuth refresh has no timeout and is awaited on the caller's path.**
- `reqwest::Client::builder().timeout(180s)` is a total deadline that includes reading the body: `anthropic.rs:75`, `codex.rs:≈195`, `antigravity.rs:380`, `opencode.rs:161`; `openai_compat.rs:30` uses 120 s. Opus 5 / Fable 5.1 at high effort with 64k–128k output routinely run longer than 3 minutes.
- On expiry reqwest reports "… operation timed out"; `router.rs:196-197` treats `"timed out"`/`"timeout"` as retriable; `handler.rs:356-366` saves the partial text as an Assistant event and hops to the next provider, which answers again from scratch — duplicate content, double spend.
- No `connect_timeout`, no `read_timeout` (idle between chunks); antigravity's 3-host loop (`antigravity.rs:388-428`) can take 3 × 180 s = 9 min before reporting.
- `antigravity_oauth.rs:91` `reqwest::Client::new()` (no timeout at all) backs `refresh_access`, which `antigravity.rs:238-243` awaits **inside `chat_stream` before returning `rx`** — a hung token endpoint hangs the orchestrator's `chat_stream` call, not just the stream.
- Fix: `connect_timeout(10s)` + `read_timeout(90s)` on stream clients and drop (or raise to hours) the total timeout; 30 s on the token and `/models` clients; move `refresh_now` into the spawned task; stop classifying client-side timeouts as retriable once text has streamed. Effort: S.

### P2 — moderate

**F8. Antigravity token usage is over-counted (emitted per chunk, summed by the handler).**
- `antigravity.rs:462-466` emits `Usage` for every chunk carrying `usageMetadata`; Gemini streams it (cumulative `promptTokenCount`) on every chunk. `handler.rs:348-353` `add_usage` sums each event → tokens_in ≈ prompt tokens × chunk count. Money is $0 (subscription) but the meter and per-session stats are off by an order of magnitude.
- Fix: keep the last seen `usageMetadata`, emit once after the loop. Effort: S.

**F9. Cancellation does not stop the HTTP stream.**
- All adapters ignore `tx.send` results (`let _ =`, 50+ sites) and never check `tx.is_closed()`; when the handler cancels (`handler.rs:295-298` drops `rx`) the spawned task drains the whole response — tokens/quota burn to completion.
- Fix: `if tx.send(..).is_err() { return Ok(()) }` (dropping `resp` aborts the connection). Effort: S.

**F10. `stop_reason` / `finishReason` never read; refusals and truncation look like normal completions.**
- Anthropic `message_delta.stop_reason` (`refusal` on Fable 5.1, `max_tokens`): `anthropic.rs:281-286` reads only `usage`. Codex `response.incomplete`, Gemini `finishReason: MAX_TOKENS|SAFETY` likewise ignored. User sees an empty or cut-off answer with no explanation; on `max_tokens` the agent loop continues as if the turn were complete.
- Fix: new `StreamEvent::Stop(reason)` or `Err` for `refusal`/`SAFETY`, a `Notice` for `max_tokens`. Effort: S.

**F11. Failover classification by substring is both too broad and too narrow.**
- `router.rs:178-204`: `"insufficient"` matches 403 `insufficient permissions`/`insufficient_scope`; `"try again"` matches 400 "invalid request … please try again"; `"429"` matches any string containing it; `"timeout"` matches client aborts (F7). Meanwhile plain `http 500`, `http 502`, `http 408` and mid-stream `stream: error decoding response body` (connection reset) are **not** retriable → run dies. Antigravity's transport failure (`code 0`, `antigravity.rs:364`) is deliberately non-retriable while other adapters' `error sending request` is (router.rs:203) — inconsistent by design. `parse_cooldown_secs` (131-171) on an HTTP-date `Retry-After` ("Wed, 21 Oct …") returns 21 s.
- Fix: carry a structured kind in the error (`RateLimited{retry_after} | Overloaded | Auth | BadRequest | Transport | Timeout | Refusal`) set by the adapter at the HTTP boundary; router/handler match on it; keep string sniffing only as fallback. Effort: M.

**F12. Codex: no expiry detection; the subscription path is unverified yet sits at auto slot 2.**
- `codex.rs:60-75` reads `tokens.access_token` and `account_id` only; JWT `exp` / `last_refresh` unchecked (unlike `claude.rs:69-71`). Health says `ok`; the first send after expiry is `http 401` → non-retriable → run dies with no "run `codex login`" hint.
- PROGRESS:524 admits "Not verified live: an actual Codex-subscription completion"; `codex.rs:25-29` now claims "Verified 2026-09-14: gpt-5.3-codex … deprecated there". Two contradicting claims, neither backed by a test or transcript. Default `auto_order` puts codex second.
- Fix: decode the JWT payload (base64, unverified) → `exp` → `AuthStatus::Expired`; record one real ChatGPT-backend transcript in PROGRESS or mark the path experimental in the picker. Effort: S.

**F13. opencode billing class is asserted, not derived — a pay-per-token key is routed as a subscription and metered at $0.**
- `opencode.rs:306-312`: any resolved key → `Billing::Subscription`; `account_label` → "OpenCode Go". `OPENCODE_API_KEY` / keyring `opencode` (the Settings "API key" slot, `lib.rs:62`) can be a Zen pay-as-you-go key. Smart Auto spends it with `keys_in_auto=false`; `orchestrator.rs:341-346` zeroes prices for subscriptions, so the meter shows $0 for real spend (PROGRESS:651 glosses this as "Go $ caps not surfaced").
- Fix: `Subscription` only when the key came from the `opencode-go` auth.json slot; env/keyring keys → `ApiKey` unless config says `providers.opencode.plan = "go"`. Effort: S.

**F14. Hard-coded foreign GCP project id.**
- `antigravity.rs:198` `"project": … unwrap_or_else(|| "rising-fact-p41fc".into())` — with no `ANTIGRAVITY_PROJECT_ID` and no accounts file, every request is attributed to someone else's project (upstream plugin default).
- Fix: error with "no Antigravity project id — set ANTIGRAVITY_PROJECT_ID" (or port upstream's `loadCodeAssist` discovery). Effort: S.

**F15. Antigravity host fallback replays every non-2xx (except 401/403) on all three hosts.**
- `antigravity.rs:388-428`: a deterministic 400 (bad schema) or a 429 is re-sent to autopush and prod — 3× the quota hit on a rate limit, 3× the latency (each up to 180 s, F7).
- Fix: fall through only on transport errors and 5xx; return 4xx immediately. Effort: S.

**F16. Tool-call history is replayed as plain text on every provider (cross-cutting with parzi-core).**
- `crates/parzi-core/src/context.rs:70-81` renders `ToolCall` as assistant text `[tool:name {…}]` and `ToolResult` as `Role::Tool` text; every adapter maps `Role::Tool` → `"user"` and sends strings only (`anthropic.rs:81-91`, `codex.rs:≈143-153`, `antigravity.rs:139-150`, `openai_compat.rs:52-66`).
- No `tool_use`/`tool_result` (or `functionCall`/`functionResponse`) blocks ever reach the model; PLAN §3's Antigravity "session recovery via synthetic `tool_result`" cannot exist in this shape. Multi-step tool use degrades (re-issued calls, mis-attributed results). Effort: L; for the core/runtime auditors.

**F17. Claims vs code.**
- PROGRESS:16 / `antigravity.rs:5-7` claim "Claude<->Gemini transform → thinking-strip → schema allowlist → SSE transform" ported. Only `clean_schema` and `wrap` exist; no thinking-strip (nothing to strip — thoughts are never replayed), no message transform beyond role renaming.
- `opencode.rs:157-159` justifies `.http1_only()` with "Zen's edge resets negotiated HTTP/2 streams"; `Cargo.toml:13` is `default-features = false` without the `http2` feature and `Cargo.lock` contains no `h2` package — HTTP/2 is not compiled in, the call is a no-op, and the PROGRESS:648-649 diagnosis is unsupported by this build.
- `THIRD_PARTY_NOTICES.md` cites `crates/parzi-providers/src/t3.rs` (does not exist) and `ui/src/assets/providers/` (deleted in this tree).
- `catalog.rs` cache comment promises "TTL-gated refresh, retry backoff"; see F19.
- PROGRESS:17 "router for 9 ids" is historical; the roster is 5 (lib.rs:27) and matches README. Effort: S (docs).

**F18. Catalog ids/prices — unverifiable from code; two default Smart-Auto picks are $0 placeholders.**
- Verified against the current Anthropic reference table: all six `claude()` rows (ids, 1M/200K context, 128K/64K output, $5/$25, $2/$10, $1/$5, $10/$50, $5/$25, $3/$15) are correct.
- Unverifiable (no artefact in repo): `codex()` `gpt-5.5` ($5/$30), `gpt-5.6-terra`/`gpt-5.6-luna` at **$0/$0** (catalog.rs:374-375) while the same `gpt-5.6-luna` id is $0.2/$1.2 in the opencode table (catalog.rs:304); `gpt-5.3-codex` price changed 1.25/10 → 1.75/14 in this tree; all 14 `antigravity()` ids ("verified via `agy models` 2026-09-09"); all 35 `opencode()` ids ("verified 2026-09-14 against local serve"); the freshly replaced `xai()` table (`grok-4.3` default $1.25/$2.5 1M ctx, `grok-4.6`, `grok-4.5`, `grok-4.1-fast` $0.2/$0.5 2M ctx, …).
- `router.rs:32-34` now picks `gpt-5.6-luna` (low) / `gpt-5.6-terra` (medium) / `gpt-5.5` (high). On the API-key path `codex.rs:39-45` remaps luna → `gpt-5.3-codex` ($1.75/$14) and terra → `gpt-5.5` ($5/$30), but `handler.rs:176-183` prices by the *picked* id → the low/medium Smart-Auto picks are metered at **$0** while spending real money.
- Fix: price by the id actually sent (return it in `Usage` or resolve in the adapter); attach a verification artefact (curl output) for each non-Anthropic table. Effort: S + verification.

**F19. Catalog disk cache: global TTL, TTL gates reads not refreshes, dead retry code, write race.**
- `CacheFile { fetched_at, models }` — one `fetched_at` for all providers; storing xai's live list renews opencode's stale entry.
- `cached_models` returns `None` after 24 h, so a day offline loses the last-known live list and falls back to the static table — the opposite of "offline keeps working"; a refresh is attempted on every `models()` call regardless of TTL.
- `retry_wait_secs`/`CACHE_RETRY_SECS` have no callers — the "retry backoff" is unimplemented.
- `atomic_write` uses a fixed `catalog.tmp`; Tauri's `model_row_for` calls `models()` for all providers concurrently, so two `store_models` interleave (read-modify-write) and can lose an entry or fail the rename silently.
- Fix: per-provider `fetched_at`; TTL decides whether to *fetch*, cache is always served; unique tmp name; delete or wire the dead constant. Effort: S–M.

### P3 — nits

**F20.** `antigravity_oauth.rs:44,45,50` three `.unwrap()` on static header parses (cannot fail; PLAN §11 says none outside tests). Workspace lints are `clippy all/pedantic = "warn"` (`Cargo.toml:14-16`), not the "deny in CI" PLAN §11 claims — cross-cutting.

**F21.** `tracing = "0.1"` (Cargo.toml:18) is unused: zero `tracing::`/`println!` in the crate. Good for secrets, but there is no observability at all (no request/status logging even at debug).

**F22.** No cap on SSE line length: `buf` grows until `\n`; a misbehaving proxy returning a newline-free body grows memory for up to 180 s. Cap at ~4 MB.

**F23.** Per-request `reqwest::Client` construction at every call (anthropic `client()`, codex spawn, antigravity `post_once`, opencode `client()`, openai_compat `client()`): no connection reuse, TLS rebuilt each time. `rustls-tls-webpki-roots` ignores the OS trust store (corporate MITM CAs fail); with `default-features=false` the `system-proxy` feature is off — Windows proxy settings ignored, only `HTTP(S)_PROXY` env applies (verify).

**F24.** `key_entry("opencode") = "opencode"` (`lib.rs:62`) while the doc at 54-56 says subscription tokens never live in key slots — for opencode the "API key" slot *is* the subscription (F13).

**F25.** `clean_schema` (`antigravity.rs:124-131`) invents a `{"reason": string}` parameter for parameterless tools; drops `anyOf/oneOf/allOf`, array `type` (`["string","null"]`), `format`, `minimum/maximum` — a parameter can lose its type entirely. Upstream trick; document, and map single-type `anyOf` to that type.

**F26.** `anthropic.rs:121-128` sends `xhigh` for "extra" on every non-budget model; `claude-sonnet-4-6` (legacy, in catalog) only supports low/medium/high/max → 400 on that legacy id. `router::effort_options("claude")` hints ("effort high · 128k output") are stale vs the adapter (xhigh/max).

**F27.** `antigravity_oauth::wait_for_code` reads once into 8 KiB and parses the request line; ignores `Host` and path (see F2).

**F28.** Tests are host-dependent: `tests/opencode.rs` `exact_key_resolution_prefers_env_without_logging` fails on any machine with a `parzi/opencode` keyring entry (keyring consulted first, `opencode.rs:81`); `tests/router.rs` acknowledges `~/.claude`/`~/.codex` can flip results. `tests/antigravity_schema.rs` header claims "request wrap shape" but `wrap()` is private and untested.

**F29.** Antigravity is auto-routed as soon as *any* credential exists (default `auto_order` includes it; `ANTIGRAVITY_ACCESS_TOKEN` alone suffices — `tests/router.rs:69-76` proves it). PLAN §3: "opt-in toggle … never default-on"; today opt-in = signed-in, no config switch. The CLI login has a y/N TOS prompt; `src-tauri login_antigravity` has none server-side.

**F30.** Cross-crate: `crates/parzi-cli/src/main.rs:394-400` `cmd_logout(provider)` deletes `parzi/{provider}`, `{provider}-refresh`, `{provider}-session`; for `claude` the adapter reads `claude-code`/`anthropic`, for `codex` also `openai` — logout is a no-op for those slots.

**F31.** `Model::default()` (`types.rs:54`) is a builder that shadows the `Default` idiom; `compat_providers.rs` now holds only xai (name misleading); `is_loopback` (`openai_compat.rs`) is a leftover from the retired `opencode serve` path, reachable only via an xai `base_url` override.

---

## Verified good

- **No secret-leak path found in the crate**: no `#[derive(Debug)]` on any struct holding a token (`AnthropicNative`, `Codex`, `Antigravity`, `Opencode`, `OpenAiCompat`, `Tokens`, `CliSignIn`); no `{:?}` formatting; no `tracing`/`println!` at all; `auth_status`/`health`/hints carry only fixed strings; error strings carry status + ≤300 chars of response body, never request headers.
- **Credential files are read-only**: `~/.claude/.credentials.json` (claude.rs:25), `~/.codex/auth.json` (codex.rs:61), `~/.local/share/opencode/auth.json` / `~/.config/opencode/auth.json` (opencode.rs:71-77), `~/.config/opencode/antigravity-accounts.json` (antigravity.rs:85), `~/.grok/*.json` (compat_providers.rs:30) — all via `read_json_file`; nothing under those dirs is ever written. Keyring writes are limited to `parzi/antigravity` and `parzi/antigravity-refresh` (antigravity.rs:64-71, 276-278), names consistent with CLI/Tauri login and logout.
- **Env fallbacks** present and consistent: `ANTHROPIC_AUTH_TOKEN`/`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `XAI_API_KEY`/`GROK_API_KEY`, `OPENCODE_API_KEY`, `ANTIGRAVITY_ACCESS_TOKEN`/`_REFRESH_TOKEN`/`_PROJECT_ID`; empty strings ignored.
- **Claude adapter vs current API**: model ids and prices match the reference table; `takes_budget` split (Haiku 4.5 / Sonnet 4.5 / 4.1 / 4.0 / 3.x → `budget_tokens` clamped `< max_tokens`, `>= 1024`; everything else → `thinking: adaptive` + `output_config.effort` low/medium/high/xhigh/max) is correct; `display: "summarized"` set so the reasoning rail is not empty on 4.7+; OAuth bearer + `anthropic-beta: oauth-2025-04-20` only on the bearer path; no prefill, no forced `tool_choice`; tool names sanitized to `^[a-zA-Z0-9_-]{1,64}$` and mapped back (new in this tree, all adapters).
- **Claude CLI token expiry** detected via `expiresAt` → `AuthStatus::Expired`; skipped by auto, still honoured on explicit pick.
- **Router**: subscriptions first, keys only with `routing.keys_in_auto`, unauthed skipped, alias-normalised, duplicate-free, deterministic (router.rs:50-98); `tier_fallback_chain` slot 0 = explicit pick, primary excluded afterwards; matches README and PROGRESS:496. Failover bounded by slot count (handler.rs:204-278); Antigravity refresh-and-retry happens exactly once (antigravity.rs:264-313). Picks and catalogs were moved together by the lane (tests/router.rs updated to `grok-4.3`).
- **Trait/roster**: `Provider` is dyn-compatible (`async_trait`, `Box<dyn Provider>`); `PROVIDERS` = 5 ids = README; `opencode` registered in `provider()` (lib.rs:85), `PROVIDERS`, `router::pick`, `catalog::for_provider` and reachable; `opencode_wire` private; retired ids (`ollama`, `t3`, `openrouter`) resolve to `None`.
- **SSE basics**: `data:` prefix + trim (CRLF-safe), keep-alive comment lines and `ping` events ignored, `[DONE]` handled where applicable, JSON parse failures skipped; OpenAI-compat and Zen chat assemble tool-call fragments keyed by `index` with id capture; Antigravity collects all `functionCall` parts; Anthropic `input_json_delta` accumulation correct for a single call.
- **Antigravity port**: MIT attribution in both source headers and THIRD_PARTY_NOTICES; host order daily→autopush→prod as PLAN; `clean_schema` keeps the allowlist, string `const`→`enum` (numeric dropped — new), `$ref`/`additionalProperties`/`default`/`title` removed; thought parts go to the reasoning rail; callback listener binds `127.0.0.1` with a 5-minute timeout; CLI login has a TOS confirmation.
- **opencode**: `x-opencode-session` stable per Parzi session, identifying UA, `output_cap` clamps on messages/responses kinds, three wire kinds routed by id table with `-free` heuristic, legacy `opencode-default` resolved.
- **Catalog**: `catalog_refresh` kill switch honoured at both live-pull sites (openai_compat.rs:97, opencode.rs:218); `store_models` refuses to overwrite with an empty list; `atomic_write` is tmp+rename; family/variant collapse for the gemini families and explicit `gpt-oss-120b`; `with_effort` maps family bases to variants and passes explicit variants through; one default per provider (tests enforce).

---

## Tests to add

1. **Mocked SSE per parser** (anthropic, codex, openai_compat, antigravity, opencode_wire × 3) — PLAN §11 requires mocked adapters and there are none today: feed a `Vec<Bytes>` chunk sequence and assert events — multi-byte char split across chunks; CRLF lines; `: keep-alive` comments; `[DONE]`; an `event: error` / `{"error":…}` frame → `Err` with retriable kind; usage extracted once with correct numbers; an oversized line.
2. **Parallel tool calls**: two `tool_use` blocks (Anthropic / Zen-messages) and two `function_call` items (Codex / Zen-responses) → two `ToolCall` events with distinct ids and correctly assembled args.
3. **Request-body snapshots**: `AnthropicNative::body()` per effort rung and catalog model (budget vs adaptive, effort ladder, `max_tokens <= output_limit`); `antigravity::wrap()` shape (`project/model/request`, variant mapping, `thinkingConfig`); Codex subscription vs API-key body (`store:false` vs `max_output_tokens`, model remap).
4. **Timeouts**: local TCP mock that sends one chunk then stalls → error within `read_timeout`; slow-but-alive stream (chunk every 5 s for > 180 s) → completes.
5. **Cancellation**: drop `rx` after the first event → adapter task exits and the mock server sees the connection close.
6. **OAuth**: `auth_url()` carries `state` and `code_challenge`; `wait_for_code` rejects a wrong `state`; listener bound before `open_browser` (inject the opener).
7. **Key resolution**: opencode auth.json containing only `anthropic.key` → `resolve_key() == None`; billing `ApiKey` for env/keyring keys, `Subscription` only for `opencode-go`. Make tests hermetic by injecting a `KeyStore` trait (or `PARZI_KEYRING=none`) instead of the OS keyring.
8. **Router classification** (table-driven, both directions): negatives — `http 403: insufficient permissions`, `http 400: … please try again`, client `operation timed out` after streamed text; positives — `http 500`, `http 502`, `stream: error decoding response body`, in-stream `overloaded_error`. `parse_cooldown_secs` on an HTTP-date → `None`.
9. **Catalog cache**: per-provider freshness; stale cache still served offline; two concurrent `store_models` keep both entries.
10. **Codex expiry**: auth.json with an expired JWT `exp` → `AuthStatus::Expired`.
11. **Antigravity usage**: multi-chunk stream with cumulative `usageMetadata` → exactly one `Usage` with the final counts; `post_once` does not retry a 400 on the next host.
12. **Pricing**: the cost recorded for a codex low/medium auto run equals the price of the model actually sent (after remap), not $0.
