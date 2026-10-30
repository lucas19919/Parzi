<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import { ask, open as pickFile } from "@tauri-apps/plugin-dialog";
  import Icon from "../Icon.svelte";
  import { tooltip } from "../tooltip";
  import { api, type ParziConfig } from "../api";
  import type { ProviderStatus } from "../api";
  import { remote, remoteBoard, remoteInfo as linked, type RemoteInfo } from "../remote";
  import { PROVIDER_ORDER, nameOf, stateLabel } from "../providerRows";
  import SegControl from "./SegControl.svelte";
  import ConnectMcp from "./ConnectMcp.svelte";
  import { updateConfig } from "../config";
  import { CATALOG, type ServerCfg as Server } from "../connectorCatalog";
  import ConnectorLogo from "../ConnectorLogo.svelte";
  import "./shared.css";

  export let notify: (msg: string, err?: boolean) => void = () => {};
  export let which: "apps" | "remote" = "apps";

  const dispatch = createEventDispatcher<{ setupRemote: void }>();

  let cfg: ParziConfig | null = null;
  let loadError = "";
  let info: RemoteInfo | null = null;
  let saving = false;
  let connectOpen = false;

  interface Health {
    state: "checking" | "signing" | "signin" | "ok" | "error";
    tools?: number;
    error?: string;
    raw?: string;
  }
  let health: Record<string, Health> = {};

  const SIGN_IN = "sign-in needed";

  function failureOf(raw: string): { text: string; title: string } {
    if (raw.includes("isn't installed") && raw.includes("npx")) return { text: "Needs Node.js", title: raw };
    if (/timed out/i.test(raw)) return { text: "Didn't answer in time", title: raw };
    const clean = raw.replace(/^tool `[^`]*`: /, "").replace(/^`[^`]*`: /, "").split("\n")[0].trim();
    const text = clean.length > 80 ? `${clean.slice(0, 77)}...` : clean;
    return { text: text || "Couldn't connect", title: raw };
  }

  async function check(id: string, signInIfAsked = false) {
    health = { ...health, [id]: { state: "checking" } };
    try {
      const tools = await api.connectorCheck(id);
      health = { ...health, [id]: { state: "ok", tools } };
      void loadDetails(id);
    } catch (e) {
      if (String(e).includes(SIGN_IN)) {
        health = { ...health, [id]: { state: "signin" } };
        if (signInIfAsked) void signIn(id);
        return;
      }
      const fail = failureOf(String(e));
      health = { ...health, [id]: { state: "error", error: fail.text, raw: fail.title } };
    }
  }

  async function signIn(id: string) {
    health = { ...health, [id]: { state: "signing" } };
    try {
      const tools = await api.connectorSignIn(id);
      health = { ...health, [id]: { state: "ok", tools } };
      void loadDetails(id);
    } catch (e) {
      const fail = failureOf(String(e));
      health = { ...health, [id]: { state: "error", error: fail.text, raw: fail.title } };
    }
  }

  type Details = Awaited<ReturnType<typeof api.connectorDetails>>;
  let details: Record<string, Details> = {};

  async function loadDetails(id: string) {
    try {
      const d = await api.connectorDetails(id);
      details = { ...details, [id]: d };
    } catch {
    }
  }

  $: policyAuto = (cfg?.lanes.default_mode ?? "ask") === "auto";
  const effective = (mode: string) => mode || (policyAuto ? "auto" : "ask");
  const TOOL_OPTIONS = [
    { value: "ask", label: "Ask" },
    { value: "auto", label: "Auto" },
    { value: "off", label: "Off" },
  ];

  function pickMode(id: string, tool: string, mode: string) {
    if (mode !== "ask" && mode !== "auto" && mode !== "off") return;
    void setToolMode(id, tool, mode);
  }

  async function setToolMode(id: string, tool: string, mode: "auto" | "ask" | "off") {
    const ok = await save((c) => {
      const srv = c.mcp.servers[id] as Server & { deny?: string[]; tool_modes?: Record<string, string> };
      if (!srv) return;
      srv.deny = (srv.deny ?? []).filter((t) => t !== tool);
      srv.tool_modes = { ...(srv.tool_modes ?? {}) };
      delete srv.tool_modes[tool];
      if (mode === "off") srv.deny.push(tool);
      else srv.tool_modes[tool] = mode;
    }, "");
    if (ok && details[id]) {
      details = { ...details, [id]: { ...details[id], tools: details[id].tools.map((t) => (t.name === tool ? { ...t, mode } : t)) } };
    }
  }

  interface AppRow {
    id: string;
    name: string;
    site: string;
    server: Server;
  }

  // Only connected connectors live here now; connecting happens in the modal.
  function appList(config: ParziConfig | null, titles: Record<string, string>): AppRow[] {
    const map = (config?.mcp.servers ?? {}) as Record<string, Server>;
    const knownIds = new Set(CATALOG.map((c) => c.id));
    const rows: AppRow[] = [];
    for (const c of CATALOG) {
      if (map[c.id]) rows.push({ id: c.id, name: c.name, site: c.site, server: map[c.id] });
    }
    for (const [id, server] of Object.entries(map).sort(([a], [b]) => a.localeCompare(b))) {
      if (knownIds.has(id)) continue;
      rows.push({ id, name: titles[id] || id, site: "", server });
    }
    return rows;
  }

  $: titles = Object.fromEntries(Object.entries(details).map(([id, d]) => [id, d.title || ""]));
  $: rows = which === "apps" ? appList(cfg, titles) : [];
  const catalogIds = new Set(CATALOG.map((c) => c.id));
  $: mcpRows = rows.filter((row) => catalogIds.has(row.id));
  $: customRows = rows.filter((row) => !catalogIds.has(row.id));
  $: groups = [
    { id: "mcp" as const, rows: mcpRows },
    { id: "custom" as const, rows: customRows },
  ];

  const openOutside = (url: string) => api.openConfirmedUrl(url).catch((e) => notify(String(e), true));

  function reloadAfterAdd() {
    connectOpen = false;
    void load();
  }

  $: if ($linked) info = $linked;

  onMount(() => {
    void load();
    const onFocus = () => {
      if (which === "remote" && info && agents.length) void checkAgents(true);
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  });

  async function load() {
    try {
      [cfg, info] = await Promise.all([api.getConfig(), remote.info()]);
      loadError = "";
      if (which === "remote" && info) void checkAgents(false);
      if (which === "apps" && cfg) {
        for (const [id, srv] of Object.entries(cfg.mcp.servers)) {
          if ((srv as Server).enabled !== false) void check(id);
        }
      }
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

  async function toggle(id: string) {
    const on = (cfg?.mcp.servers[id] as Server | undefined)?.enabled === false;
    await save((c) => {
      const srv = c.mcp.servers[id] as Server | undefined;
      if (srv) srv.enabled = on;
    }, `${id} ${on ? "on" : "off"}`);
  }

  let openTools: string | null = null;
  function toggleTools(id: string) {
    openTools = openTools === id ? null : id;
  }

  async function chooseLogo(id: string) {
    const picked = await pickFile({ multiple: false, filters: [{ name: "Pictures", extensions: ["png", "jpg", "jpeg", "svg", "ico", "gif", "webp"] }] }).catch(() => null);
    if (typeof picked !== "string" || !picked) return;
    try {
      const url = await api.connectorSetIcon(id, picked);
      details = { ...details, [id]: { ...(details[id] ?? { title: "", tools: [] }), icon: url } };
    } catch (e) {
      notify(`Could not set the logo: ${e}`, true);
    }
  }

  // Always visible: first click arms it, second click removes.
  let delConn: string | null = null;
  async function removeServer(id: string) {
    if (delConn !== id) {
      delConn = id;
      return;
    }
    delConn = null;
    await save((c) => delete c.mcp.servers[id], "");
    await api.connectorForget(id).catch(() => {});
    const { [id]: _, ...rest } = health;
    health = rest;
  }

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
{:else if which === "remote"}
  <div class="pref-section">
    <div class="rows">
      <div class="row row-title"><span>Remote</span></div>
      {#if info}
        <div class="row">
          <span class="row-label">{info.label}
            <small>Parzi {info.version || "?"}{
                info.linked_at ? ` · linked ${when(info.linked_at)}` : ""
              }</small>
          </span>
          <span class="row-ctl auto">
            <button class="sbtn" on:click={() => dispatch("setupRemote")}>Set up again</button>
            <button class="sbtn danger" on:click={unlink}>Unlink</button>
          </span>
        </div>
        <div class="row">
          <span class="row-label">Sync<small>{syncedLine}</small></span>
          <span class="row-ctl auto"><button class="sbtn" disabled={syncing} on:click={syncNow}>{syncing ? "Syncing…" : "Sync now"}</button></span>
        </div>
      {:else}
        <div class="row">
          <span class="row-label">Not linked</span>
          <span class="row-ctl auto"><button class="sbtn" on:click={() => dispatch("setupRemote")}>Set up</button></span>
        </div>
      {/if}
    </div>
    {#if info}
      <div class="rows">
        <div class="row row-title">
          <span>Agents on the remote</span>
          <span class="row-ctl auto"><button class="sbtn" disabled={checking} on:click={() => checkAgents(true)}>{checking ? "Checking…" : "Check again"}</button></span>
        </div>
        {#each agents as a (a.provider)}
          <div class="row">
            <span class="row-label">{nameOf(a.provider)}
              <small class:ok={a.state === "ready"}>{agentState(a)}</small>
            </span>
            <span class="row-ctl auto">
              {#if a.state === "not_installed"}
                <button class="sbtn" on:click={() => agentAction(a.provider, "install")}>Install</button>
              {:else if a.state !== "ready" && a.state !== "disabled"}
                <button class="sbtn" on:click={() => agentAction(a.provider, "login")}>Sign in</button>
              {/if}
            </span>
          </div>
        {/each}
      </div>
    {/if}
  </div>

{:else}
  <div class="pref-section">
    {#each groups as group (group.id)}
      <div class="section-head-with-action">
        <h3 class="section-title">{group.id === "mcp" ? "MCPs" : "Connectors"}</h3>
        <button class="sbtn connect-mcp" on:click={() => (connectOpen = true)}>Connect</button>
      </div>
      {#if !group.rows.length}
        <p class="section-desc">None yet.</p>
      {/if}
      {#if group.rows.length}
        {#each group.rows as row (row.id)}
          {@const h = health[row.id]}
          {@const off = !!row.server && row.server.enabled === false}
          {@const open = openTools === row.id}
          {@const tools = details[row.id]?.tools ?? []}
          <div class="rows" class:off={off}>
            <div class="row row-title">
              <button class="logo-pick" use:tooltip={"Choose a logo"} aria-label="Choose a logo for {row.name}" on:click={() => chooseLogo(row.id)}>
                <ConnectorLogo id={row.id} name={row.name} site={row.site} src={details[row.id]?.icon ?? null} size={20} />
              </button>
              <span title={h?.raw ?? ""}>{row.name}</span>
              <span class="row-ctl auto">
                {#if !off && h?.error === "Needs Node.js"}
                  <button class="sbtn" on:click={() => openOutside("https://nodejs.org")}>Get Node.js</button>
                {:else if !off && (h?.state === "signin" || h?.state === "signing")}
                  <button class="sbtn primary" disabled={h?.state === "signing"} on:click={() => signIn(row.id)}>{h?.state === "signing" ? "Signing in…" : "Sign in"}</button>
                {:else if !off && h?.state === "error"}
                  <button class="sbtn" on:click={() => check(row.id, true)}>Try again</button>
                {/if}
                <button class="sbtn" title={off ? `Connect ${row.name}` : `Disconnect ${row.name}`} on:click={() => toggle(row.id)}>{off ? "Disconnected" : "Connected"}</button>
                <button class="icon-btn" class:armed={delConn === row.id} use:tooltip={delConn === row.id ? "Click again to remove" : `Remove ${row.name}`} aria-label={delConn === row.id ? `Confirm remove ${row.name}` : `Remove ${row.name}`} on:click={() => removeServer(row.id)}>
                  <Icon name="trash" size={12} />
                </button>
              </span>
            </div>
            <button class="row as-toggle" aria-expanded={open} on:click={() => toggleTools(row.id)}>
              <span class="disclosure"><Icon name={open ? "chevDown" : "chevRight"} size={12} /></span>
              <span>Tools{#if tools.length} · {tools.length}{/if}</span>
            </button>
            {#if open}
              <div class="details">
                {#each tools as t (t.name)}
                  {@const m = effective(t.mode)}
                  <div class="row" class:off={m === "off"}>
                    <span class="row-label" title={t.description || t.name}>{t.name}</span>
                    <SegControl label={t.name} options={TOOL_OPTIONS} value={m} on:pick={(e) => pickMode(row.id, t.name, e.detail)} />
                  </div>
                {:else}
                  <div class="row"><span class="row-label">{#if h}Connect it to see its tools.{:else}Not connected yet.{/if}</span></div>
                {/each}
              </div>
            {/if}
          </div>
        {/each}
      {/if}
    {/each}
  </div>
  {#if connectOpen}
    <ConnectMcp {notify} on:close={() => (connectOpen = false)} on:added={reloadAfterAdd} />
  {/if}
{/if}

<style>
  .rows.off .row:first-child > :not(.row-ctl),
  .rows.off .as-toggle {
    opacity: 0.55;
  }
  .logo-pick {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 0;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  .row.off .row-label {
    color: var(--muted);
  }
  .as-toggle {
    color: var(--muted);
  }
  .as-toggle:hover {
    color: var(--text);
  }
  .disclosure {
    display: inline-flex;
    flex: none;
  }
</style>
