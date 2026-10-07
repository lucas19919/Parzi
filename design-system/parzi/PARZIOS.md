# ParziOS — headless quartermaster + one-click remote setup (plan)

Vision: ParziOS runs on your VPS/server holding context, settings,
brain, and sessions. Desktop and Mobile Parzi are thin harnesses. The
onboarding climax: **Set up remote → type `user@ip` → Parzi does the
rest over your SSH** — sends the package, the package reads the specs
and sets itself up optimally.

## 0. Where we stand (verified 2026-10-07)

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
