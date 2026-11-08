<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import Icon from "../Icon.svelte";
  import { api, type ParziConfig } from "../api";
  import { classifyPaste, nameFromProgram, splitLine } from "../cmdline";
  import { remote, remoteBoard, remoteInfo as linked, type RemoteInfo } from "../remote";
  import SegControl from "./SegControl.svelte";
  import Switch from "./Switch.svelte";
  import { updateConfig } from "../config";
  import { CATALOG, nameFromUrl, remoteFromPaste, serverOf, withBearer, type CatalogConnector, type ServerCfg as Server } from "../connectorCatalog";
  import ConnectorLogo from "../ConnectorLogo.svelte";
  import "./shared.css";

  export let notify: (msg: string, err?: boolean) => void = () => {};
  export let which: "apps" | "remote" = "apps";

  const dispatch = createEventDispatcher<{ setupRemote: void }>();

  let cfg: ParziConfig | null = null;
  let loadError = "";
  let info: RemoteInfo | null = null;
  let saving = false;
  let customName = "";
  let pasted = "";
  let bearer = "";
  let ownOpen = false;
  let tokenDrafts: Record<string, string> = {};

  let found: { name: string; source: string; server: Server }[] = [];

  interface Health {
    state: "checking" | "signing" | "signin" | "ok" | "error";
    tools?: number;
    error?: string;
    raw?: string;
  }
  let health: Record<string, Health> = {};

  const SIGN_IN = "sign-in needed";
  const NEEDS: Record<string, string> = { "Needs Node.js": "https://nodejs.org", "Needs uv": "https://docs.astral.sh/uv/" };

  function failureOf(raw: string): { text: string; title: string } {
    if (raw.includes("isn't installed") && raw.includes("npx")) return { text: "Needs Node.js", title: raw };
    if (raw.includes("isn't installed") && raw.includes("uvx")) return { text: "Needs uv", title: raw };
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
    about: string;
    site: string;
    server: Server;
  }

  // Only connected connectors live here now; connecting happens in the modal.
  function appList(config: ParziConfig | null, titles: Record<string, string>): AppRow[] {
    const map = (config?.mcp.servers ?? {}) as Record<string, Server>;
    const knownIds = new Set(CATALOG.map((c) => c.id));
    const rows: AppRow[] = [];
    for (const c of CATALOG) {
      if (map[c.id]) rows.push({ id: c.id, name: c.name, about: c.about, site: c.site, server: map[c.id] });
    }
    for (const [id, server] of Object.entries(map).sort(([a], [b]) => a.localeCompare(b))) {
      if (knownIds.has(id)) continue;
      rows.push({ id, name: titles[id] || id, about: "", site: "", server });
    }
    return rows;
  }

  $: titles = Object.fromEntries(Object.entries(details).map(([id, d]) => [id, d.title || ""]));
  $: rows = which === "apps" ? appList(cfg, titles) : [];
  $: have = new Set(Object.keys(cfg?.mcp.servers ?? {}));
  $: fresh = found.filter((f) => !have.has(f.name));
  $: freshCustom = fresh.filter((f) => !CATALOG.some((c) => c.id === f.name));

  $: addable = CATALOG.filter((c) => !have.has(c.id));

  function statusLine(h: Health | undefined, off: boolean): string {
    if (off) return "Off";
    if (!h || h.state === "checking") return "Checking…";
    if (h.state === "signing") return "Signing in…";
    if (h.state === "signin") return "Needs sign-in";
    if (h.state === "error") return h.error || "Couldn't connect";
    return h.tools === 1 ? "1 tool" : `${h.tools ?? 0} tools`;
  }

  const openOutside = (url: string) => api.openConfirmedUrl(url).catch((e) => notify(String(e), true));

  async function addKnown(id: string) {
    const c = CATALOG.find((item) => item.id === id) as CatalogConnector | undefined;
    if (!c || cfg?.mcp.servers[id]) return;
    const server = serverOf(c);
    if (!(await save((cf) => (cf.mcp.servers[id] = server), `${c.name} added`))) return;
    if (c.auth === "signin") void signIn(id);
    else void check(id);
  }

  async function addFound(name: string, server: Server) {
    if (cfg?.mcp.servers[name]) return notify(`${name} is already here`, true);
    if (await save((c) => (c.mcp.servers[name] = { ...server, enabled: true }), `${name} added`)) void check(name, true);
  }

  function freeName(base: string): string {
    const servers = cfg?.mcp.servers ?? {};
    let id = base.slice(0, 32);
    if (!servers[id]) return id;
    let n = 2;
    while (servers[`${id.slice(0, 28)}${n}`]) n += 1;
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
      const built = remoteFromPaste(text);
      if (!built?.url) return notify("That link is not a connector", true);
      const id = chosenName(nameFromUrl(built.url));
      if (!id) return notify("Use letters and numbers for the name", true);
      list = [[id, withBearer(built, bearer)]];
    } else if (kind === "block") {
      list = ((await api.connectorsFromText(text).catch(() => [])) as [string, Server][])
        .filter(([id]) => NAME_OK.test(id))
        .map(([id, srv]) => [id, withBearer(srv, bearer)]);
      if (!list.length) return notify("That did not include a connector", true);
    } else {
      const words = splitLine(text);
      if (!words.length) return notify("Paste a link, a config block, or a command", true);
      const id = chosenName(nameFromProgram(text));
      if (!id) return notify("Use letters and numbers for the name", true);
      list = [[id, { command: words[0], args: words.slice(1), env: {}, enabled: true }]];
    }
    const freshList = list.filter(([id]) => !cfg?.mcp.servers[id]);
    if (!freshList.length) return notify(list.length === 1 ? `${list[0][0]} is already here` : "Those are already here", true);
    if (await save((c) => freshList.forEach(([id, srv]) => (c.mcp.servers[id] = srv)), "")) {
      pasted = "";
      customName = "";
      bearer = "";
      ownOpen = false;
      for (const [id] of freshList) void check(id, true);
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
  });

  async function load() {
    try {
      [cfg, info] = await Promise.all([api.getConfig(), remote.info()]);
      loadError = "";
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

  function wantsToken(row: AppRow, h: Health | undefined): boolean {
    if (!row.server.url || row.server.auth === "gh") return false;
    const known = CATALOG.find((c) => c.id === row.id);
    if (known && known.auth !== "token") return false;
    if (h?.state === "signin") return true;
    return h?.state === "error" && /bearer|token|unauthorized|sign-in/i.test(`${h.error ?? ""} ${h.raw ?? ""}`);
  }

  function setTokenDraft(id: string, event: Event) {
    const value = (event.currentTarget as HTMLInputElement).value;
    tokenDrafts = { ...tokenDrafts, [id]: value };
  }

  async function saveToken(id: string) {
    const token = (tokenDrafts[id] ?? "").trim();
    const srv = cfg?.mcp.servers[id] as Server | undefined;
    if (!token || !srv) return;
    const next = withBearer(srv, token);
    const ok = await save((c) => {
      const cur = c.mcp.servers[id] as Server | undefined;
      if (cur) cur.headers = next.headers;
    }, "");
    if (!ok) return;
    tokenDrafts = { ...tokenDrafts, [id]: "" };
    void check(id);
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
            <button class="btn" on:click={() => dispatch("setupRemote")}><Icon name="wrench" size={12} /><span>Set up again</span></button>
            <button class="btn danger" on:click={unlink}>Unlink</button>
          </span>
        </div>
        <div class="row">
          <span class="row-label">Sync<small>{syncedLine}</small></span>
          <span class="row-ctl auto"><button class="btn" disabled={syncing} on:click={syncNow}>{syncing ? "Syncing…" : "Sync now"}</button></span>
        </div>
      {:else}
        <div class="row">
          <span class="row-label">Not linked</span>
          <span class="row-ctl auto"><button class="btn" on:click={() => dispatch("setupRemote")}><Icon name="wrench" size={12} /><span>Set up</span></button></span>
        </div>
      {/if}
    </div>
  </div>

{:else}
    {#if rows.length}
      <div class="rows">
        <div class="row row-title"><span>Connected</span></div>
        {#each rows as row (row.id)}
          {@const h = health[row.id]}
          {@const off = row.server.enabled === false}
          {@const open = openTools === row.id}
          {@const tools = details[row.id]?.tools ?? []}
          {@const line = statusLine(h, off)}
          {@const need = !off && h?.state === "error" ? NEEDS[h.error ?? ""] : undefined}
          <div class="conn" class:off>
            <div class="row">
              <button class="row-main" aria-expanded={open} on:click={() => toggleTools(row.id)}>
                <ConnectorLogo id={row.id} name={row.name} site={row.site} src={details[row.id]?.icon ?? null} size={32} />
                <span class="row-label">{row.name}
                  <small title={h?.raw ?? ""}><span class:st-ok={h?.state === "ok" && !off} class:st-bad={h?.state === "error" && !off}>{line}</span>{row.about ? ` · ${row.about}` : ""}</small>
                </span>
                <Icon name={open ? "chevDown" : "chevRight"} size={12} />
              </button>
              <span class="row-ctl auto">
                {#if need}
                  <button class="btn" on:click={() => openOutside(need)}>Get {h?.error?.replace("Needs ", "")}</button>
                {:else if !off && (h?.state === "signin" || h?.state === "signing") && !wantsToken(row, h)}
                  <button class="btn primary" disabled={h?.state === "signing"} on:click={() => signIn(row.id)}>{h?.state === "signing" ? "Signing in…" : "Sign in"}</button>
                {:else if !off && h?.state === "error" && !wantsToken(row, h)}
                  <button class="btn" on:click={() => check(row.id, true)}>Try again</button>
                {/if}
                <Switch on={!off} title={off ? `Turn ${row.name} on` : `Turn ${row.name} off`} on:toggle={() => toggle(row.id)} />
              </span>
            </div>
            {#if wantsToken(row, h)}
              <div class="row">
                <input
                  class="tin mono"
                  type="password"
                  placeholder="Bearer token"
                  aria-label={`Bearer token for ${row.name}`}
                  autocomplete="off"
                  spellcheck="false"
                  value={tokenDrafts[row.id] ?? ""}
                  on:input={(e) => setTokenDraft(row.id, e)}
                  on:keydown={(e) => { if (e.key === "Enter" && (tokenDrafts[row.id] ?? "").trim()) void saveToken(row.id); }}
                />
                <button class="btn primary" disabled={!(tokenDrafts[row.id] ?? "").trim()} on:click={() => saveToken(row.id)}>Save token</button>
              </div>
            {/if}
            {#if open}
              <div class="details">
                {#if !off && h?.state === "error" && h.raw}
                  <div class="row"><pre class="fail">{h.raw}</pre></div>
                {:else if !off && tools.length}
                  <div class="row"><span class="row-label"><small>Agents in every session can use these. Ask means they ask you first.</small></span></div>
                {/if}
                {#each tools as t (t.name)}
                  {@const m = effective(t.mode)}
                  <div class="row" class:off={m === "off"}>
                    <span class="row-label" title={t.description || t.name}>{t.name}{#if t.description}<small>{t.description}</small>{/if}</span>
                    <SegControl label={t.name} options={TOOL_OPTIONS} value={m} on:pick={(e) => pickMode(row.id, t.name, e.detail)} />
                  </div>
                {:else}
                  <div class="row"><span class="row-label"><small>{off ? "Turn it on to see its tools." : "No tools to show yet."}</small></span></div>
                {/each}
                <div class="row end">
                  <button class="btn danger" on:click={() => removeServer(row.id)}>{delConn === row.id ? "Click again to remove" : "Remove"}</button>
                </div>
              </div>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
    {#if addable.length}
      <div class="rows">
        <div class="row row-title"><span>Add a connector</span></div>
        <div class="row">
          <div class="tiles">
            {#each addable as c (c.id)}
              {@const hit = fresh.find((f) => f.name === c.id)}
              <button class="tile" type="button" disabled={saving} on:click={() => (hit ? addFound(hit.name, hit.server) : addKnown(c.id))}>
                <ConnectorLogo id={c.id} name={c.name} site={c.site} src={null} size={32} />
                <span class="tile-text"><span class="t-label">{c.name}</span><span class="t-hint">{c.about}</span></span>
                <Icon name="plus" size={16} />
              </button>
            {/each}
          </div>
        </div>
      </div>
    {/if}
    {#if freshCustom.length}
      <div class="rows">
        <div class="row row-title"><span>Found on this computer</span></div>
        <div class="row">
          <div class="tiles">
            {#each freshCustom as f (f.name)}
              <button class="tile" type="button" disabled={saving} on:click={() => addFound(f.name, f.server)}>
                <ConnectorLogo id={f.name} name={f.name} site="" src={null} size={32} />
                <span class="tile-text"><span class="t-label">{f.name}</span><span class="t-hint">From {f.source}</span></span>
                <Icon name="plus" size={16} />
              </button>
            {/each}
          </div>
        </div>
      </div>
    {/if}
    <div class="rows">
      {#if ownOpen}
        <div class="row row-title">
          <span>Add your own</span>
          <span class="row-ctl auto"><button class="btn" on:click={() => (ownOpen = false)}>Cancel</button></span>
        </div>
        <div class="row">
          <textarea class="tin" rows="3" bind:value={pasted} placeholder="Paste a link, a config block, or a command" aria-label="Paste a link, a config block, or a command" spellcheck="false"></textarea>
        </div>
        <div class="row">
          <input class="tin mono" type="password" bind:value={bearer} placeholder="Bearer token" aria-label="Bearer token" autocomplete="off" spellcheck="false" on:keydown={(e) => { if (e.key === "Enter" && pasted.trim()) void addPasted(); }} />
        </div>
        <div class="row">
          <input class="tin" bind:value={customName} placeholder="Name (optional)" aria-label="Name" spellcheck="false" on:keydown={(e) => { if (e.key === "Enter" && pasted.trim()) void addPasted(); }} />
          <button class="btn primary" disabled={saving || !pasted.trim()} on:click={() => void addPasted()}>Add</button>
        </div>
      {:else}
        <div class="row ctl-row">
          <span class="row-label">Add your own</span>
          <span class="row-ctl auto"><button class="btn" on:click={() => (ownOpen = true)}>Add</button></span>
        </div>
      {/if}
    </div>
{/if}

<style>
  .conn {
    border-top: 1px solid var(--line);
  }
  /* A connector that is off dims, its switch does not. */
  .conn.off .row-main {
    opacity: 0.55;
  }
  .row.off .row-label {
    color: var(--muted);
  }
  .row.end {
    justify-content: flex-end;
    gap: calc(var(--space) * 2);
  }
  .st-ok {
    color: var(--ok-text);
  }
  .st-bad {
    color: var(--bad-text);
  }
  /* The last error a connector gave, scrollable. */
  .fail {
    flex: 1;
    min-width: 0;
    max-height: 160px;
    margin: 0;
    overflow: auto;
    color: var(--muted);
    font-family: var(--mono);
    font-size: var(--fs-sm);
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  /* ── Catalog: a tile per connector to add ── */

  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(var(--w-field), 1fr));
    gap: calc(var(--space) * 2);
    flex: 1;
    min-width: 0;
  }
  .tile {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 3);
    min-width: 0;
    padding: calc(var(--space) * 3);
    background: none;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    color: var(--muted);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition: background var(--dur) ease, color var(--dur) ease;
  }
  .tile:hover:not(:disabled) {
    background: var(--hover);
    color: var(--text);
  }
  .tile:disabled {
    opacity: var(--disabled);
    cursor: default;
  }
  .tile-text {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
  }
  .tile-text > span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
