# MCP — connector support across the board (plan)

Connectors are how Parzi reaches the outside world (email, calendar,
databases, services). Today: config-file-only (`mcp.servers`), no UI,
no discovery, no health. This plan takes it from hidden to first-class.

## 0. What exists (verified 2026-10-07)

- `McpServerCfg { command, args, env, allow, deny, tool_modes,
  timeout_ms, enabled }` in config; `McpManager` spawns stdio servers,
  idle-kills them, routes `server.tool` calls with per-tool modes
  (auto/deny/ask) through the normal approval flow.
- `save_config` already confirms connector command changes with a
  security dialog. Env passthrough is an allowlist
  (`CHILD_ENV_PASSTHROUGH`).
- Tasks dock tab reads `cfg.mcp.servers` for the Connections list —
  config presence only, not liveness.

## 1. Local connector UX (do first)

1. **Settings › System connectors section**: table of servers
   (name, command + args, on/off toggle), add/edit/delete.
   Reuses `save_config` confirm. Validation before save: command
   resolves on PATH (`process::resolve`), fail message names the fix.
2. **Curated manifest** (`connectors.toml` baked in): ~15 known
   servers (filesystem, sqlite, postgres, github, fetch, memory…).
   "Add connector" offers manifest first (one toggle), custom second.
3. **Discovery**: onboarding + a System "Scan" button walk PATH and
   well-known npm/global locations, match manifest entries, propose —
   user toggles, nothing enables itself.
4. **`connector.propose` agent tool** (read-only): scans + returns
   suggestion text. Lets Work do "connect my email" as a conversation
   ending in one toggle. Never writes config itself.
5. **Health**: `connector_status` command (spawn probe → tools/list →
   count + latency, no quota spent) backing green/grey dots in Tasks
   Connections and a "Check again" button. Failure shows stderr tail,
   not a toast wall.

## 2. Policy model (no new concepts)

- Per-tool modes stay (`tool_modes`: auto/deny/ask) + lane allowlists
  stay (Work/Build see connectors, Search sees none).
- `allow`/`deny` lists are tool-name filters per server (already
  implemented, just unexposed — surface as multi-toggle in the editor).
- Secrets live in `env` per server, values stored sealed (OS keyring
  where present, else 0600 file + warning — same rule as ParziOS §5).
  Never echoed into threads, never into logs.

## 3. ParziOS travel

- `spec.json` carries `mcp.servers` **minus secrets** + a secret manifest
  (names only). Secrets travel once over the SSH tunnel at setup, then
  the local copy is wiped (same rule as provider keys).
- `parzi-os setup` installs connector runtimes per manifest
  (`needs: [node, docker, …]` declared per entry; missing runtime =
  loud skip, not silent break), then runs the same health probe and
  reports per-connector status in the setup stream.
- Remote `connector_status` rides the token bridge like everything else.

## 4. Later (explicitly not now)

- OAuth connectors (Google etc.): device-code flow UI; needs the
  RemoteApprover pattern. Spec when a real connector demands it.
- Per-connector network policy (domain allowlists): needs the sandbox
  story first.
- Hot-reload of edited servers without run restart (today: applies to
  `apply_config`; verify no in-flight call drops).

## Order

1. Settings connectors CRUD + validation.
2. Manifest + discovery scan + `connector.propose`.
3. `connector_status` + live dots.
4. ParziOS travel (with the remote build).
