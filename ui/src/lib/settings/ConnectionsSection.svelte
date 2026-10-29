<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { api, type ParziConfig } from "../api";
  import { classifyPaste, nameFromProgram, splitLine } from "../cmdline";
  import type { ProviderStatus } from "../api";
  import { remote, remoteBoard, remoteInfo as linked, type RemoteInfo } from "../remote";
  import { PROVIDER_ORDER, nameOf, stateLabel } from "../providerRows";
  import SegControl from "./SegControl.svelte";
  import Switch from "./Switch.svelte";
  import { updateConfig } from "../config";
  import { CATALOG, hostedServer, nameFromUrl, type CatalogConnector, type ServerCfg as Server } from "../connectorCatalog";
  import ConnectorLogo from "../ConnectorLogo.svelte";
  import "./shared.css";

  export let notify: (msg: string, err?: boolean) => void = () => {};
  export let which: "apps" | "remote" = "apps";

  const dispatch = createEventDispatcher<{ setupRemote: void }>();

  let cfg: ParziConfig | null = null;
  let loadError = "";
  let info: RemoteInfo | null = null;
  let saving = false;
  let adding = false;
  let pasted = "";
  let customName = "";

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

  let found: { name: string; source: string; server: Server }[] = [];

  interface AppRow {
    id: string;
    name: string;
    site: string;
    about: string;
    kind: "found" | "ready" | "added";
    server?: Server;
  }

  function appList(config: ParziConfig | null, hits: typeof found, titles: Record<string, string>): AppRow[] {
    const map = (config?.mcp.servers ?? {}) as Record<string, Server>;
    const have = new Set(Object.keys(map));
    const knownIds = new Set(CATALOG.map((c) => c.id));
    const rows: AppRow[] = [];
    for (const c of CATALOG) {
      if (have.has(c.id)) rows.push({ id: c.id, name: c.name, site: c.site, about: c.about, kind: "added", server: map[c.id] });
      else {
        const hit = hits.find((f) => f.name === c.id);
        if (hit) rows.push({ id: c.id, name: c.name, site: c.site, about: "Already on this computer", kind: "found", server: hit.server });
        else rows.push({ id: c.id, name: c.name, site: c.site, about: c.about, kind: "ready" });
      }
    }
    for (const f of hits) {
      if (have.has(f.name) || knownIds.has(f.name)) continue;
      let site = "";
      try {
        site = f.server.url ? new URL(f.server.url).origin : "";
      } catch {
        site = "";
      }
      rows.push({ id: f.name, name: f.name, site, about: "Already on this computer", kind: "found", server: f.server });
    }
    for (const [id, server] of Object.entries(map).sort(([a], [b]) => a.localeCompare(b))) {
      if (knownIds.has(id)) continue;
      rows.push({ id, name: titles[id] || id, site: "", about: "", kind: "added", server });
    }
    return rows;
  }

  $: titles = Object.fromEntries(Object.entries(details).map(([id, d]) => [id, d.title || ""]));
  $: rows = which === "apps" ? appList(cfg, found, titles) : [];
  const catalogIds = new Set(CATALOG.map((c) => c.id));
  $: mcpRows = rows.filter((row) => catalogIds.has(row.id));
  $: customRows = rows.filter((row) => !catalogIds.has(row.id));
  $: groups = [
    { id: "mcp" as const, rows: mcpRows },
    { id: "custom" as const, rows: customRows },
  ];

  const openOutside = (url: string) => api.openConfirmedUrl(url).catch((e) => notify(String(e), true));

  function connectRow(row: AppRow) {
    if (row.kind === "found" && row.server) void addFound(row.id, row.server);
    else if (row.kind === "ready") {
      const c = CATALOG.find((item) => item.id === row.id);
      if (c) void addKnown(c);
    }
  }

  async function addKnown(c: CatalogConnector) {
    const server = hostedServer(c.url, { auth: c.auth === "gh" ? "gh" : "" });
    if (!(await save((cf) => (cf.mcp.servers[c.id] = server), ""))) return;
    if (c.auth === "signin") void signIn(c.id);
    else void check(c.id);
  }

  async function addFound(name: string, server: Server) {
    if (cfg?.mcp.servers[name]) return notify(`${name} is already here`, true);
    if (await save((c) => (c.mcp.servers[name] = { ...server, enabled: true }), "")) void check(name, true);
  }

  function freeName(base: string): string {
    const have = cfg?.mcp.servers ?? {};
    let id = base.slice(0, 32);
    if (!have[id]) return id;
    let n = 2;
    while (have[`${id.slice(0, 28)}${n}`]) n += 1;
    return `${id.slice(0, 28)}${n}`.slice(0, 32);
  }

  const NAME_OK = /^[a-z0-9][a-z0-9_-]{0,31}$/i;

  function slugId(raw: string): string {
    let id = raw.trim().toLowerCase().replace(/[^a-z0-9_-]/g, "");
    if (!id) return "";
    if (!/^[a-z0-9]/.test(id)) id = `app${id}`;
    id = id.slice(0, 32);
    return NAME_OK.test(id) ? id : "";
  }

  function chosenName(fallback: string): string | null {
    const raw = customName.trim();
    if (!raw) return freeName(fallback || "app");
    const id = slugId(raw);
    if (!id) return null;
    return freeName(id);
  }

  async function addPasted() {
    const text = pasted.trim();
    const kind = classifyPaste(text);
    if (!kind) return;
    let list: [string, Server][];
    if (kind === "link") {
      const id = chosenName(nameFromUrl(text));
      if (!id) return notify("Use letters and numbers for the name", true);
      list = [[id, hostedServer(text)]];
    } else if (kind === "block") {
      list = ((await api.connectorsFromText(text).catch(() => [])) as [string, Server][]).filter(([id]) => NAME_OK.test(id));
      if (!list.length) return notify("That did not include a connector", true);
    } else {
      const words = splitLine(text);
      if (!words.length) return notify("Paste a link, a config block, or a command", true);
      const id = chosenName(nameFromProgram(text));
      if (!id) return notify("Use letters and numbers for the name", true);
      list = [[id, { command: words[0], args: words.slice(1), env: {}, enabled: true }]];
    }
    const fresh = list.filter(([id]) => !cfg?.mcp.servers[id]);
    if (!fresh.length) return notify(list.length === 1 ? `${list[0][0]} is already here` : "Those are already here", true);
    if (await save((c) => fresh.forEach(([id, srv]) => (c.mcp.servers[id] = srv)), "")) {
      pasted = "";
      customName = "";
      adding = false;
      for (const [id] of fresh) void check(id, true);
    }
  }

  $: if ($linked) info = $linked;

  onMount(() => {
    void load();
    if (which === "apps") {
      void api
        .connectorsFound()
        .then((all) => (found = all as typeof found))
        .catch(() => {});
    }
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

  async function removeServer(id: string, label = id) {
    const ok = await ask(`Remove ${label}?`, { title: "Remove", kind: "warning" }).catch(() => false);
    if (!ok) return;
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
      {#if group.id === "mcp"}
        <h3 class="section-title">MCPs</h3>
      {:else}
        <div class="section-head-with-action">
          <h3 class="section-title">Connectors</h3>
          <button class="sbtn" aria-expanded={adding} on:click={() => (adding = !adding)}>Add custom</button>
        </div>
        {#if adding}
          <div class="paste">
            <input class="tin" bind:value={customName} placeholder="Name (optional)" aria-label="Name" spellcheck="false" />
            <textarea class="tin" rows="4" bind:value={pasted} placeholder="Paste a link, a config block, or a command" aria-label="Paste a link, a config block, or a command" spellcheck="false"></textarea>
            <button class="sbtn primary" disabled={saving || !pasted.trim()} on:click={() => void addPasted()}>Add</button>
          </div>
        {/if}
        {#if !group.rows.length && !adding}
          <p class="section-desc">None yet.</p>
        {/if}
      {/if}
      {#if group.rows.length}
      <div class={group.id === "mcp" ? "mcp-grid" : "stack"}>
        {#each group.rows as row (row.kind + row.id)}
          {@const h = health[row.id]}
          {@const off = row.server?.enabled === false}
          {@const sentence = row.kind !== "added" ? "" : off ? "Off" : h?.state === "checking" ? "Connecting…" : h?.state === "signing" ? "Finish signing in in your browser" : h?.state === "signin" ? "Sign in to use it" : h?.state === "ok" ? "Connected" : h?.state === "error" ? (h.error ?? "Couldn't connect") : "Added"}
          <div class="app-block">
            <div class="field-card" class:off={row.kind === "added" && off}>
              <span class="mark"><ConnectorLogo id={row.id} name={row.name} site={row.site} src={details[row.id]?.icon ?? null} size={28} /></span>
              <div class="field-info">
                <span class="field-label">{row.name}</span>
                {#if sentence}<span class="field-hint" class:ok={!off && h?.state === "ok"} class:bad={!off && h?.state === "error"} title={h?.raw ?? ""}>{sentence}</span>{/if}
              </div>
              {#if row.kind !== "added"}
                <button class="sbtn" disabled={saving} on:click={() => connectRow(row)}>{row.kind === "found" ? "Add" : "Connect"}</button>
              {:else if !off && h?.error === "Needs Node.js"}
                <button class="sbtn" on:click={() => openOutside("https://nodejs.org")}>Get Node.js</button>
              {:else if !off && (h?.state === "signin" || h?.state === "signing")}
                <button class="sbtn primary" disabled={h?.state === "signing"} on:click={() => signIn(row.id)}>{h?.state === "signing" ? "Signing in…" : "Sign in"}</button>
              {:else if !off && h?.state === "error"}
                <button class="sbtn" on:click={() => check(row.id, true)}>Try again</button>
              {:else if h?.state === "ok" || off || !h}
                <Switch on={!off} title={off ? `Turn ${row.name} on` : `Turn ${row.name} off`} on:toggle={() => toggle(row.id)} />
              {/if}
            </div>
            {#if row.kind === "added"}
              <details class="can">
                <summary>Tools</summary>
                <div class="rows">
                  {#each details[row.id]?.tools ?? [] as t (t.name)}
                    {@const m = effective(t.mode)}
                    <div class="row" class:off={m === "off"}>
                      <span class="row-label" title={t.name}>{t.name}</span>
                      <SegControl label={t.name} options={TOOL_OPTIONS} value={m} on:pick={(e) => pickMode(row.id, t.name, e.detail)} />
                    </div>
                  {:else}
                    <p class="section-desc">{h?.state === "ok" ? (details[row.id] ? "No tools." : "Loading…") : "Connect it to see its tools."}</p>
                  {/each}
                </div>
                <button class="remove" disabled={saving} on:click={() => removeServer(row.id, row.name)}>Remove</button>
              </details>
            {/if}
          </div>
        {/each}
      </div>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .mcp-grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: calc(var(--space) * 3);
    align-items: stretch;
  }
  .stack {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
  }
  .app-block {
    display: flex;
    flex-direction: column;
    gap: var(--space);
    min-width: 0;
  }
  .mcp-grid .field-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mark {
    flex: none;
    display: inline-flex;
  }
  .field-card.off .field-label {
    opacity: 0.55;
  }
  .field-hint {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .field-hint.ok {
    color: var(--ok);
  }
  .field-hint.bad {
    color: var(--bad);
  }
  .can {
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
  }
  .can summary {
    padding: calc(var(--space) * 3) calc(var(--space) * 4);
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-md);
    font-weight: 500;
    cursor: pointer;
  }
  .can[open] summary {
    color: var(--text);
  }
  .can .rows {
    border: none;
    border-radius: 0;
    background: transparent;
    border-top: 1px solid var(--line);
  }
  .can .section-desc {
    padding: calc(var(--space) * 2) calc(var(--space) * 4);
  }
  .row.off .row-label {
    color: var(--muted);
  }
  .remove {
    margin: calc(var(--space) * 2) calc(var(--space) * 4) calc(var(--space) * 3);
    padding: 0;
    background: none;
    border: none;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-md);
    font-weight: 500;
    cursor: pointer;
  }
  .remove:hover:not(:disabled) {
    color: var(--bad);
  }
  .paste {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: calc(var(--space) * 2);
    width: 100%;
  }
  .paste .tin,
  .paste textarea {
    width: 100%;
  }
  .paste textarea {
    min-height: 88px;
    resize: vertical;
  }
</style>
