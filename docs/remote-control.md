# Remote control — run Parzi work on a VM

Status: direction agreed with Lucas. V1 ships now (no new wires);
V2/V3 are design, not promises.

## The insight

`parzi mcp` speaks newline-delimited JSON-RPC on stdio. SSH *is* an MCP
transport: `ssh vm parzi mcp` gives any harness — Claude Code, Opencode,
another Parzi — full Parzi-on-VM, with zero new protocol. And workspaces
are git repos that already sync. Remote control is therefore composition
of two things that exist, not a new system.

## V1 — SSH + git (ships now)

Provision the VM once: install the `parzi` CLI, `parzi init`, then install
the agents you want there and sign in with each agent's own program;
`parzi providers` confirms what the VM can run. Agents that file issues from
there need a GitHub token, same as here.

```json
{ "mcpServers": { "parzi-vm": {
  "command": "ssh",
  "args": ["user@vm", "parzi", "mcp"]
} } }
```

Four verbs carry the whole thing, on the CLI and as MCP tools
(`workspace_list` / `_remote` / `_sync` / `_clone`):

```powershell
parzi workspace remote acme git@github.com:you/acme-workspace.git
parzi workspace sync acme                       # seeds the remote
ssh vm parzi workspace clone git@github.com:you/acme-workspace.git acme
```

**Clone on the second machine, never create.** Two machines that each ran
`workspace_create` hold unrelated histories; git refuses to merge them and
no sync can fix it. `workspace_clone` is the only way in, and it refuses to
clone a repo that carries no `workspace.toml`.

The loop after that:

1. Local: do planning / review where the screens are.
2. `parzi workspace sync <name>` (or the MCP tool, or the app's own timer)
   — commits + pushes through the workspace's git remote.
3. Remote: sync there (SSH, or a `session_send` asking for it) pulls; heavy
   lanes run on the VM's CPUs; sync again when done.
4. Local syncs. Sessions themselves stay where they ran; transcripts are
   plain `session.md` files, so `session_show`/`export` reads either side.

A sync that hits a conflict reports the paths and exits non-zero — a
provisioning script stops there instead of syncing half a plan.

Rules that keep this safe:

- The VM enforces *its own* `workspace.toml [policy]` and hooks — a lockdown
  travels with the workspace because both are files in the synced tree.
  `SYSTEM.md`, `rules/`, `hooks.toml` sync the same way.
- Never sync provider keys: `~/.parzi` syncs *workspace dirs*, not the home.
  Each machine signs in itself. MCP children and hooks scrub secrets anyway.
- `report_issue` filed from the VM labels itself `agent-report` like any
  other agent report; add the VM name to the body when it matters.

## V2 — the comfort layer (next slice, not built)

- Ahead/behind reporting: `workspace list` says `(local only)` or the URL,
  not how far apart the two copies are. A `--status` that counts unpushed
  and unpulled commits is the one thing a person asks before syncing.
- The app's Workspaces panel still has no remote field — setting one is
  CLI/MCP only today.
- Health probe: `session_send` to the VM asking `doctor` — one call tells
  you the VM's providers, models and disk before you ship work over.
- Session handoff: `session_export` here → attachment → `session_send new`
  there, carrying the transcript as context. (Works today by hand;
  V2 makes it one command.)

## V3 — remote lanes (design only)

`lane.dispatch` to a `vm:<host>` lane: the orchestrator syncs, spawns the
worker *through* the remote MCP server, and streams its events back into
the local transcript. Needs: machine identity in workspace.toml,
secret-free auth (SSH agent), and cost attribution per machine. Not started
on purpose — V1 covers the real workflow (heavy work elsewhere, review
locally) without any of it.

## Explicitly out

- A custom wire protocol or Parzi daemon port. SSH + stdio + git is the
  whole transport story; anything else is operational surface with no user.
- Remote desktop / screen sharing. The transcript is the interface.
- Cross-machine leases. A lane holds files on exactly one machine; moving
  work means syncing, never shared locks.
