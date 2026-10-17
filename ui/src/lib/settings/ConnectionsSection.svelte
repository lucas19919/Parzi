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
  import ConnectorLogo from "../ConnectorLogo.svelte";
  import ConnectorDialog from "./ConnectorDialog.svelte";
  import "./shared.css";

  export let notify: (msg: string, err?: boolean) => void = () => {};

  const dispatch = createEventDispatcher<{ setupRemote: void }>();


  let cfg: ParziConfig | null = null;
  let loadError = "";
  let info: RemoteInfo | null = null;
  let saving = false;
  // The add dialog, open on its directory or its custom form.
  let dialog: "browse" | "custom" | null = null;

  $: servers = Object.entries((cfg?.mcp.servers ?? {}) as Record<string, Server>).sort(([a], [b]) => a.localeCompare(b));
  $: known = new Map(CATALOG.map((c) => [c.id, c]));
  $: added = new Set(Object.keys(cfg?.mcp.servers ?? {}));

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

  // A directory connector: added (with its token, if it takes one), then
  // started, which signs in the first time.
  async function addKnown(c: CatalogConnector, token: string) {
    if (await save((cf) => (cf.mcp.servers[c.id] = remoteServer(c.url, token)), "")) void check(c.id);
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
  // directory's, or takes an API key), or the command that starts a local one.
  async function addCustom(a: { name: string; url: string; token: string; command: string; env: string }) {
    const url = a.url;
    const id = a.name || (url ? nameFromUrl(url) : "");
    const words = url ? [] : splitLine(a.command);
    if (!/^[a-z0-9][a-z0-9_-]{0,31}$/i.test(id)) {
      notify("Name it with letters, digits, - or _ (up to 32)", true);
      return;
    }
    if (!url && !words.length) {
      notify("Enter an address or a command", true);
      return;
    }
    const env = url ? {} : parseEnv(a.env);
    if (typeof env === "string") {
      notify(env, true);
      return;
    }
    if (cfg?.mcp.servers[id]) {
      notify(`A connector called ${id} already exists`, true);
      return;
    }
    const server: Server = url ? remoteServer(url, a.token) : { command: words[0], args: words.slice(1), env, enabled: true };
    if (await save((c) => (c.mcp.servers[id] = server), "")) {
      dialog = null;
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
      <h3 class="section-title">Remote</h3>
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
        <h3 class="section-title">Agents on the remote</h3>
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
    <div class="section-head-with-action">
      <h3 class="section-title">Connectors</h3>
      <button class="sbtn add-conn" on:click={() => (dialog = "browse")}><Icon name="plus" size={12} /><span>Add connector</span></button>
    </div>

    {#each servers as [id, srv] (id)}
      {@const k = known.get(id)}
      {@const h = health[id]}
      {@const off = srv.enabled === false}
      <div class="field-card conn" class:off>
        <ConnectorLogo id={id} name={k?.name ?? id} site={k?.site ?? (remoteUrl(srv) ? new URL(remoteUrl(srv)).origin : "")} size={32} />
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
            {:else}
              {k?.about ?? (remoteUrl(srv) || [srv.command, ...(srv.args ?? [])].join(" "))}
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
    {:else}
      <button class="empty" on:click={() => (dialog = "browse")}>
        <span class="faces">
          {#each CATALOG.slice(0, 5) as c (c.id)}
            <ConnectorLogo id={c.id} name={c.name} site={c.site} size={28} />
          {/each}
        </span>
        <span class="empty-text">Give your agents GitHub, Notion, Linear, Higgsfield and more</span>
        <Icon name="chevRight" size={12} />
      </button>
    {/each}
  </div>
{/if}

{#if dialog}
  <ConnectorDialog
    start={dialog}
    {added}
    {health}
    busy={saving}
    on:connect={(e) => void addKnown(e.detail.connector, e.detail.token)}
    on:custom={(e) => void addCustom(e.detail)}
    on:open={(e) => openOutside(e.detail.url)}
    on:close={() => (dialog = null)}
  />
{/if}

<style>
  .conn.off .field-label {
    opacity: 0.55;
  }
  .empty {
    display: flex;
    align-items: center;
    gap: 14px;
    width: 100%;
    padding: 14px 16px;
    background: var(--panel);
    border: 1px dashed var(--line);
    border-radius: var(--radius-lg);
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-md);
    text-align: left;
    cursor: pointer;
  }
  .empty:hover {
    border-color: var(--muted);
    color: var(--text);
  }
  .faces {
    display: inline-flex;
    flex: none;
  }
  .faces > :global(* + *) {
    margin-left: -6px;
  }
  .empty-text {
    flex: 1;
    min-width: 0;
  }
  .conn .field-hint {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .field-hint.bad {
    color: var(--bad);
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
  .field-card {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .field-hint.ok {
    color: var(--ok);
  }
</style>
