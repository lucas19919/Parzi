# ParziOS — headless quartermaster + one-click remote setup (plan)

Vision: ParziOS runs on your VPS/server holding context, settings,
brain, and sessions. Desktop and Mobile Parzi are thin harnesses. The
onboarding climax: **Set up remote → type `user@ip` → Parzi does the
rest over your SSH** — sends the package, the package reads the specs
and sets itself up optimally.

## Built (2026-10-10)

The one-button flow exists end to end. What shipped, and where it
departs from the plan below:

- **Transport: `parzi rpc` over SSH stdio, not `ssh -L`.** The desktop
  runs `ssh … ~/.local/bin/parzi rpc`; that process reads the 0600
  `serve.json` on the server and relays newline JSON (with request ids)
  to `parzi serve` on 127.0.0.1. No port forward to manage, and the
  token never leaves the server. Every other remote action is
  `ssh … sh -s` with the script on stdin, so fish or zsh login shells
  only ever parse `sh -s`. (`parzi_runtime::remote`)
- **Setup** (`remote::setup`, desktop wizard and `parzi remote setup`):
  key login, else password once via a throwaway askpass helper, after
  which Parzi's own key (`~/.parzi/ssh/id_ed25519`) is added to
  `authorized_keys`; probe (Linux x64/arm64, curl/wget, systemd);
  install to `~/.local/bin/parzi` from the GitHub release with a
  `.sha256` check (or upload a local build); spec upload; then
  `parzi setup --spec … --enable --json`, whose JSON steps stream into
  the wizard; link and save `~/.parzi/remote.json`.
- **Spec v1** (`parzi_runtime::provision::Spec`): the portable config
  (connectors and agent program paths stripped; the server keeps its
  own) and brain notes minus project notes and archived ones. Notes are
  add-only on the server. The spec file is deleted once applied.
- **Server side** (`parzi_runtime::provision`): systemd user unit at
  `~/.config/systemd/user/parzi-os.service`, enabled; lingering checked
  and explained (never sudo); without user systemd the engine starts
  detached and `parzi rpc` restarts it when it is down. An engine of an
  older version is asked to `shutdown` and replaced, so upgrades are a
  re-run of setup.
- **Approvals: `ParkedApprover`, in memory.** Not durable, on purpose: a
  run cannot outlive its engine anyway (`recover()` marks it idle). A
  parked approval nobody answers is denied after 6 hours.
- **Serve ops added:** `session.events` (newest page, then deltas by
  `from`), `providers`, `doctor`, `shutdown`. The socket has a 10 s
  request timeout and a 64-connection cap; `serve.json` is born 0600 in
  a 0700 home.
- **Desktop:** the Remote tab (sessions, thread, composer, approvals,
  agents with Install/Sign in in a visible `ssh -t` terminal), the
  onboarding step, Settings › System remote row.
- **Agents on the server:** `parzi agent install|login <id>` runs the
  vendor's own installer or login on that OS.
- **Release:** `parzi-linux-x64` and `parzi-linux-arm64` (+ `.sha256`),
  built on Ubuntu 22.04 (glibc 2.35 floor). CI tests the engine on Linux.
- Tested end to end against WSL standing in for the server
  (`crates/parzi-runtime/tests/remote_wsl.rs`, ignored by default).

Still open, in order:

1. Connector onboarding beyond the Settings list: the manifest scan and
   `connector.propose`.
2. Browser tools on the server (headless Chromium over CDP). Today
   sessions on a server simply have no browser tools.
3. Provider API keys sealed on the server (keyring or 0600 file). Today
   agents sign in with their own login in the `ssh -t` terminal and
   Parzi stores no secret.
4. Live text streaming to the Remote tab. Today the tab polls and shows
   a turn's text when the turn ends (the store writes assistant text at
   turn end).
5. Mobile: the same Remote tab with no local engine.

## 0. Where we stood (verified 2026-10-07)

- `parzi-core` / `parzi-providers` / `parzi-runtime` are **Tauri-free**
  (no tauri dep). The engine travels; only `src-tauri` doesn't.
- `parzi-cli` already drives the same `Orchestrator` headless
  (init/doctor/providers/session/tab). It is the ParziOS seed.
- MCP manager spawns stdio servers — headless-safe as-is.
- Desktop talks to its engine over a **token-authed localhost TCP
  bridge** (`gui.json`: port + token). The remote protocol wants to
  look like this, over SSH, not HTTP.
- Release CI builds Windows + macOS only. No Linux artifact exists.
- **No connector UI exists anywhere.** MCP servers are config-file-only
  today, and the Tasks tab points at a Settings › System section that
  doesn't manage them. Connector onboarding is greenfield, local first.
- Windows ships `ssh.exe` (verified present). SSH is the only remote
  dependency and it's already installed.
- Known non-travelers: WebView browser hands (`pagectl`/click/type),
  `GuiApprover` cards, provider CLIs needing interactive login.

## 1. Connector onboarding (local first — do this before any remote)

1. **Connectors UI in Settings › System**: list configured MCP servers
   (name, command, on/off), add/edit/delete with the existing
   `save_config` security confirm. Reuse `McpServerCfg` untouched.
2. **Curated manifest** (`connectors.toml` baked in): known tools
   (github, postgres, sqlite, filesystem, fetch…) → package + default
   args. "Add" offers the manifest first, custom command second.
3. **Propose, don't autoinstall** (recorded verdict): onboarding scans
   PATH (`process::resolve` already does), suggests matches, user
   toggles. Nothing enables itself — context bloat + arbitrary code.
4. **Work-lane onboarding job**: the Work brief already covers guiding
   connections; give it `connector.propose` (read-only: scan + suggest
   text) so "onboard my email" is a conversation ending in one toggle.
5. Fix the Tasks tab dead-end to point at the real section.

## 2. ParziOS package

- New binary in the existing workspace: `parzi-os` (or `parzi serve`
  in parzi-cli — prefer the latter, zero new crates to wire).
  Commands: `serve` (daemon: orchestrator + API + queue + brain),
  `setup <spec.json>` (first-boot provisioner, see §4).
- API = the desk bridge grown up: same newline-JSON over a Unix socket
  locally, over `ssh -L` port-forward remotely. **No public ports,
  ever.** Auth = the `gui.json` token pattern, per-install secret.
- Linux CI: add `ubuntu-latest` x86_64 to release.yml. The engine has
  no OS-specific deps except small `cfg(windows)` blocks (job objects,
  consoles) — all already gated.
- Headless replacements, each small and explicit:
  - `GuiApprover` → `RemoteApprover`: parks the run (`ApprovalPending`
    status, durable), pushes over the bridge; approve/deny from any
    client resumes. This is the one real state-machine addition.
  - Browser hands → **headless Chromium** (not WebView): `pagectl`
    retargeted at a CDP endpoint. Better than parity — screenshots and
    DOM without a window, and it kills the 1px-sliver bug class.
    Desktop keeps WebView for the visible tabs; agents use Chromium
    everywhere.
  - Provider CLIs on first boot: install scripts already exist per
    vendor (setup.rs) and most are curl|bash-able on Linux. Auth is
    the hard part — see §4.

## 3. "Set up remote" — the one-button flow

Desktop side, all over the user's existing `ssh.exe`:

1. Button → input `user@host` (+ key/alias select; default
   `~/.ssh/id_*`). Test: `ssh -o BatchMode=yes user@host true`.
   Failure shows stderr verbatim, nothing else.
2. Probe the remote: arch, OS, systemd user availability, NVIDIA/CPU,
   disk. This becomes the spec sent with the package.
3. Ship: `scp` the ParziOS tarball (binary + `spec.json`:
   ParziConfig, theme, provider roster + versions, brain snapshot
   tarball, connector manifest choices) to `~/.parzi-os/`.
4. Run: `ssh user@host 'parzi-os setup spec.json'` — streams logs back
   into the Parzi setup window:
   - installs provider CLIs per roster (official installers),
   - **auth, per provider, in preference order**: (a) API key the
     user pastes once in Parzi (stored sealed, see §5); (b) OAuth
     device-code flow over the SSH session (link + code shown in
     Parzi, user completes in browser); (c) mark unsigned, sessions
     route around it;
   - restores brain snapshot, writes config, installs the systemd
     user unit, starts the daemon, prints the token QR/line.
5. Desktop stores the remote (alias, host, token) and becomes a switch:
   local engine / remote engine. Mobile gets the same switch with no
   local option.

## 4. Spec-driven self-setup (the package side)

`parzi-os setup spec.json` is idempotent — safe to re-run after
failure or for upgrades:

1. Validate spec version; back up any existing `~/.parzi-os`.
2. Install/verify each roster provider; record versions.
3. Auth pass (§3.4). Never store OAuth refresh tokens in cleartext —
   OS keyring where present, else mode-0600 file + loud warning.
4. Restore brain + config; `parzi doctor` gate — setup fails loudly
   instead of starting half-ready.
5. Write systemd unit (`parzi-os.service`, user scope, Restart=always),
   enable + start, health-check the local socket, report ready.

## 5. Security posture (day one, not hardening-later)

- No listening sockets on any public interface. Remote access only via
  SSH port-forward; the API token never leaves that tunnel.
- Secrets: provider keys + tokens sealed at rest (keyring / 0600),
  never in logs, never in `spec.json` after setup (keys travel once,
  in the SSH tunnel, then the local copy is wiped from the outbox).
- `save_config`-style confirm dialog already exists for connector
  changes — keep it on every client, including Mobile.
- Brain snapshot transfer is a tarball over scp inside the same SSH
  session — no third service, no new credentials.

## 6. Order

1. Connector UI + manifest + `connector.propose` (local value now,
   required by §3 anyway).
2. Linux CI artifact.
3. `RemoteApprover` (park/resume) — the only new state machine.
4. `parzi serve` + token bridge + headless Chromium swap.
5. `parzi-os setup` provisioner + `spec.json` schema.
6. Desktop "Set up remote" wizard (probe → ship → stream logs → switch).
7. Mobile = the same switch with no local engine.

Each step ships independently; 1–2 are useful with no remote at all.
