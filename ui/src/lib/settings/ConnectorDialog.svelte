<script lang="ts">
  // Adding a connector: the directory of well-known ones (a click each, a
  // token step for those that take one), or a custom one by its address or
  // the command that starts it. Saving and starting stay with the section;
  // this only asks.
  import { createEventDispatcher, onMount, tick } from "svelte";
  import { fade, fly } from "svelte/transition";
  import Icon from "../Icon.svelte";
  import ConnectorLogo from "../ConnectorLogo.svelte";
  import { CATALOG, nameFromUrl, type CatalogConnector } from "../connectorCatalog";
  import { tooltip } from "../tooltip";
  import { portal } from "../popover";

  // Connectors already added, and how each answered this visit.
  export let added: Set<string> = new Set();
  export let health: Record<string, { state: "checking" | "ok" | "error"; tools?: number; error?: string }> = {};
  export let busy = false;
  export let start: "browse" | "custom" = "browse";

  const dispatch = createEventDispatcher<{
    connect: { connector: CatalogConnector; token: string };
    custom: { name: string; url: string; token: string; command: string; env: string };
    open: { url: string };
    close: void;
  }>();

  let view: "browse" | "token" | "custom" = start;
  let query = "";
  let pick: CatalogConnector | null = null;
  let token = "";
  let name = "";
  let url = "";
  let key = "";
  let local = false;
  let command = "";
  let env = "";
  let card: HTMLDivElement;

  $: q = query.trim().toLowerCase();
  $: shown = CATALOG.filter((c) => !q || `${c.name} ${c.about}`.toLowerCase().includes(q));

  onMount(() => {
    const before = document.activeElement as HTMLElement | null;
    void focusFirst();
    return () => before?.focus?.();
  });

  async function focusFirst() {
    await tick();
    card?.querySelector<HTMLInputElement>("input")?.focus();
  }

  function go(next: typeof view) {
    view = next;
    void focusFirst();
  }

  function connect(c: CatalogConnector) {
    if (c.auth === "token") {
      pick = c;
      token = "";
      go("token");
      return;
    }
    dispatch("connect", { connector: c, token: "" });
  }

  function sendToken() {
    if (!pick || !token.trim()) return;
    dispatch("connect", { connector: pick, token: token.trim() });
    go("browse");
  }

  function sendCustom() {
    dispatch("custom", { name: name.trim(), url: local ? "" : url.trim(), token: local ? "" : key.trim(), command: local ? command.trim() : "", env: local ? env : "" });
  }

  function onKey(e: KeyboardEvent) {
    if (e.key !== "Escape") return;
    e.preventDefault();
    e.stopPropagation();
    if (view === "browse") dispatch("close");
    else go("browse");
  }

  $: canAdd = !!(local ? command.trim() && name.trim() : /^https?:\/\/\S+$/i.test(url.trim()));
</script>

<!-- svelte-ignore a11y-no-noninteractive-element-interactions -->
<div class="backdrop" role="presentation" use:portal transition:fade={{ duration: 120 }} on:click|self={() => dispatch("close")} on:keydown={onKey}>
  <div class="card" role="dialog" aria-modal="true" aria-label="Add a connector" bind:this={card} transition:fly={{ y: 8, duration: 160 }}>
    <header>
      {#if view !== "browse"}
        <button class="icon" use:tooltip={"Back"} aria-label="Back" on:click={() => go("browse")}><Icon name="arrowLeft" /></button>
      {/if}
      <h2>{view === "custom" ? "Custom connector" : view === "token" && pick ? `Connect ${pick.name}` : "Add a connector"}</h2>
      <button class="icon" use:tooltip={"Close"} aria-label="Close" on:click={() => dispatch("close")}><Icon name="close" /></button>
    </header>

    {#if view === "browse"}
      <div class="search">
        <Icon name="search" size={12} />
        <input bind:value={query} placeholder="Search connectors" aria-label="Search connectors" spellcheck="false" />
      </div>
      <div class="grid" data-ui-list>
        {#each shown as c (c.id)}
          {@const h = health[c.id]}
          {@const on = added.has(c.id)}
          <div class="item">
            <ConnectorLogo id={c.id} name={c.name} site={c.site} size={36} />
            <span class="text">
              <span class="name">{c.name}</span>
              <span class="about">{c.about}</span>
            </span>
            {#if h?.state === "checking"}
              <span class="state">Connecting…</span>
            {:else if on && h?.state !== "error"}
              <span class="state ok"><Icon name="check" size={12} /> Added</span>
            {:else}
              <button class="sbtn" disabled={busy} on:click={() => connect(c)}>{h?.state === "error" ? "Retry" : "Connect"}</button>
            {/if}
          </div>
        {:else}
          <p class="none">Nothing matches. Add it as a custom connector.</p>
        {/each}
      </div>
      <footer>
        <span class="hint">Signing in opens the service's page in your browser.</span>
        <button class="link" on:click={() => go("custom")}>Custom connector</button>
      </footer>
    {:else if view === "token" && pick}
      <div class="step">
        <ConnectorLogo id={pick.id} name={pick.name} site={pick.site} size={44} />
        <label>
          <span class="label">{pick.name} {pick.tokenName ?? "token"}</span>
          <input type="password" bind:value={token} placeholder="Paste it here" autocomplete="off" spellcheck="false" on:keydown={(e) => e.key === "Enter" && sendToken()} />
        </label>
      </div>
      <footer>
        {#if pick.tokenUrl}<button class="link" on:click={() => pick?.tokenUrl && dispatch("open", { url: pick.tokenUrl })}>Create one</button>{/if}
        <span class="grow" />
        <button class="sbtn primary" disabled={busy || !token.trim()} on:click={sendToken}>Connect</button>
      </footer>
    {:else}
      <div class="form">
        <label>
          <span class="label">Name</span>
          <input bind:value={name} placeholder={!local && url.trim() ? nameFromUrl(url.trim()) : "mail"} spellcheck="false" />
        </label>
        {#if local}
          <label>
            <span class="label">Command</span>
            <input class="mono" bind:value={command} placeholder="npx -y some-mcp-server" spellcheck="false" />
          </label>
          <label>
            <span class="label">Environment <em>optional, NAME=value per line</em></span>
            <textarea class="mono" rows="3" bind:value={env} spellcheck="false" />
          </label>
        {:else}
          <label>
            <span class="label">Server address</span>
            <input class="mono" bind:value={url} placeholder="https://example.com/mcp" spellcheck="false" on:keydown={(e) => e.key === "Enter" && canAdd && sendCustom()} />
          </label>
          <label>
            <span class="label">API key <em>optional, if the service gives you one instead of a sign-in</em></span>
            <input type="password" bind:value={key} autocomplete="off" spellcheck="false" />
          </label>
        {/if}
      </div>
      <footer>
        <button class="link" on:click={() => (local = !local)}>{local ? "Use a server address" : "Run a local program instead"}</button>
        <span class="grow" />
        <button class="sbtn primary" disabled={busy || !canAdd} on:click={sendCustom}>Add</button>
      </footer>
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 1000;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    background: color-mix(in srgb, var(--bg) 60%, transparent);
  }
  .card {
    width: min(640px, 100%);
    max-height: min(640px, 100%);
    display: flex;
    flex-direction: column;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
    overflow: hidden;
  }
  header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 14px 14px 10px 20px;
  }
  header:has(.icon:first-child) {
    padding-left: 12px;
  }
  h2 {
    flex: 1;
    margin: 0;
    font-size: var(--fs-lg);
    font-weight: 600;
    color: var(--text);
  }
  .icon {
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
  .icon:hover {
    background: var(--line);
    color: var(--text);
  }
  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 20px 12px;
    padding: 8px 12px;
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    color: var(--muted);
  }
  .search input {
    flex: 1;
    min-width: 0;
    background: none;
    border: none;
    outline: none;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-md);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: 8px;
    padding: 0 20px 16px;
    overflow-y: auto;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 12px;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .name {
    font-size: var(--fs-md);
    font-weight: 600;
    color: var(--text);
  }
  /* Short lines that wrap when the window is narrow. */
  .about {
    font-size: var(--fs-sm);
    line-height: 1.35;
    color: var(--muted);
  }
  .state {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .state.ok {
    color: var(--ok);
  }
  .none {
    margin: 0;
    padding: 12px 4px;
    font-size: var(--fs-md);
    color: var(--muted);
  }
  footer {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 20px;
    border-top: 1px solid var(--line);
  }
  .hint {
    flex: 1;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .grow {
    flex: 1;
  }
  .link {
    padding: 0;
    background: none;
    border: none;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
    text-decoration: underline;
    text-underline-offset: 2px;
    cursor: pointer;
  }
  .step {
    display: flex;
    align-items: flex-end;
    gap: 16px;
    padding: 4px 20px 20px;
  }
  .step label {
    flex: 1;
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 4px 20px 20px;
    overflow-y: auto;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .label {
    font-size: var(--fs-sm);
    font-weight: 500;
    color: var(--text);
  }
  .label em {
    font-style: normal;
    font-weight: 400;
    color: var(--muted);
  }
  label input,
  label textarea {
    background: var(--bg);
    color: var(--text);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 8px 10px;
    font: inherit;
    font-size: var(--fs-md);
    resize: vertical;
    outline: none;
  }
  label input:focus,
  label textarea:focus {
    border-color: var(--accent);
  }
  .mono {
    font-family: var(--mono);
  }
  .sbtn.primary {
    background: var(--text);
    border-color: var(--text);
    color: var(--bg);
  }
  .sbtn.primary:disabled {
    opacity: 0.4;
  }
</style>
