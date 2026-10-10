<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { api, type ParziConfig } from "../api";
  import { parseEnv, splitLine } from "../cmdline";
  import type { ProviderStatus } from "../api";
  import { remote, remoteBoard, remoteInfo as linked, type RemoteInfo } from "../remote";
  import { PROVIDER_ORDER, nameOf } from "../providerRows";
  import Switch from "./Switch.svelte";
  import "./shared.css";

  export let notify: (msg: string) => void = () => {};

  const dispatch = createEventDispatcher<{ setupRemote: void }>();

  interface Server {
    command: string;
    args?: string[];
    env?: Record<string, string>;
    enabled?: boolean;
    [k: string]: unknown;
  }

  // Well-known stdio servers. Each only fills the form; nothing installs
  // or enables itself.
  const PRESETS = [
    { name: "files", line: "npx -y @modelcontextprotocol/server-filesystem C:\\path\\to\\folder", note: "Read and write one folder" },
    { name: "memory", line: "npx -y @modelcontextprotocol/server-memory", note: "A small knowledge graph" },
    { name: "fetch", line: "uvx mcp-server-fetch", note: "Fetch web pages as text (needs uv)" },
    { name: "git", line: "uvx mcp-server-git --repository C:\\path\\to\\repo", note: "Read a git repository (needs uv)" },
    { name: "playwright", line: "npx -y @playwright/mcp@latest", note: "Drive a real browser" },
  ];

  let cfg: ParziConfig | null = null;
  let loadError = "";
  let info: RemoteInfo | null = null;
  let adding = false;
  let saving = false;
  let name = "";
  let line = "";
  let envText = "";

  $: servers = Object.entries((cfg?.mcp.servers ?? {}) as Record<string, Server>).sort(([a], [b]) => a.localeCompare(b));

  onMount(() => {
    void load();
  });

  async function load() {
    try {
      [cfg, info] = await Promise.all([api.getConfig(), remote.info()]);
      loadError = "";
    } catch (e) {
      loadError = String(e);
    }
  }

  async function save(next: ParziConfig, done: string) {
    saving = true;
    try {
      await api.saveConfig(next);
      cfg = next;
      notify(done);
      return true;
    } catch (e) {
      notify(String(e).includes("cancelled") ? "Not saved" : `Could not save: ${e}`);
      cfg = await api.getConfig().catch(() => cfg);
      return false;
    } finally {
      saving = false;
    }
  }

  function clone(): ParziConfig | null {
    return cfg ? (JSON.parse(JSON.stringify(cfg)) as ParziConfig) : null;
  }

  async function add() {
    const id = name.trim();
    const words = splitLine(line);
    if (!/^[a-z0-9][a-z0-9_-]{0,31}$/i.test(id)) {
      notify("Name it with letters, digits, - or _ (up to 32)");
      return;
    }
    if (!words.length) {
      notify("Enter the command that starts the connector");
      return;
    }
    const env = parseEnv(envText);
    if (typeof env === "string") {
      notify(env);
      return;
    }
    const next = clone();
    if (!next) return;
    if (next.mcp.servers[id]) {
      notify(`A connector called ${id} already exists`);
      return;
    }
    next.mcp.servers[id] = { command: words[0], args: words.slice(1), env, enabled: true };
    if (await save(next, `Added ${id}`)) {
      adding = false;
      name = line = envText = "";
    }
  }

  async function toggle(id: string) {
    const next = clone();
    const srv = next?.mcp.servers[id] as Server | undefined;
    if (!next || !srv) return;
    srv.enabled = srv.enabled === false;
    await save(next, `${id} ${srv.enabled ? "on" : "off"}`);
  }

  async function removeServer(id: string) {
    const ok = await ask(`Remove the connector "${id}"?`, { title: "Remove connector", kind: "warning" }).catch(() => false);
    if (!ok) return;
    const next = clone();
    if (!next) return;
    delete next.mcp.servers[id];
    await save(next, `Removed ${id}`);
  }

  function preset(p: (typeof PRESETS)[number]) {
    name = p.name;
    line = p.line;
    adding = true;
  }

  // Agents on the server: shown only after a check, since it connects.
  let agents: ProviderStatus[] = [];
  let checking = false;

  async function checkAgents(refresh: boolean) {
    checking = true;
    try {
      const all = await remote.providers(refresh);
      remoteBoard.set(all);
      agents = PROVIDER_ORDER.map((id) => all.find((a) => a.provider === id)).filter((a): a is ProviderStatus => !!a);
    } catch (e) {
      notify(`Can't reach the server: ${e}`);
    } finally {
      checking = false;
    }
  }

  async function agentAction(provider: string, action: "install" | "login") {
    try {
      await remote.agent(provider, action);
      notify(action === "install" ? "Installing in a terminal. Check again when it is done." : "Sign in in the terminal, then check again.");
    } catch (e) {
      notify(String(e));
    }
  }

  function agentState(a: ProviderStatus): string {
    if (a.state === "ready") return a.account ? `Ready · ${a.account}` : "Ready";
    if (a.state === "signed_out") return "Not signed in";
    if (a.state === "not_installed") return "Not installed";
    if (a.state === "disabled") return "Switched off";
    if (a.state === "error") return a.hint || "Error";
    return "Installed";
  }

  async function unlink() {
    if (!info) return;
    const ok = await ask(`Unlink ${info.label}? Parzi keeps running there; set it up again to link back.`, {
      title: "Unlink remote",
      kind: "warning",
    }).catch(() => false);
    if (!ok) return;
    try {
      await remote.forget();
      info = null;
      linked.set(null);
      remoteBoard.set([]);
      agents = [];
      notify("Unlinked");
    } catch (e) {
      notify(`Could not unlink: ${e}`);
    }
  }

  function when(ms: number): string {
    return ms ? new Date(ms).toLocaleDateString() : "";
  }
</script>

{#if loadError}
  <div class="load-err"><span>{loadError}</span></div>
{:else if !cfg}
  <div class="pref-section">
    <div class="skel tall" />
  </div>
{:else}
  <div class="pref-section">
    <div class="section-head-with-action">
      <div>
        <h3 class="section-title">Remote</h3>
        <p class="section-desc">Parzi on your own Linux server, reached over SSH. Sessions there keep going while this PC sleeps.</p>
      </div>
      {#if !info}
        <button class="sbtn" on:click={() => dispatch("setupRemote")}>Set up</button>
      {/if}
    </div>
    {#if info}
      <div class="field-card">
        <div class="field-info">
          <span class="field-label">{info.label}</span>
          <span class="field-hint">Parzi {info.version || "?"}{info.linked_at ? ` · linked ${when(info.linked_at)}` : ""}</span>
        </div>
        <button class="sbtn" on:click={() => dispatch("setupRemote")}>Set up again</button>
        <button class="sbtn danger" on:click={unlink}>Unlink</button>
      </div>
      <p class="section-desc">Turn on Remote in the composer to start a session there. It shows in your session list like any other.</p>
      <div class="section-head-with-action">
        <div>
          <h3 class="section-title">Agents on the server</h3>
          <p class="section-desc">Install and sign in run in a terminal you watch, with each agent's own login.</p>
        </div>
        <button class="sbtn" disabled={checking} on:click={() => checkAgents(true)}>{checking ? "Checking…" : agents.length ? "Check again" : "Check"}</button>
      </div>
      {#each agents as a (a.provider)}
        <div class="field-card">
          <div class="field-info">
            <span class="field-label">{nameOf(a.provider)}</span>
            <span class="field-hint" class:ok={a.state === "ready"}>{agentState(a)}</span>
          </div>
          {#if a.state === "not_installed"}
            <button class="sbtn" on:click={() => agentAction(a.provider, "install")}>Install</button>
          {:else if a.state !== "ready" && a.state !== "disabled"}
            <button class="sbtn" on:click={() => agentAction(a.provider, "login")}>Sign in</button>
          {/if}
        </div>
      {/each}
    {/if}
  </div>

  <div class="pref-section">
    <div class="section-head-with-action">
      <div>
        <h3 class="section-title">Connectors</h3>
        <p class="section-desc">MCP servers your agents can use. Each runs as a program on this PC, so Parzi asks before saving a new or changed one.</p>
      </div>
      <button class="sbtn" disabled={saving} on:click={() => (adding = !adding)}>{adding ? "Cancel" : "Add"}</button>
    </div>

    {#each servers as [id, srv] (id)}
      <div class="field-card">
        <div class="field-info">
          <span class="field-label">{id}</span>
          <span class="field-hint mono" title={[srv.command, ...(srv.args ?? [])].join(" ")}>{[srv.command, ...(srv.args ?? [])].join(" ")}</span>
          {#if srv.env && Object.keys(srv.env).length}
            <span class="field-hint">Sets {Object.keys(srv.env).join(", ")}</span>
          {/if}
        </div>
        <Switch on={srv.enabled !== false} title={srv.enabled !== false ? `Turn ${id} off` : `Turn ${id} on`} on:toggle={() => toggle(id)} />
        <button class="sbtn danger" disabled={saving} on:click={() => removeServer(id)}>Remove</button>
      </div>
    {:else}
      {#if !adding}<p class="section-desc">No connectors yet.</p>{/if}
    {/each}

    {#if adding}
      <div class="field-card add">
        <label>
          <span class="field-label">Start from</span>
          <select
            on:change={(e) => {
              const p = PRESETS.find((x) => x.name === e.currentTarget.value);
              if (p) preset(p);
            }}
          >
            <option value="">A known connector, or write your own below</option>
            {#each PRESETS as p (p.name)}
              <option value={p.name}>{p.name}: {p.note}</option>
            {/each}
          </select>
        </label>
        <label>
          <span class="field-label">Name</span>
          <input bind:value={name} placeholder="mail" spellcheck="false" />
        </label>
        <label>
          <span class="field-label">Command</span>
          <input class="mono" bind:value={line} placeholder="npx -y some-mcp-server --flag value" spellcheck="false" />
        </label>
        <label>
          <span class="field-label">Environment <span class="field-hint">(optional, NAME=value per line; stays on this PC)</span></span>
          <textarea class="mono" rows="2" bind:value={envText} spellcheck="false" />
        </label>
        <div class="row-end">
          <button class="sbtn" disabled={saving || !name.trim() || !line.trim()} on:click={add}>{saving ? "Saving…" : "Add connector"}</button>
        </div>
      </div>
    {/if}
  </div>
{/if}

<style>
  .field-card {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .field-hint.ok {
    color: var(--ok);
  }
  .mono {
    font-family: var(--mono);
  }
  .field-hint.mono {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .add {
    flex-direction: column;
    align-items: stretch;
    gap: 10px;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  input,
  select,
  textarea {
    background: var(--bg);
    color: var(--text);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 6px 8px;
    font-size: 12.5px;
    resize: vertical;
  }
  input:focus,
  textarea:focus {
    outline: none;
    border-color: var(--accent);
  }
  .row-end {
    display: flex;
    justify-content: flex-end;
  }
</style>
