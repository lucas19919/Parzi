<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import { ask, open as pickFile } from "@tauri-apps/plugin-dialog";
  import { api, type ParziConfig } from "../api";
  import { splitLine } from "../cmdline";
  import type { ProviderStatus } from "../api";
  import { remote, remoteBoard, remoteInfo as linked, type RemoteInfo } from "../remote";
  import { PROVIDER_ORDER, nameOf, stateLabel } from "../providerRows";
  import Switch from "./Switch.svelte";
  import Icon from "../Icon.svelte";
  import { tooltip } from "../tooltip";
  import { updateConfig } from "../config";
  import { CATALOG, hostedServer, nameFromUrl, serverLine, type CatalogConnector, type ServerCfg as Server } from "../connectorCatalog";
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
    state: "checking" | "signing" | "signin" | "ok" | "error";
    tools?: number;
    error?: string;
  }
  let health: Record<string, Health> = {};

  const SIGN_IN = "sign-in needed";

  function plainError(e: string): string {
    if (e.includes("isn't installed") && e.includes("npx")) return "Needs Node.js";
    if (/timed out/i.test(e)) return "Didn't answer in time";
    return e.replace(/^tool `[^`]*`: /, "").replace(/^`[^`]*`: /, "");
  }

  // Connects and counts its tools. One that asks to sign in, just added,
  // goes straight to its sign-in page (`signIn`).
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
      health = { ...health, [id]: { state: "error", error: plainError(String(e)) } };
    }
  }

  // Its sign-in page opens in the browser; Parzi keeps the token.
  async function signIn(id: string) {
    health = { ...health, [id]: { state: "signing" } };
    try {
      const tools = await api.connectorSignIn(id);
      health = { ...health, [id]: { state: "ok", tools } };
      void loadDetails(id);
    } catch (e) {
      health = { ...health, [id]: { state: "error", error: plainError(String(e)) } };
    }
  }

  // What each connected one says about itself: title, logo and tools.
  type Details = Awaited<ReturnType<typeof api.connectorDetails>>;
  let details: Record<string, Details> = {};
  let openTools: string | null = null;

  async function loadDetails(id: string) {
    try {
      // Waited for first: two loading at once must not drop each other.
      const d = await api.connectorDetails(id);
      details = { ...details, [id]: d };
    } catch {
      // It connected a moment ago; the row still says so.
    }
  }

  // What a tool does when the agent calls it: its own setting, else the
  // approval mode's (Settings › General).
  $: policyAuto = (cfg?.lanes.default_mode ?? "ask") === "auto";
  const effective = (mode: string) => mode || (policyAuto ? "auto" : "ask");
  const TOOL_MODES: ["auto" | "ask" | "off", string][] = [
    ["auto", "Allow"],
    ["ask", "Ask"],
    ["off", "Off"],
  ];

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

  // A logo of the person's own choosing.
  async function chooseLogo(id: string) {
    const path = await pickFile({ multiple: false, filters: [{ name: "Pictures", extensions: ["png", "jpg", "jpeg", "svg", "webp", "ico", "gif"] }] }).catch(() => null);
    if (typeof path !== "string") return;
    try {
      const icon = await api.connectorSetIcon(id, path);
      details = { ...details, [id]: { ...(details[id] ?? { title: "", tools: [] }), icon } };
    } catch (e) {
      notify(String(e), true);
    }
  }

  // Connectors set up in your other apps, offered in the add dialog.
  let found: { name: string; source: string; server: Server }[] = [];

  const openOutside = (url: string) => api.openConfirmedUrl(url).catch((e) => notify(String(e), true));

  // A directory connector: added, then connected; one that signs in opens
  // its page right away.
  async function addKnown(c: CatalogConnector, token: string) {
    const server = hostedServer(c.url, { token, auth: c.auth === "gh" ? "gh" : "" });
    if (!(await save((cf) => (cf.mcp.servers[c.id] = server), ""))) return;
    if (c.auth === "signin") void signIn(c.id);
    else void check(c.id);
  }

  // One found in another app, as it was set up there.
  async function addFound(name: string, server: Server) {
    if (cfg?.mcp.servers[name]) return notify(`A connector called ${name} already exists`, true);
    if (await save((c) => (c.mcp.servers[name] = { ...server, enabled: true }), "")) void check(name, true);
  }

  // Linked from Set up Parzi while this shows: follow it.
  $: if ($linked) info = $linked;

  onMount(() => {
    void load();
    void api
      .connectorsFound()
      .then((all) => (found = all as typeof found))
      .catch(() => {});
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

  // Anything else, pasted: a hosted server's address (it signs in itself,
  // or takes an API key), an mcpServers block (every server in it), or the
  // command that starts a local one.
  async function addCustom(a: { text: string; name: string; token: string }) {
    let list: [string, Server][];
    if (/^https?:\/\//i.test(a.text)) {
      list = [[a.name || nameFromUrl(a.text), hostedServer(a.text, { token: a.token })]];
    } else if (a.text.startsWith("{")) {
      list = (await api.connectorsFromText(a.text).catch(() => [])) as [string, Server][];
      if (!list.length) return notify("No servers in that block", true);
    } else {
      const words = splitLine(a.text);
      if (!words.length) return notify("Paste an address, a command or a config block", true);
      list = [[a.name, { command: words[0], args: words.slice(1), env: {}, enabled: true }]];
    }
    const bad = list.find(([id]) => !/^[a-z0-9][a-z0-9_-]{0,31}$/i.test(id));
    if (bad) return notify("Name it with letters, digits, - or _ (up to 32)", true);
    const taken = list.find(([id]) => cfg?.mcp.servers[id]);
    if (taken) return notify(`A connector called ${taken[0]} already exists`, true);
    if (await save((c) => list.forEach(([id, srv]) => (c.mcp.servers[id] = srv)), "")) {
      dialog = null;
      for (const [id] of list) void check(id, true);
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
    await api.connectorForget(id).catch(() => {});
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
        <button class="logo-pick" use:tooltip={"Choose a logo"} aria-label="Choose a logo for {k?.name ?? id}" on:click={() => chooseLogo(id)}>
          <ConnectorLogo id={id} name={k?.name ?? id} site={k?.site ?? ""} src={details[id]?.icon ?? null} size={32} />
        </button>
        <div class="field-info">
          <span class="field-label">{k?.name ?? id}</span>
          <span class="field-hint" class:ok={h?.state === "ok"} class:bad={h?.state === "error"} title={serverLine(srv)}>
            {#if off}
              Off
            {:else if h?.state === "checking"}
              {srv.url ? "Connecting…" : "Starting…"}
            {:else if h?.state === "signing"}
              Finish signing in in your browser…
            {:else if h?.state === "signin"}
              Needs you to sign in
            {:else if h?.state === "ok"}
              Connected
            {:else if h?.state === "error"}
              {h.error}
            {:else}
              {k?.about ?? serverLine(srv)}
            {/if}
          </span>
        </div>
        {#if h?.state === "error" && h.error === "Needs Node.js"}
          <button class="sbtn" on:click={() => openOutside("https://nodejs.org")}>Get Node.js</button>
        {:else if !off && (h?.state === "signin" || h?.state === "signing")}
          <button class="sbtn primary" disabled={h?.state === "signing"} on:click={() => signIn(id)}>{h?.state === "signing" ? "Signing in…" : "Sign in"}</button>
        {:else if !off && h?.state !== "ok"}
          <button class="sbtn" disabled={h?.state === "checking"} on:click={() => check(id)}>{h?.state === "checking" ? "Connecting…" : "Connect"}</button>
        {:else if !off && h?.state === "ok"}
          <button class="sbtn" aria-expanded={openTools === id} on:click={() => (openTools = openTools === id ? null : id)}>
            {h.tools} tool{h.tools === 1 ? "" : "s"}
            <Icon name="chevDown" size={12} />
          </button>
        {/if}
        <Switch on={!off} title={off ? `Turn ${k?.name ?? id} on` : `Turn ${k?.name ?? id} off`} on:toggle={() => toggle(id)} />
        <button class="remove" disabled={saving} use:tooltip={`Remove ${k?.name ?? id}`} aria-label="Remove {k?.name ?? id}" on:click={() => removeServer(id)}>
          <Icon name="trash" size={12} />
        </button>
      </div>
      {#if openTools === id && !off}
        <div class="tools" data-ui-list>
          {#each details[id]?.tools ?? [] as t (t.name)}
            {@const m = effective(t.mode)}
            <div class="tool" class:off={m === "off"}>
              <div class="tool-text">
                <span class="tool-name">{t.name}{#if t.read_only}<span class="tag">reads only</span>{:else if t.destructive}<span class="tag warn">can delete</span>{/if}</span>
                {#if t.description}<span class="tool-about" title={t.description}>{t.description}</span>{/if}
              </div>
              <div class="modes" role="group" aria-label="When {t.name} runs" data-ui-segmented>
                {#each TOOL_MODES as [v, label] (v)}
                  <button class:on={m === v} aria-pressed={m === v} on:click={() => setToolMode(id, t.name, v)}>{label}</button>
                {/each}
              </div>
            </div>
          {:else}
            <p class="tools-empty">{details[id] ? "It offers no tools." : "Loading its tools…"}</p>
          {/each}
        </div>
      {/if}
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
    {found}
    busy={saving}
    on:connect={(e) => void addKnown(e.detail.connector, e.detail.token)}
    on:custom={(e) => void addCustom(e.detail)}
    on:import={(e) => void addFound(e.detail.name, e.detail.server)}
    on:open={(e) => openOutside(e.detail.url)}
    on:close={() => (dialog = null)}
  />
{/if}

<style>
  .logo-pick {
    flex: none;
    display: inline-flex;
    padding: 0;
    background: none;
    border: none;
    border-radius: 25%;
    cursor: pointer;
  }
  .logo-pick:hover {
    outline: 1px solid var(--line);
    outline-offset: 2px;
  }
  .tools {
    display: flex;
    flex-direction: column;
    margin: calc(var(--space) * -1) 0 calc(var(--space) * 2);
    padding: var(--space) 0;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
  }
  .tool {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 3);
    padding: calc(var(--space) * 2) calc(var(--space) * 4);
  }
  .tool + .tool {
    border-top: 1px solid var(--line);
  }
  .tool-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space);
  }
  .tool-name {
    font-family: var(--mono);
    font-size: var(--fs-sm);
    color: var(--text);
  }
  .tool.off .tool-name {
    color: var(--muted);
  }
  .tag {
    margin-left: calc(var(--space) * 2);
    font-family: var(--font);
    font-size: var(--fs-xs);
    color: var(--muted);
  }
  .tag.warn {
    color: var(--warn);
  }
  .tool-about {
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .modes {
    flex: none;
    display: inline-flex;
    gap: var(--space);
    padding: var(--space);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--text) 5%, transparent);
  }
  .modes button {
    height: 24px;
    padding: 0 calc(var(--space) * 3);
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .modes button:hover {
    color: var(--text);
  }
  .modes button.on {
    background: var(--selected);
    color: var(--text);
  }
  .tools-empty {
    margin: calc(var(--space) * 2) calc(var(--space) * 4);
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .conn.off .field-label {
    opacity: 0.55;
  }
  .empty {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 4);
    width: 100%;
    padding: calc(var(--space) * 4) calc(var(--space) * 4);
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
    margin-left: calc(var(--space) * -2);
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
    gap: calc(var(--space) * 2);
  }
  .field-hint.ok {
    color: var(--ok);
  }
</style>
