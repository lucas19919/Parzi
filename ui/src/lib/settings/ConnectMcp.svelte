<script lang="ts">
  import { createEventDispatcher, onMount, tick } from "svelte";
  import { api, type ParziConfig } from "../api";
  import { updateConfig } from "../config";
  import { CATALOG, hostedServer, nameFromUrl, type ServerCfg as Server } from "../connectorCatalog";
  import { classifyPaste, nameFromProgram, splitLine } from "../cmdline";
  import ConnectorLogo from "../ConnectorLogo.svelte";
  import Icon from "../Icon.svelte";
  import { tooltip } from "../tooltip";
  import { setOverlay } from "../overlay";
  import "./shared.css";

  export let notify: (msg: string, err?: boolean) => void = () => {};

  const dispatch = createEventDispatcher<{ close: void; added: void }>();

  let cfg: ParziConfig | null = null;
  let found: { name: string; source: string; server: Server }[] = [];
  let customName = "";
  let pasted = "";
  let saving = false;
  let loadError = "";
  let card: HTMLDivElement;

  onMount(() => {
    setOverlay("connect-mcp", true);
    void load();
    void tick().then(() => card?.focus());
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") dispatch("close");
    };
    window.addEventListener("keydown", onKey);
    return () => {
      setOverlay("connect-mcp", false);
      window.removeEventListener("keydown", onKey);
    };
  });

  async function load() {
    try {
      const [c, f] = await Promise.all([
        api.getConfig(),
        api.connectorsFound().catch(() => [] as { name: string; source: string; server: Server }[]),
      ]);
      cfg = c;
      found = f;
    } catch (e) {
      loadError = String(e);
    }
  }

  $: have = new Set(Object.keys(cfg?.mcp.servers ?? {}));
  $: fresh = found.filter((f) => !have.has(f.name));
  $: freshCustom = fresh.filter((f) => !CATALOG.some((c) => c.id === f.name));

  async function save(change: (c: ParziConfig) => void, done: string): Promise<boolean> {
    saving = true;
    try {
      cfg = await updateConfig(change);
      if (done) notify(done);
      dispatch("added");
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

  async function addKnown(id: string) {
    const c = CATALOG.find((item) => item.id === id);
    if (!c || cfg?.mcp.servers[id]) return;
    const server = hostedServer(c.url, { auth: c.auth === "gh" ? "gh" : "" });
    await save((cf) => (cf.mcp.servers[id] = server), `${c.name} added`);
  }

  async function addFound(name: string, server: Server) {
    if (cfg?.mcp.servers[name]) return notify(`${name} is already here`, true);
    await save((c) => (c.mcp.servers[name] = { ...server, enabled: true }), `${name} added`);
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
    const freshList = list.filter(([id]) => !cfg?.mcp.servers[id]);
    if (!freshList.length) return notify(list.length === 1 ? `${list[0][0]} is already here` : "Those are already here", true);
    if (await save((c) => freshList.forEach(([id, srv]) => (c.mcp.servers[id] = srv)), "")) {
      pasted = "";
      customName = "";
    }
  }
</script>

<div class="backdrop" role="presentation" on:click|self={() => dispatch("close")}>
  <div class="box" role="dialog" aria-modal="true" aria-label="Connect MCP" tabindex="-1" bind:this={card}>
    <div class="head">
      <span class="title">Connect MCP</span>
      <button class="x" aria-label="Close" use:tooltip={"Close"} on:click={() => dispatch("close")}><Icon name="close" size={12} /></button>
    </div>
    {#if loadError}
      <div class="load-err"><span>{loadError}</span></div>
    {:else if !cfg}
      <div class="pad">
        <div class="skel" />
        <div class="skel" />
      </div>
    {:else}
      <div class="body">
        <h3 class="section-title">MCPs</h3>
        {#each CATALOG as c (c.id)}
          {@const hit = fresh.find((f) => f.name === c.id)}
          <div class="pick">
            <span class="logo"><ConnectorLogo id={c.id} name={c.name} site={c.site} src={null} size={20} /></span>
            <span class="grow">
              <span class="t">{c.name}</span>
              <span class="s">{c.site || c.about}</span>
            </span>
            {#if have.has(c.id)}
              <span class="added">Added</span>
            {:else if hit}
              <button class="sbtn" disabled={saving} on:click={() => addFound(hit.name, hit.server)}>Add</button>
            {:else}
              <button class="sbtn" disabled={saving} on:click={() => addKnown(c.id)}>Connect</button>
            {/if}
          </div>
        {/each}
        {#if freshCustom.length}
          <h3 class="section-title">Found on this computer</h3>
          {#each freshCustom as f (f.name)}
            <div class="pick">
              <span class="logo"><ConnectorLogo id={f.name} name={f.name} site="" src={null} size={20} /></span>
              <span class="grow">
                <span class="t">{f.name}</span>
                <span class="s">Already on this computer</span>
              </span>
              <button class="sbtn" disabled={saving} on:click={() => addFound(f.name, f.server)}>Add</button>
            </div>
          {/each}
        {/if}
        <h3 class="section-title">Custom</h3>
        <div class="paste">
          <input class="tin" bind:value={customName} placeholder="Name (optional)" aria-label="Name" spellcheck="false" />
          <textarea class="tin" rows="4" bind:value={pasted} placeholder="Paste a link, a config block, or a command" aria-label="Paste a link, a config block, or a command" spellcheck="false"></textarea>
          <button class="sbtn primary" disabled={saving || !pasted.trim()} on:click={() => void addPasted()}>Add</button>
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 1100;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: calc(var(--space) * 6);
    background: var(--scrim);
  }
  .box {
    width: min(560px, calc(100vw - 48px));
    max-height: min(640px, calc(100vh - 48px));
    min-width: 0;
    flex: none;
    display: flex;
    flex-direction: column;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    overflow: hidden;
  }
  .head {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: calc(var(--space) * 3);
    padding: calc(var(--space) * 3) calc(var(--space) * 4);
    border-bottom: 1px solid var(--line);
  }
  .title {
    font-size: var(--fs-md);
    font-weight: 600;
    color: var(--text);
  }
  .x {
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
  .x:hover {
    background: var(--hover);
    color: var(--text);
  }
  .load-err {
    margin: calc(var(--space) * 4);
  }
  .pad {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    padding: calc(var(--space) * 4);
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    padding: calc(var(--space) * 4);
  }
  .body .section-title {
    margin: calc(var(--space) * 2) 0 0;
  }
  .body .section-title:first-child {
    margin-top: 0;
  }
  .pick {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 3);
    padding: calc(var(--space) * 2) calc(var(--space) * 3);
    border: 1px solid transparent;
    border-radius: var(--radius);
  }
  .pick:hover {
    background: var(--hover);
  }
  .logo {
    flex: none;
    display: inline-flex;
  }
  .grow {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space);
  }
  .t {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-md);
    font-weight: 500;
    color: var(--text);
  }
  .s {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-xs);
    color: var(--muted);
  }
  .added {
    flex: none;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .paste {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: calc(var(--space) * 2);
  }
  .paste .tin {
    width: 100%;
  }
  .paste textarea {
    min-height: 88px;
    resize: vertical;
  }
  .paste .sbtn {
    align-self: flex-start;
  }
</style>
