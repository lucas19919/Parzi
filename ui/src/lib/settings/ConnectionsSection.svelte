<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { api, type ParziConfig } from "../api";
  import { parseEnv, splitLine } from "../cmdline";
  import type { ProviderStatus } from "../api";
  import { remote, remoteBoard, remoteInfo as linked, type RemoteInfo } from "../remote";
  import { PROVIDER_ORDER, nameOf, stateLabel } from "../providerRows";
  import Switch from "./Switch.svelte";
  import Icon from "../Icon.svelte";
  import { tooltip } from "../tooltip";
  import { updateConfig } from "../config";
  import { CATALOG, nameFromUrl, remoteServer, remoteUrl, type CatalogConnector, type ServerCfg as Server } from "../connectorCatalog";
  import "./shared.css";

  export let notify: (msg: string, err?: boolean) => void = () => {};

  const dispatch = createEventDispatcher<{ setupRemote: void }>();


  let cfg: ParziConfig | null = null;
  let loadError = "";
  let info: RemoteInfo | null = null;
  let adding = false;
  let saving = false;
  let name = "";
  let line = "";
  let envText = "";
  // The catalog connector whose token is being pasted.
  let tokenFor: CatalogConnector | null = null;
  let token = "";

  $: servers = Object.entries((cfg?.mcp.servers ?? {}) as Record<string, Server>).sort(([a], [b]) => a.localeCompare(b));
  $: known = new Map(CATALOG.map((c) => [c.id, c]));
  $: available = CATALOG.filter((c) => !cfg?.mcp.servers[c.id]);
  $: isUrl = /^https?:\/\//i.test(line.trim());

  // How each connector answered when it last started, this visit.
  interface Health {
    state: "checking" | "ok" | "error";
    tools?: number;
    error?: string;
  }
  let health: Record<string, Health> = {};

  function plainError(e: string): string {
    if (e.includes("isn't installed") && e.includes("npx")) return "Needs Node.js";
    if (/timed out/i.test(e)) return "Didn't answer in time. Finish the sign-in, then connect again";
    return e.replace(/^tool `[^`]*`: /, "");
  }

  // Starts it and counts its tools. A connector that signs in opens the
  // service's page in your browser the first time.
  async function check(id: string) {
    health = { ...health, [id]: { state: "checking" } };
    try {
      const tools = await api.connectorCheck(id);
      health = { ...health, [id]: { state: "ok", tools } };
    } catch (e) {
      health = { ...health, [id]: { state: "error", error: plainError(String(e)) } };
    }
  }

  const openOutside = (url: string) => api.openConfirmedUrl(url).catch((e) => notify(String(e), true));

  async function addKnown(c: CatalogConnector) {
    adding = false;
    if (c.auth === "token") {
      tokenFor = tokenFor?.id === c.id ? null : c;
      token = "";
      return;
    }
    tokenFor = null;
    if (await save((cf) => (cf.mcp.servers[c.id] = remoteServer(c.url)), "")) void check(c.id);
  }

  async function addToken() {
    const c = tokenFor;
    const t = token.trim();
    if (!c || !t) return;
    if (await save((cf) => (cf.mcp.servers[c.id] = remoteServer(c.url, t)), "")) {
      tokenFor = null;
      token = "";
      void check(c.id);
    }
  }

  // Linked from Set up Parzi while this shows: follow it.
  $: if ($linked) info = $linked;

  onMount(() => {
    void load();
    // Back from a terminal that installed or signed in an agent there.
    const onFocus = () => {
      if (info && agents.length) void checkAgents(true);
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  });

  async function load() {
    try {
      [cfg, info] = await Promise.all([api.getConfig(), remote.info()]);
      loadError = "";
      if (info) void checkAgents(false);
    } catch (e) {
      loadError = String(e);
    }
  }

  async function save(change: (c: ParziConfig) => void, done: string) {
    saving = true;
    try {
      cfg = await updateConfig(change);
      if (done) notify(done);
      return true;
    } catch (e) {
      const declined = String(e).includes("cancelled");
      notify(declined ? "Not saved" : `Could not save: ${e}`, !declined);
      cfg = await api.getConfig().catch(() => cfg);
      return false;
    } finally {
      saving = false;
    }
  }

  // Anything else: a hosted server's address (it signs in like the
  // catalog's), or the command that starts a local one.
  async function add() {
    const url = isUrl ? line.trim() : "";
    const id = name.trim() || (url ? nameFromUrl(url) : "");
    const words = url ? [] : splitLine(line);
    if (!/^[a-z0-9][a-z0-9_-]{0,31}$/i.test(id)) {
      notify("Name it with letters, digits, - or _ (up to 32)", true);
      return;
    }
    if (!url && !words.length) {
      notify("Enter an address or a command", true);
      return;
    }
    const env = url ? {} : parseEnv(envText);
    if (typeof env === "string") {
      notify(env, true);
      return;
    }
    if (cfg?.mcp.servers[id]) {
      notify(`A connector called ${id} already exists`, true);
      return;
    }
    const server: Server = url ? remoteServer(url) : { command: words[0], args: words.slice(1), env, enabled: true };
    if (await save((c) => (c.mcp.servers[id] = server), "")) {
      adding = false;
      name = line = envText = "";
      void check(id);
    }
  }

  async function toggle(id: string) {
    const on = (cfg?.mcp.servers[id] as Server | undefined)?.enabled === false;
    await save((c) => {
      const srv = c.mcp.servers[id] as Server | undefined;
      if (srv) srv.enabled = on;
    }, `${id} ${on ? "on" : "off"}`);
  }

  async function removeServer(id: string) {
    const ok = await ask(`Remove the connector "${id}"?`, { title: "Remove connector", kind: "warning" }).catch(() => false);
    if (!ok) return;
    await save((c) => delete c.mcp.servers[id], "");
    const { [id]: _, ...rest } = health;
    health = rest;
  }

  // Agents on the server: what it last reported, checked again on demand.
  let agents: ProviderStatus[] = [];
  let checking = false;

  async function checkAgents(refresh: boolean) {
    checking = true;
    try {
      const all = await remote.providers(refresh);
      remoteBoard.set(all);
      agents = PROVIDER_ORDER.map((id) => all.find((a) => a.provider === id)).filter((a): a is ProviderStatus => !!a);
    } catch (e) {
      notify(`Can't reach the server: ${e}`, true);
    } finally {
      checking = false;
    }
  }

  async function agentAction(provider: string, action: "install" | "login") {
    try {
      await remote.agent(provider, action);
      notify("Finish in the terminal window");
    } catch (e) {
      notify(String(e), true);
    }
  }

  const agentState = (a: ProviderStatus) => (a.state === "error" && a.hint ? a.hint : stateLabel(a));

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
      notify(`Could not unlink: ${e}`, true);
    }
  }

  let syncing = false;
  $: syncedLine = info?.synced_at ? `Synced ${new Date(info.synced_at).toLocaleTimeString()}` : "Not synced yet";

  async function syncNow() {
    syncing = true;
    try {
      const moved = await remote.sync();
      notify(moved ? `Synced ${moved}` : "Up to date");
      info = await remote.info();
    } catch (e) {
      notify(`Could not sync: ${e}`, true);
    } finally {
      syncing = false;
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
      <h3 class="section-title">Server</h3>
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
      <div class="field-card">
        <div class="field-info">
          <span class="field-label">Sync</span>
          <span class="field-hint">{syncedLine}</span>
        </div>
        <button class="sbtn" disabled={syncing} on:click={syncNow}>{syncing ? "Syncing…" : "Sync now"}</button>
      </div>
      <div class="section-head-with-action">
        <h3 class="section-title">Agents on the server</h3>
        <button class="sbtn" disabled={checking} on:click={() => checkAgents(true)}>{checking ? "Checking…" : "Check again"}</button>
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
    <h3 class="section-title">Connectors</h3>

    {#each servers as [id, srv] (id)}
      {@const k = known.get(id)}
      {@const h = health[id]}
      {@const off = srv.enabled === false}
      <div class="field-card conn" class:off>
        <span class="mark" aria-hidden="true">{(k?.name ?? id)[0].toUpperCase()}</span>
        <div class="field-info">
          <span class="field-label">{k?.name ?? id}</span>
          <span class="field-hint" class:ok={h?.state === "ok"} class:bad={h?.state === "error"} title={remoteUrl(srv) || [srv.command, ...(srv.args ?? [])].join(" ")}>
            {#if off}
              Off
            {:else if h?.state === "checking"}
              {remoteUrl(srv) ? "Connecting… sign in if your browser asks" : "Starting…"}
            {:else if h?.state === "ok"}
              Connected · {h.tools} tool{h.tools === 1 ? "" : "s"}
            {:else if h?.state === "error"}
              {h.error}
            {:else if !k}
              {remoteUrl(srv) || [srv.command, ...(srv.args ?? [])].join(" ")}
            {/if}
          </span>
        </div>
        {#if h?.state === "error" && h.error === "Needs Node.js"}
          <button class="sbtn" on:click={() => openOutside("https://nodejs.org")}>Get Node.js</button>
        {:else if !off && h?.state !== "ok"}
          <button class="sbtn" disabled={h?.state === "checking"} on:click={() => check(id)}>{h?.state === "checking" ? "Connecting…" : "Connect"}</button>
        {/if}
        <Switch on={!off} title={off ? `Turn ${k?.name ?? id} on` : `Turn ${k?.name ?? id} off`} on:toggle={() => toggle(id)} />
        <button class="remove" disabled={saving} use:tooltip={`Remove ${k?.name ?? id}`} aria-label="Remove {k?.name ?? id}" on:click={() => removeServer(id)}>
          <Icon name="trash" size={12} />
        </button>
      </div>
    {/each}

    <div class="catalog" data-ui-list>
      {#each available as c (c.id)}
        <button class="tile" class:on={tokenFor?.id === c.id} disabled={saving} on:click={() => addKnown(c)}>
          <span class="mark" aria-hidden="true">{c.name[0]}</span>
          <span class="tile-name">{c.name}</span>
          <Icon name="plus" size={12} />
        </button>
      {/each}
      <button
        class="tile"
        class:on={adding}
        on:click={() => {
          adding = !adding;
          tokenFor = null;
        }}
      >
        <span class="mark" aria-hidden="true"><Icon name="plus" size={12} /></span>
        <span class="tile-name">Other</span>
      </button>
    </div>

    {#if tokenFor}
      <div class="field-card add">
        <label>
          <span class="field-label">{tokenFor.name} {tokenFor.tokenName ?? "token"}</span>
          <input
            type="password"
            bind:value={token}
            placeholder="Paste it here"
            autocomplete="off"
            spellcheck="false"
            on:keydown={(e) => e.key === "Enter" && addToken()}
          />
        </label>
        <div class="row-end">
          {#if tokenFor.tokenUrl}
            <button class="sbtn" on:click={() => tokenFor?.tokenUrl && openOutside(tokenFor.tokenUrl)}>Create one</button>
          {/if}
          <button class="sbtn" disabled={saving || !token.trim()} on:click={addToken}>Connect</button>
        </div>
      </div>
    {/if}

    {#if adding}
      <div class="field-card add">
        <label>
          <span class="field-label">Address or command</span>
          <input class="mono" bind:value={line} placeholder="https://example.com/mcp, or npx -y some-mcp-server" spellcheck="false" />
        </label>
        <label>
          <span class="field-label">Name</span>
          <input bind:value={name} placeholder={isUrl ? nameFromUrl(line.trim()) : "mail"} spellcheck="false" />
        </label>
        {#if line.trim() && !isUrl}
          <label>
            <span class="field-label">Environment <span class="field-hint">(optional, NAME=value per line)</span></span>
            <textarea class="mono" rows="2" bind:value={envText} spellcheck="false" />
          </label>
        {/if}
        <div class="row-end">
          <button class="sbtn" disabled={saving || !line.trim()} on:click={add}>{saving ? "Adding…" : "Add"}</button>
        </div>
      </div>
    {/if}
  </div>
{/if}

<style>
  .conn .field-hint {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .field-hint.bad {
    color: var(--bad);
  }
  .conn.off .mark,
  .conn.off .field-label {
    opacity: 0.55;
  }
  .mark {
    width: 28px;
    height: 28px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--text) 8%, transparent);
    color: var(--text);
    font-size: var(--fs-md);
    font-weight: 650;
  }
  .remove {
    width: 28px;
    height: 28px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .remove:hover:not(:disabled) {
    background: var(--line);
    color: var(--bad);
  }
  /* The catalog: one click adds a connector. */
  .catalog {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: 8px;
  }
  .tile {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 44px;
    padding: 8px 12px 8px 8px;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-md);
    text-align: left;
    cursor: pointer;
  }
  .tile:hover:not(:disabled),
  .tile.on {
    border-color: var(--muted);
    color: var(--text);
  }
  .tile:disabled {
    cursor: default;
    opacity: 0.6;
  }
  .tile .mark {
    width: 26px;
    height: 26px;
  }
  .tile-name {
    flex: 1;
    min-width: 0;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
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
  textarea {
    background: var(--bg);
    color: var(--text);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 6px 8px;
    font-size: var(--fs-md);
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
    gap: 8px;
  }
</style>
