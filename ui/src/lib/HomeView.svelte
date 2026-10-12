<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import Icon from "./Icon.svelte";
  import ProviderLogo from "./ProviderLogo.svelte";
  import { brain, type Project, type ProviderStatus, type SessionMeta } from "./api";
  import { faviconUrl, history, pins } from "./browserData";
  import { hasMark } from "./providerMarks";
  import { PROVIDER_ORDER, isUsable, nameOf, stateLabel } from "./providerRows";
  import { remote as server } from "./remote";
  import { folderName, hostOf } from "./tabs";
  import { HOME_WIDGETS, hiddenWidgets, toggleWidget, type HomeWidget } from "./homeWidgets";
  import { popover, placeAbove } from "./popover";

  export let threads: SessionMeta[] = [];
  export let running: Set<string> = new Set();
  // The agents a new session here can use (the server's when Remote is on).
  export let board: ProviderStatus[] = [];
  // Remote on: projects are the server's, with their server folders.
  export let remote = false;
  // The draft's folder (local or on the server), to mark its project.
  export let folder = "";

  const dispatch = createEventDispatcher<{
    open: { url: string };
    openSession: { id: string };
    allSessions: void;
    folder: { path: string };
    agents: void;
    brain: void;
  }>();

  const ROWS = 4;

  $: shown = (id: HomeWidget) => !$hiddenWidgets.includes(id);
  let customizeBtn: HTMLButtonElement | null = null;
  let customizing = false;
  let customizeStyle = "";

  function toggleCustomize() {
    customizing = !customizing;
    if (customizing) customizeStyle = placeAbove(customizeBtn, 220, true);
  }

  let projects: Project[] = [];
  let projectsFor: boolean | null = null;
  $: if (remote !== projectsFor) {
    projectsFor = remote;
    void loadProjects(remote);
  }

  async function loadProjects(onServer: boolean) {
    const list = await (onServer ? server.projects() : brain.projects()).catch(() => [] as Project[]);
    if (onServer === remote) projects = list.filter((p) => p.folder).slice(0, ROWS);
  }

  $: recent = threads.slice(0, ROWS);
  $: agents = PROVIDER_ORDER.map((id) => board.find((b) => b.provider === id))
    .filter((p): p is ProviderStatus => !!p && p.state !== "not_installed" && p.state !== "disabled")
    .slice(0, ROWS);
  // Pinned sites first; until there are some, the most visited.
  $: sites = ($pins.length ? $pins : [...$history].sort((a, b) => b.count - a.count)).slice(0, 8);

  function ago(iso: string) {
    const t = Date.parse(iso);
    if (!t) return "";
    const s = (Date.now() - t) / 1000;
    if (s < 60) return "just now";
    if (s < 3600) return `${Math.floor(s / 60)}m ago`;
    if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
    if (s < 172800) return "yesterday";
    return new Date(t).toLocaleDateString(undefined, { month: "short", day: "numeric" });
  }

  // The folder when its name says something the title doesn't, else the notes.
  function projectMeta(p: Project) {
    const dir = folderName(p.folder);
    if (dir.toLowerCase() !== p.title.toLowerCase()) return dir;
    return p.notes.length ? `${p.notes.length} note${p.notes.length === 1 ? "" : "s"}` : "";
  }

  // A site without a favicon keeps its row, just no picture.
  function noIcon(e: Event) {
    (e.currentTarget as HTMLImageElement).style.visibility = "hidden";
  }

  function usage(p: ProviderStatus) {
    const u = p.usage[0];
    return u ? { label: u.label, pct: Math.max(0, Math.min(100, Math.round(u.used_percent))) } : null;
  }
</script>

<div class="home">
  {#if shown("recent")}
  <section class="card" aria-labelledby="home-recent">
    <header>
      <h3 id="home-recent"><Icon name="chat" size={14} />Recent sessions</h3>
      {#if threads.length}<button class="more" on:click={() => dispatch("allSessions")}>All</button>{/if}
    </header>
    {#if recent.length}
      <div class="rows" data-ui-list>
        {#each recent as s (s.id)}
          <button class="row" on:click={() => dispatch("openSession", { id: s.id })}>
            <span class="dot" class:live={running.has(s.id)} />
            <span class="name">{s.title || "Untitled session"}</span>
            <span class="meta">{ago(s.updated)}</span>
          </button>
        {/each}
      </div>
    {:else}
      <p class="empty">Your sessions show up here.</p>
    {/if}
  </section>
  {/if}

  {#if shown("agents")}
  <section class="card" aria-labelledby="home-agents">
    <header>
      <h3 id="home-agents"><Icon name="bot" size={14} />Agents</h3>
      <button class="more" on:click={() => dispatch("agents")}>Manage</button>
    </header>
    {#if agents.length}
      <div class="rows" data-ui-list>
        {#each agents as p (p.provider)}
          {@const u = usage(p)}
          <button class="row" on:click={() => dispatch("agents")}>
            <span class="logo">{#if hasMark(p.provider)}<ProviderLogo provider={p.provider} size={14} />{/if}</span>
            <span class="name">{nameOf(p.provider)}</span>
            {#if u && isUsable(p)}
              <span class="meter" title="{u.label}: {u.pct}% used"><i style:width="{u.pct}%" class:hot={u.pct >= 80} /></span>
              <span class="meta num">{u.pct}%</span>
            {:else}
              <span class="meta" class:warn={!isUsable(p)}>{stateLabel(p)}</span>
            {/if}
          </button>
        {/each}
      </div>
    {:else}
      <p class="empty">No agents set up yet. Manage opens their sign-in.</p>
    {/if}
  </section>
  {/if}

  {#if shown("projects")}
  <section class="card" aria-labelledby="home-projects">
    <header>
      <h3 id="home-projects"><Icon name="project" size={14} />Projects</h3>
      <button class="more" on:click={() => dispatch("brain")}>Brain</button>
    </header>
    {#if projects.length}
      <div class="rows" data-ui-list>
        {#each projects as p (p.slug)}
          <button class="row" class:on={!!folder && p.folder === folder} on:click={() => dispatch("folder", { path: p.folder })}>
            <span class="name">{p.title}</span>
            <span class="meta">{projectMeta(p)}</span>
          </button>
        {/each}
      </div>
    {:else}
      <p class="empty">A project is a folder plus the notes agents should know. Pick a folder in the composer to start one.</p>
    {/if}
  </section>
  {/if}

  {#if shown("sites")}
  <section class="card" aria-labelledby="home-sites">
    <header>
      <h3 id="home-sites"><Icon name="globe" size={14} />{$pins.length ? "Pinned" : "Most visited"}</h3>
    </header>
    {#if sites.length}
      <div class="sites" data-ui-list>
        {#each sites as s (s.url)}
          <button class="site" title={s.url} on:click={() => dispatch("open", { url: s.url })}>
            <img src={faviconUrl(s.url)} alt="" draggable="false" on:error={noIcon} />
            <span>{s.title || hostOf(s.url)}</span>
          </button>
        {/each}
      </div>
    {:else}
      <p class="empty">Pin a page from its toolbar and it shows up here.</p>
    {/if}
  </section>
  {/if}
</div>

<div class="foot">
  <button
    bind:this={customizeBtn}
    class="customize"
    class:open={customizing}
    aria-haspopup="true"
    aria-expanded={customizing}
    on:click|stopPropagation={toggleCustomize}
  >
    <Icon name="settings" size={12} />
    <span>Customize</span>
  </button>
</div>

{#if customizing}
  <div class="home-menu" style={customizeStyle} use:popover={{ anchor: customizeBtn, close: () => (customizing = false) }}>
    <p class="menu-head">Show on Home</p>
    {#each HOME_WIDGETS as w (w.id)}
      <label class="opt">
        <input type="checkbox" checked={shown(w.id)} on:change={() => toggleWidget(w.id)} />
        <span>{w.label}</span>
      </label>
    {/each}
  </div>
{/if}

<style>
  .home {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 12px;
    width: 100%;
  }
  /* An odd card out takes the full width. */
  .card:last-child:nth-child(odd) {
    grid-column: 1 / -1;
  }
  .card:last-child:nth-child(odd) .sites {
    grid-template-columns: repeat(4, minmax(0, 1fr));
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
    min-height: 172px;
    padding: 10px 8px 8px;
    background: var(--glass-strong-bg);
    -webkit-backdrop-filter: var(--glass-strong-blur);
    backdrop-filter: var(--glass-strong-blur);
    border: var(--glass-hairline);
    border-radius: var(--radius-lg);
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.28);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    height: 24px;
    padding: 0 4px 0 8px;
  }
  h3 {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0;
    font-size: var(--fs-md);
    font-weight: 600;
    color: var(--muted);
  }
  .more {
    height: 22px;
    padding: 0 8px;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .more:hover {
    background: var(--hover);
    color: var(--text);
  }
  .rows {
    display: flex;
    flex-direction: column;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 32px;
    padding: 0 8px;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--text);
    text-align: left;
    cursor: pointer;
  }
  .row:hover {
    background: var(--hover);
  }
  .row.on {
    background: var(--selected);
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-md);
  }
  .meta {
    flex: none;
    max-width: 45%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .meta.warn {
    color: var(--warn);
  }
  .num {
    min-width: 32px;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .dot {
    width: 6px;
    height: 6px;
    flex: none;
    border-radius: 50%;
    background: var(--faint);
  }
  .dot.live {
    background: var(--ok);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--ok) 22%, transparent);
  }
  .logo {
    display: inline-flex;
    width: 14px;
    flex: none;
  }
  .meter {
    width: 64px;
    height: 4px;
    flex: none;
    overflow: hidden;
    border-radius: 999px;
    background: var(--selected);
  }
  .meter i {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--muted);
  }
  .meter i.hot {
    background: var(--warn);
  }
  .empty {
    margin: 0;
    padding: 4px 8px;
    font-size: var(--fs-sm);
    line-height: 1.5;
    color: var(--muted);
  }
  /* Short windows: three rows a card, so both rows of cards fit. */
  @media (max-height: 720px) {
    .card {
      min-height: 0;
    }
    .rows > :nth-child(n + 4),
    .sites > :nth-child(n + 7) {
      display: none;
    }
  }
  .foot {
    display: flex;
    justify-content: flex-end;
    padding-top: 8px;
  }
  .customize {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 8px;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .customize:hover,
  .customize.open {
    background: var(--hover);
    color: var(--text);
  }
  .home-menu {
    width: 220px;
    padding: 4px;
  }
  .menu-head {
    margin: 0;
    padding: 6px 8px 4px;
    font-size: var(--fs-sm);
    font-weight: 600;
    color: var(--muted);
  }
  .opt {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 32px;
    padding: 0 8px;
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--fs-md);
    cursor: pointer;
  }
  .opt:hover {
    background: var(--hover);
  }
  .sites {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 0 8px;
  }
  .site {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    height: 32px;
    padding: 0 8px;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--fs-md);
    text-align: left;
    cursor: pointer;
  }
  .site:hover {
    background: var(--hover);
  }
  .site img {
    width: 14px;
    height: 14px;
    flex: none;
  }
  .site span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
