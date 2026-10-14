<script lang="ts">
  import { createEventDispatcher, onMount, tick } from "svelte";
  import { flip } from "svelte/animate";
  import { cubicOut } from "svelte/easing";
  import Icon from "./Icon.svelte";
  import type { IconName } from "./icons";
  import ProviderLogo from "./ProviderLogo.svelte";
  import Viz from "./viz/Viz.svelte";
  import { api, brain, onRunEvent, type GitStatus, type Project, type ProviderStatus, type SessionMeta } from "./api";
  import { faviconUrl, history, pins } from "./browserData";
  import { openBrainNote } from "./brainStore";
  import { hasMark } from "./providerMarks";
  import { PROVIDER_ORDER, isUsable, nameOf, stateLabel } from "./providerRows";
  import { loadRemoteInfo, remote as server, remoteInfo } from "./remote";
  import { folderName, hostOf } from "./tabs";
  import { HOME_WIDGETS, customWidgets, hiddenWidgets, loadCustomWidgets, toggleWidget, widgetOrder, type WidgetId } from "./homeWidgets";
  import { arrange, moveBy, moveTo, withShown } from "./homeLayout";
  import { popover, placeAbove } from "./popover";
  import { sessionTone, TONE_WORDS } from "./planModel";
  import { tooltip } from "./tooltip";

  export let threads: SessionMeta[] = [];
  export let running: Set<string> = new Set();
  // Sessions waiting on an approval or a question.
  export let waiting: Set<string> = new Set();
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
    remoteSetup: void;
  }>();

  const ROWS = 4;
  const ICONS: Record<string, IconName> = { recent: "chat", agents: "bot", projects: "project", git: "branch", server: "server", sites: "globe" };

  // Every card in its order: the built-ins, then the widgets agents wrote
  // (Brain notes under widgets/), as the person arranged them.
  $: customs = $customWidgets ?? [];
  $: order = arrange($widgetOrder, [...HOME_WIDGETS.map((w): string => w.id), ...customs.map((c) => c.id)]);
  $: shown = (id: WidgetId) => !$hiddenWidgets.includes(id);
  $: cards = drag ? drag.order : order.filter((id) => shown(id));
  $: labelOf = (id: string) => HOME_WIDGETS.find((w) => w.id === id)?.label ?? customs.find((c) => c.id === id)?.title ?? id;
  $: titleOf = (id: string) => (id === "sites" ? ($pins.length ? "Pinned" : "Most visited") : labelOf(id));
  const domId = (id: string) => `home-${id.replace(/[^\w-]+/g, "-")}`;

  let customizeBtn: HTMLButtonElement | null = null;
  let customizing = false;
  let customizeStyle = "";

  function toggleCustomize() {
    customizing = !customizing;
    if (customizing) customizeStyle = placeAbove(customizeBtn, 260, true);
  }

  // A new order; custom cards not read yet keep their saved places.
  function commit(next: string[]) {
    widgetOrder.set([...next, ...$widgetOrder.filter((id) => !next.includes(id))]);
  }

  function step(id: string, by: number) {
    commit(moveBy(order, id, by));
  }

  // Dragging a card by its header: it follows the pointer and the others
  // make room as it passes over them. Esc puts everything back.
  const DRAG_START = 4;
  let grid: HTMLDivElement | null = null;
  let press: { id: string; x: number; y: number; card: HTMLElement } | null = null;
  let drag: { id: string; order: string[]; grabX: number; grabY: number; dx: number; dy: number } | null = null;
  // The card just swapped with: not again until the pointer leaves it.
  let swapped = "";
  let pointer = { x: 0, y: 0 };

  function onHeaderDown(e: PointerEvent, id: string) {
    if (e.button !== 0 || (e.target as HTMLElement).closest("button, a, input")) return;
    const head = e.currentTarget as HTMLElement;
    const card = head.closest<HTMLElement>(".card");
    if (!card) return;
    // No text selection while the card moves, and the release is heard
    // even outside the window.
    e.preventDefault();
    head.setPointerCapture(e.pointerId);
    press = { id, x: e.clientX, y: e.clientY, card };
  }

  async function onMove(e: PointerEvent) {
    if (!press) return;
    if (!drag) {
      if (Math.hypot(e.clientX - press.x, e.clientY - press.y) < DRAG_START) return;
      const r = press.card.getBoundingClientRect();
      drag = { id: press.id, order: order.filter((id) => shown(id)), grabX: press.x - r.left, grabY: press.y - r.top, dx: 0, dy: 0 };
      swapped = "";
    }
    pointer = { x: e.clientX, y: e.clientY };
    edgeScroll(e.clientY);
    const over = cardAt(e.clientX, e.clientY);
    if (over && over !== swapped) {
      drag.order = moveTo(drag.order, drag.id, drag.order.indexOf(over));
      swapped = over;
      await tick();
    } else if (!over) {
      swapped = "";
    }
    follow();
  }

  // The card under the pointer, by the slots the grid lays out (offsets
  // ignore the moving cards' transforms).
  function cardAt(x: number, y: number): string {
    const g = grid;
    if (!g || !drag) return "";
    const box = g.getBoundingClientRect();
    const px = x - box.left;
    const py = y - box.top;
    for (const el of g.querySelectorAll<HTMLElement>(":scope > .card")) {
      const id = el.dataset.id ?? "";
      if (!id || id === drag.id) continue;
      if (px >= el.offsetLeft && px < el.offsetLeft + el.offsetWidth && py >= el.offsetTop && py < el.offsetTop + el.offsetHeight) return id;
    }
    return "";
  }

  // Keep the dragged card under the pointer, wherever its slot is now.
  function follow() {
    const g = grid;
    if (!g || !drag || !press) return;
    const box = g.getBoundingClientRect();
    drag.dx = pointer.x - drag.grabX - (box.left + press.card.offsetLeft);
    drag.dy = pointer.y - drag.grabY - (box.top + press.card.offsetTop);
    drag = drag;
  }

  // Home's cards scroll under the composer: near an edge, scroll on.
  function edgeScroll(y: number) {
    const scroller = grid?.parentElement;
    if (!scroller) return;
    const r = scroller.getBoundingClientRect();
    if (y < r.top + 40) scroller.scrollTop -= 12;
    else if (y > r.bottom - 40) scroller.scrollTop += 12;
  }

  function onUp() {
    if (drag) commit(withShown(order, drag.order));
    press = null;
    drag = null;
  }

  function cancelDrag() {
    press = null;
    drag = null;
  }

  function onKey(e: KeyboardEvent) {
    if (e.key !== "Escape" || !drag) return;
    e.preventDefault();
    e.stopPropagation();
    cancelDrag();
  }

  let projects: Project[] = [];
  let projectsFor: boolean | null = null;
  $: if (remote !== projectsFor) {
    projectsFor = remote;
    void loadProjects(remote);
  }

  // A call made while the app still starts can fail: try again a few times,
  // and whenever the window comes back into focus.
  async function loadProjects(onServer: boolean, tries = 3) {
    const list = await (onServer ? server.projects() : brain.projects()).catch(() => null);
    if (onServer !== remote) return;
    if (list) projects = list.filter((p) => p.folder).slice(0, ROWS);
    else if (tries > 1) window.setTimeout(() => void loadProjects(onServer, tries - 1), 2000);
  }
  onMount(() => {
    const onFocus = () => void loadProjects(remote, 1);
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  });

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

  // Git: the draft's folder on this PC (a server folder has no status here).
  let git: GitStatus | null = null;
  let gitFor: string | null = null;
  $: gitFolder = remote ? "" : folder;
  $: if (gitFolder !== gitFor) {
    gitFor = gitFolder;
    void loadGit();
  }

  async function loadGit() {
    const dir = gitFor;
    const s = dir ? await api.gitStatus(dir).catch(() => null) : null;
    if (dir === gitFor) git = s;
  }

  let syncing = false;
  async function syncNow() {
    syncing = true;
    try {
      await server.sync();
      await loadRemoteInfo();
    } catch {
      // The card shows the server as offline; nothing else to say here.
    } finally {
      syncing = false;
    }
  }

  // Branches and remotes change outside Parzi: refresh while Home is up.
  onMount(() => {
    void loadCustomWidgets();
    const refresh = () => {
      if (shown("git")) void loadGit();
    };
    const onFocus = () => {
      refresh();
      void loadCustomWidgets();
    };
    const timer = window.setInterval(refresh, 30_000);
    window.addEventListener("focus", onFocus);
    // An agent that wrote or deleted a note may have changed a widget.
    const unRun = onRunEvent((e) => {
      if (e.kind === "tool_result" && e.ok && /brain[._](write|delete)/.test(e.name)) void loadCustomWidgets();
    });
    return () => {
      window.clearInterval(timer);
      window.removeEventListener("focus", onFocus);
      void unRun.then((off) => off());
    };
  });

  function agoSecs(t: number) {
    return t ? ago(new Date(t * 1000).toISOString()) : "";
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

<svelte:window on:pointermove={onMove} on:pointerup={onUp} on:pointercancel={cancelDrag} on:keydown={onKey} />

<div class="home" class:dragging={!!drag} bind:this={grid}>
  {#each cards as id (id)}
    {@const custom = customs.find((c) => c.id === id)}
    <section
      class="card"
      class:lifted={drag?.id === id}
      data-id={id}
      style:transform={drag?.id === id ? `translate(${drag.dx}px, ${drag.dy}px)` : null}
      aria-labelledby={domId(id)}
      animate:flip={{ duration: drag?.id === id ? 0 : 240, easing: cubicOut }}
    >
      <!-- Dragging the header moves the card; Customize moves it by keyboard. -->
      <!-- svelte-ignore a11y-no-static-element-interactions -->
      <header on:pointerdown={(e) => onHeaderDown(e, id)}>
        <h3 id={domId(id)}><Icon name={ICONS[id] ?? "chart"} /><span class="h-text">{titleOf(id)}</span></h3>
        {#if id === "recent"}
          {#if threads.length}<button class="more" on:click={() => dispatch("allSessions")}>All</button>{/if}
        {:else if id === "agents"}
          <button class="more" on:click={() => dispatch("agents")}>Manage</button>
        {:else if id === "projects"}
          <button class="more" on:click={() => dispatch("brain")}>Brain</button>
        {:else if id === "git"}
          {#if gitFolder}<button class="more" on:click={() => loadGit()}>Refresh</button>{/if}
        {:else if id === "server"}
          {#if $remoteInfo}<button class="more" disabled={syncing} on:click={syncNow}>{syncing ? "Syncing…" : "Sync now"}</button>{/if}
        {:else if custom}
          <button class="more" on:click={() => openBrainNote(custom.id)}>Edit</button>
        {/if}
      </header>

      {#if id === "recent"}
        {#if recent.length}
          <div class="rows" data-ui-list>
            {#each recent as s (s.id)}
              {@const tone = sessionTone(s, running, waiting)}
              <button class="row" on:click={() => dispatch("openSession", { id: s.id })}>
                <span class="dot {tone}" role="img" aria-label={TONE_WORDS[tone]} />
                <span class="name">{s.title || "Untitled session"}</span>
                <span class="meta">{ago(s.updated)}</span>
              </button>
            {/each}
          </div>
        {:else}
          <p class="empty">Your sessions show up here.</p>
        {/if}
      {:else if id === "agents"}
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
      {:else if id === "projects"}
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
      {:else if id === "git"}
        {#if git}
          <div class="lines">
            <div class="line">
              <span class="name">{git.branch}</span>
              <span class="meta num">{git.upstream ? (git.ahead || git.behind ? `${git.ahead} ahead, ${git.behind} behind` : "Up to date") : "No upstream"}</span>
            </div>
            <div class="line">
              <span class="name" class:quiet={!git.changed && !git.untracked}>{git.changed || git.untracked ? `${git.changed} changed, ${git.untracked} new` : "Clean"}</span>
              <span class="meta">{folderName(gitFolder)}</span>
            </div>
            {#if git.last_subject}
              <div class="line">
                <span class="name quiet">{git.last_subject}</span>
                <span class="meta">{agoSecs(git.last_time)}</span>
              </div>
            {/if}
          </div>
        {:else}
          <p class="empty">{remote ? "Git status shows for folders on this PC." : gitFolder ? `${folderName(gitFolder)} isn't a git repository.` : "Pick a project to see its branch and changes."}</p>
        {/if}
      {:else if id === "server"}
        {#if $remoteInfo}
          <div class="lines">
            <div class="line">
              <span class="dot" class:live={$remoteInfo.alive} />
              <span class="name">{$remoteInfo.user}@{$remoteInfo.host}</span>
              <span class="meta">{$remoteInfo.alive ? "Online" : "Offline"}</span>
            </div>
            <div class="line">
              <span class="name quiet">{$remoteInfo.synced_at ? `Synced ${ago(new Date($remoteInfo.synced_at).toISOString())}` : "Not synced yet"}</span>
              <span class="meta">{$remoteInfo.version ? `Parzi ${$remoteInfo.version}` : ""}</span>
            </div>
          </div>
        {:else}
          <p class="empty">Run sessions on your own Linux server and keep notes, settings and sessions in step across devices.</p>
          <div class="cta"><button class="more" on:click={() => dispatch("remoteSetup")}>Set up a server</button></div>
        {/if}
      {:else if id === "sites"}
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
      {:else if custom}
        {#if custom.spec}
          <div class="widget"><Viz spec={custom.spec} bare /></div>
        {:else}
          <p class="empty">This note has no viz block yet. Ask an agent to add one.</p>
        {/if}
      {/if}
    </section>
  {/each}
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
    {#each order as id, i (id)}
      <div class="opt" animate:flip={{ duration: 200, easing: cubicOut }}>
        <label>
          <input type="checkbox" checked={shown(id)} on:change={() => toggleWidget(id)} />
          <span>{labelOf(id)}</span>
        </label>
        <button class="mv" aria-label="Move {labelOf(id)} up" use:tooltip={"Move up"} disabled={i === 0} on:click={() => step(id, -1)}>
          <Icon name="arrowUp" />
        </button>
        <button class="mv" aria-label="Move {labelOf(id)} down" use:tooltip={"Move down"} disabled={i === order.length - 1} on:click={() => step(id, 1)}>
          <Icon name="arrowDown" />
        </button>
      </div>
    {/each}
    <p class="menu-foot">Ask an agent to add a Home widget.</p>
  </div>
{/if}

<style>
  /* Positioned: the cards' offsets (their grid slots) are relative to it. */
  .home {
    position: relative;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
    gap: 12px;
    width: 100%;
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
    /* Same resting elevation as the composer: cards and omnibar are one family. */
    box-shadow: var(--glass-shadow);
    /* Settles into its slot when let go. */
    transition:
      transform 240ms var(--ease-out),
      box-shadow 240ms var(--ease-out);
  }
  /* The card being dragged: above the rest, tracking the pointer exactly. */
  .card.lifted {
    position: relative;
    z-index: 5;
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.55);
    transition: none;
  }
  /* The header is the handle. */
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    height: 24px;
    padding: 0 4px 0 8px;
    cursor: grab;
    touch-action: none;
    user-select: none;
  }
  .home.dragging,
  .home.dragging * {
    cursor: grabbing;
  }
  h3 {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    margin: 0;
    font-size: var(--fs-md);
    font-weight: 600;
    color: var(--muted);
  }
  .h-text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .widget {
    min-width: 0;
    padding: 2px 8px 4px;
  }
  .more {
    height: 24px;
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
  /* A session's state: running, needs you, failed, done. */
  .dot.need {
    background: var(--warn);
  }
  .dot.bad {
    background: var(--bad);
  }
  .dot.ok {
    background: var(--muted);
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
    width: 260px;
    padding: 4px;
    overflow-y: auto;
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
    gap: 8px;
    height: 32px;
    padding: 0 4px 0 8px;
    border-radius: var(--radius);
  }
  .opt:hover {
    background: var(--hover);
  }
  .opt label {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 10px;
    min-width: 0;
    height: 100%;
    color: var(--text);
    font-size: var(--fs-md);
    cursor: pointer;
  }
  .opt label span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mv {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    flex: none;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--muted);
    cursor: pointer;
  }
  .mv:hover:not(:disabled) {
    background: var(--hover);
    color: var(--text);
  }
  .mv:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .menu-foot {
    margin: 4px 0 0;
    padding: 8px 8px 6px;
    border-top: 1px solid var(--line);
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .lines {
    display: flex;
    flex-direction: column;
  }
  .line {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 32px;
    padding: 0 8px;
    color: var(--text);
  }
  .quiet {
    color: var(--muted);
  }
  .cta {
    padding: 4px;
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
