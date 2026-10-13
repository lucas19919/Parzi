<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import type { IconName } from "./icons";
  import ProviderLogo from "./ProviderLogo.svelte";
  import Viz from "./viz/Viz.svelte";
  import { api, brain, onRunEvent, type GitStatus, type Project, type ProviderStatus, type SessionMeta } from "./api";
  import { faviconUrl, history, pins } from "./browserData";
  import { openBrainNote } from "./brainStore";
  import { ago } from "./format";
  import { hasMark } from "./providerMarks";
  import { PROVIDER_ORDER, isUsable, nameOf, stateLabel } from "./providerRows";
  import { loadRemoteInfo, remote as server, remoteInfo } from "./remote";
  import { folderName, hostOf } from "./tabs";
  import { HOME_WIDGETS, customWidgets, hiddenWidgets, loadCustomWidgets, toggleWidget, widgetOrder, type CustomWidget, type WidgetId } from "./homeWidgets";
  import { arrange, clamp, fades, mergeOrder, moveBy, moveTo, nearestSlot, peekHeight, rowsOf, withShown, type Box } from "./homeLayout";
  import { popover, placeAbove } from "./popover";
  import { sessionTone, TONE_WORDS } from "./sessionTone";
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

  // A drag freezes the cards (and the widgets' contents) it started with,
  // so a widget reload meanwhile can't re-render the list under the pointer.
  let drag: { id: string; ids: string[]; customs: CustomWidget[] } | null = null;

  // Every card in its order: the built-ins, then the widgets agents wrote
  // (Brain notes under widgets/), as the person arranged them.
  $: liveCustoms = $customWidgets ?? [];
  $: customs = drag ? drag.customs : liveCustoms;
  $: order = arrange($widgetOrder, [...HOME_WIDGETS.map((w): string => w.id), ...liveCustoms.map((c) => c.id)]);
  $: shown = (id: WidgetId) => !$hiddenWidgets.includes(id);
  $: visible = order.filter((id) => shown(id));
  $: cards = drag ? drag.ids : visible;
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

  // A new order saved over the old one; custom cards not read yet keep
  // their places.
  function commit(next: string[]) {
    widgetOrder.set(mergeOrder($widgetOrder, next));
  }

  // Customize's arrows: a shown card steps past the shown card next to it,
  // as a drag would move it (every press moves something on Home); a
  // hidden card steps through the list.
  $: stepList = (id: string) => (shown(id) ? visible : order);
  $: canStep = (id: string, by: number) => {
    const list = stepList(id);
    const at = list.indexOf(id) + by;
    return at >= 0 && at < list.length;
  };

  function step(id: string, by: number) {
    commit(shown(id) ? withShown(order, moveBy(visible, id, by)) : moveBy(order, id, by));
  }

  // Dragging a card: it lifts and follows the pointer inside the grid, an
  // outline marks the slot it will land in, and letting go saves the order.
  // Pointer events, since Tauri's file drop handling blocks HTML5 drag and
  // drop on Windows. The follow transform is written straight to the card,
  // so a move costs no Svelte update; Esc, a lost pointer or the window
  // losing focus put the card back.
  const DRAG_START = 4;
  const EDGE = 40;
  const EDGE_SPEED = 14;
  let scroller: HTMLDivElement | null = null;
  let grid: HTMLDivElement | null = null;
  let press: { id: string; x: number; y: number; card: HTMLElement } | null = null;
  // The drop slot's outline, in grid px.
  let slot: Box | null = null;
  let target = -1;
  let slots: Box[] = [];
  let grab = { x: 0, y: 0 };
  let pointer = { x: 0, y: 0 };
  let scrollFrame = 0;

  function onPress(e: PointerEvent, id: string) {
    if (e.button !== 0 || press || (e.target as HTMLElement).closest("button, a, input, label, select, textarea, .widget")) return;
    const card = e.currentTarget as HTMLElement;
    // No text selection while the card moves, and the release is heard
    // even outside the window.
    e.preventDefault();
    try {
      card.setPointerCapture(e.pointerId);
    } catch {
      // A pointer that is already gone: the next move ends the press.
    }
    press = { id, x: e.clientX, y: e.clientY, card };
  }

  function onMove(e: PointerEvent) {
    if (!press) return;
    // A release this window never heard: let go without moving anything.
    if (!(e.buttons & 1)) return reset();
    pointer = { x: e.clientX, y: e.clientY };
    if (!drag) {
      if (Math.hypot(e.clientX - press.x, e.clientY - press.y) < DRAG_START) return;
      lift();
    }
    place();
  }

  // The slots are measured once: nothing moves under the card until it drops.
  function lift() {
    const g = grid;
    if (!g || !press) return;
    const ids = visible;
    const boxes = new Map<string, Box>();
    for (const el of g.querySelectorAll<HTMLElement>(":scope > .card")) {
      boxes.set(el.dataset.id ?? "", { left: el.offsetLeft, top: el.offsetTop, width: el.offsetWidth, height: el.offsetHeight });
    }
    slots = ids.map((id) => boxes.get(id) ?? { left: 0, top: 0, width: 0, height: 0 });
    const r = press.card.getBoundingClientRect();
    grab = { x: press.x - r.left, y: press.y - r.top };
    target = ids.indexOf(press.id);
    slot = slots[target] ?? null;
    drag = { id: press.id, ids, customs: liveCustoms };
  }

  // The card under the pointer, kept inside the grid (and the pointer
  // inside the visible list), and the slot nearest its centre.
  function place() {
    const g = grid;
    const sc = scroller;
    const card = press?.card;
    if (!g || !sc || !card || !drag) return;
    const home = slots[drag.ids.indexOf(drag.id)];
    if (!home) return;
    const gb = g.getBoundingClientRect();
    const sb = sc.getBoundingClientRect();
    const py = clamp(pointer.y, sb.top, sb.bottom);
    const x = clamp(pointer.x - grab.x - gb.left, 0, Math.max(0, gb.width - home.width));
    const y = clamp(py - grab.y - gb.top, 0, Math.max(0, gb.height - home.height));
    card.style.transform = `translate(${x - home.left}px, ${y - home.top}px)`;
    const to = nearestSlot(slots, x + home.width / 2, y + home.height / 2);
    if (to >= 0 && to !== target) {
      target = to;
      slot = slots[to];
    }
    if (!scrollFrame && edgeSpeed(sb)) scrollFrame = requestAnimationFrame(edgeStep);
  }

  // Near the list's top or bottom edge, or past it, the list scrolls a
  // little every frame, faster the deeper the pointer goes, and stops at
  // either end.
  function edgeSpeed(sb: DOMRect): number {
    const y = pointer.y;
    if (y < sb.top + EDGE) return -Math.ceil(EDGE_SPEED * Math.min(1, (sb.top + EDGE - y) / EDGE));
    if (y > sb.bottom - EDGE) return Math.ceil(EDGE_SPEED * Math.min(1, (y - sb.bottom + EDGE) / EDGE));
    return 0;
  }

  function edgeStep() {
    scrollFrame = 0;
    const sc = scroller;
    if (!sc || !drag) return;
    const next = clamp(sc.scrollTop + edgeSpeed(sc.getBoundingClientRect()), 0, sc.scrollHeight - sc.clientHeight);
    if (next === sc.scrollTop) return;
    sc.scrollTop = next;
    place();
  }

  function onUp() {
    if (!press) return;
    const d = drag;
    const moved = d && target >= 0 ? moveTo(d.ids, d.id, target) : null;
    reset();
    if (moved) commit(withShown(order, moved));
  }

  // Back to rest: no transform left on the card, nothing frozen.
  function reset() {
    cancelAnimationFrame(scrollFrame);
    scrollFrame = 0;
    press?.card.style.removeProperty("transform");
    press = null;
    drag = null;
    slot = null;
    target = -1;
    slots = [];
  }

  // The list's height and fades: it stops partway down a row when the
  // cards don't all fit (homeLayout.peekHeight), so the next ones peek out.
  let peek = 0;
  let fadeTop = 0;
  let fadeBottom = 32;

  function measure() {
    const sc = scroller;
    const g = grid;
    if (!sc || !g || drag) return;
    const avail = sc.parentElement?.clientHeight ?? 0;
    if (avail > 0) {
      const rows = rowsOf([...g.querySelectorAll<HTMLElement>(":scope > .card")].map((el) => ({ top: g.offsetTop + el.offsetTop, height: el.offsetHeight })));
      const h = peekHeight(avail, rows, sc.scrollHeight);
      peek = h < avail ? h : 0;
    }
    edges();
  }

  function edges() {
    const sc = scroller;
    if (!sc) return;
    const f = fades(sc.scrollTop, sc.scrollHeight, sc.clientHeight);
    fadeTop = f.top;
    fadeBottom = f.bottom;
  }

  // Cards that change after the list is measured (a widget loads, the
  // window resizes) measure it again.
  function watchSize(node: HTMLElement) {
    const ro = new ResizeObserver(() => measure());
    ro.observe(node);
    if (node.parentElement) ro.observe(node.parentElement);
    return { destroy: () => ro.disconnect() };
  }

  let projects: Project[] = [];
  let projectsFor: boolean | null = null;
  let projectsTimer = 0;
  $: if (remote !== projectsFor) {
    projectsFor = remote;
    void loadProjects(remote);
  }

  // A call made while the app still starts can fail: try again a few times,
  // and whenever the window comes back into focus.
  async function loadProjects(onServer: boolean, tries = 5) {
    window.clearTimeout(projectsTimer);
    const list = await (onServer ? server.projects() : brain.projects()).catch(() => null);
    if (onServer !== remote) return;
    if (Array.isArray(list)) projects = list.filter((p) => p?.folder).slice(0, ROWS);
    else if (tries > 1) projectsTimer = window.setTimeout(() => void loadProjects(onServer, tries - 1), 2000);
  }

  $: recent = threads.slice(0, ROWS);
  $: agents = PROVIDER_ORDER.map((id) => board.find((b) => b?.provider === id))
    .filter((p): p is ProviderStatus => !!p && p.state !== "not_installed" && p.state !== "disabled")
    .slice(0, ROWS);
  // Pinned sites first; until there are some, the most visited.
  $: sites = ($pins.length ? $pins : [...$history].sort((a, b) => b.count - a.count)).filter((s) => s?.url).slice(0, 8);

  // The folder when its name says something the title doesn't, else the notes.
  function projectMeta(p: Project) {
    const dir = folderName(p.folder);
    if (dir.toLowerCase() !== (p.title ?? "").toLowerCase()) return dir;
    const n = p.notes?.length ?? 0;
    return n ? `${n} note${n === 1 ? "" : "s"}` : "";
  }

  // Git: the draft's folder on this PC (a server folder has no status here).
  // A call that fails (the app still starting) is not "no repository": the
  // card says it couldn't read it and tries again.
  let git: GitStatus | null = null;
  let gitFailed = false;
  let gitFor: string | null = null;
  let gitTimer = 0;
  $: gitFolder = remote ? "" : folder;
  $: if (gitFolder !== gitFor) {
    gitFor = gitFolder;
    void loadGit();
  }

  async function loadGit(tries = 3) {
    window.clearTimeout(gitTimer);
    const dir = gitFor;
    let status: GitStatus | null = null;
    let failed = false;
    if (dir) {
      try {
        status = await api.gitStatus(dir);
      } catch {
        failed = true;
      }
    }
    if (dir !== gitFor) return;
    if (failed) {
      gitFailed = !git;
      if (tries > 1) gitTimer = window.setTimeout(() => void loadGit(tries - 1), 3000);
      return;
    }
    git = status;
    gitFailed = false;
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

  // Branches, remotes, projects and widgets change outside Parzi: refresh
  // while Home is up, and on coming back to the window (at most every few
  // seconds, since a click into the window is a focus too).
  onMount(() => {
    void loadCustomWidgets();
    let lastFocus = 0;
    const onFocus = () => {
      if (Date.now() - lastFocus < 5000) return;
      lastFocus = Date.now();
      void loadProjects(remote, 1);
      if (shown("git")) void loadGit(1);
      void loadCustomWidgets();
    };
    const onBlur = () => reset();
    // Capture: Esc puts a dragged card back before anything else reads it.
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || !press) return;
      e.preventDefault();
      e.stopPropagation();
      reset();
    };
    const timer = window.setInterval(() => {
      if (shown("git")) void loadGit(1);
    }, 30_000);
    window.addEventListener("focus", onFocus);
    window.addEventListener("blur", onBlur);
    window.addEventListener("keydown", onKey, true);
    // An agent that wrote or deleted a note may have changed a widget.
    const unRun = onRunEvent((e) => {
      if (e.kind === "tool_result" && e.ok && /brain[._](write|delete)/.test(e.name)) void loadCustomWidgets();
    });
    return () => {
      reset();
      window.clearInterval(timer);
      window.clearTimeout(projectsTimer);
      window.clearTimeout(gitTimer);
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("keydown", onKey, true);
      void unRun.then((off) => off()).catch(() => {});
    };
  });

  // A site without a favicon keeps its row, just no picture.
  function noIcon(e: Event) {
    (e.currentTarget as HTMLImageElement).style.visibility = "hidden";
  }

  function usage(p: ProviderStatus) {
    const u = p.usage?.[0];
    return u ? { label: u.label, pct: Math.max(0, Math.min(100, Math.round(u.used_percent || 0))) } : null;
  }
</script>

<svelte:window on:pointermove={onMove} on:pointerup={onUp} on:pointercancel={reset} />

<div
  class="home-scroll"
  bind:this={scroller}
  use:watchSize
  style:max-height={peek ? `${peek}px` : null}
  style:--fade-top="{fadeTop}px"
  style:--fade-bottom="{fadeBottom}px"
  on:scroll={edges}
>
  <div class="cards" class:dragging={!!drag} bind:this={grid}>
    {#each cards as id (id)}
      {@const custom = customs.find((c) => c.id === id)}
      <!-- Dragging a card moves it; Customize moves it by keyboard. -->
      <!-- svelte-ignore a11y-no-static-element-interactions -->
      <section
        class="card"
        class:lifted={drag?.id === id}
        data-id={id}
        aria-labelledby={domId(id)}
        on:pointerdown={(e) => onPress(e, id)}
        on:lostpointercapture={() => {
          if (press?.id === id) reset();
        }}
      >
        <header>
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
                  <span class="meta">{ago(Date.parse(s.updated))}</span>
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
                  <span class="meta">{git.last_time ? ago(git.last_time * 1000) : ""}</span>
                </div>
              {/if}
            </div>
          {:else}
            <p class="empty">
              {remote
                ? "Git status shows for folders on this PC."
                : !gitFolder
                  ? "Pick a project to see its branch and changes."
                  : gitFailed
                    ? `Couldn't read the git status of ${folderName(gitFolder)}. Refresh to try again.`
                    : `${folderName(gitFolder)} isn't a git repository.`}
            </p>
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
                <span class="name quiet">{$remoteInfo.synced_at ? `Synced ${ago($remoteInfo.synced_at)}` : "Not synced yet"}</span>
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
    {#if slot}
      <div class="slot" aria-hidden="true" style:left="{slot.left}px" style:top="{slot.top}px" style:width="{slot.width}px" style:height="{slot.height}px" />
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
</div>

{#if customizing}
  <div class="home-menu" style={customizeStyle} use:popover={{ anchor: customizeBtn, close: () => (customizing = false) }}>
    <p class="menu-head">Show on Home</p>
    {#each order as id (id)}
      <div class="opt">
        <label>
          <input type="checkbox" checked={shown(id)} on:change={() => toggleWidget(id)} />
          <span>{labelOf(id)}</span>
        </label>
        <button class="mv" aria-label="Move {labelOf(id)} up" use:tooltip={"Move up"} disabled={!canStep(id, -1)} on:click={() => step(id, -1)}>
          <Icon name="arrowUp" />
        </button>
        <button class="mv" aria-label="Move {labelOf(id)} down" use:tooltip={"Move down"} disabled={!canStep(id, 1)} on:click={() => step(id, 1)}>
          <Icon name="arrowDown" />
        </button>
      </div>
    {/each}
    <p class="menu-foot">Ask an agent to add a Home widget.</p>
  </div>
{/if}

<style>
  /* The list scrolls with no scrollbar; its edges fade (sizes from
     homeLayout.fades): the top once scrolled, the bottom deeper while more
     is below. The side padding leaves room for the cards' shadows. */
  .home-scroll {
    position: relative;
    flex: 0 1 auto;
    min-height: 0;
    padding: 16px 24px 32px;
    overflow-y: auto;
    scrollbar-width: none;
    mask-image: linear-gradient(
      to bottom,
      transparent,
      #000 var(--fade-top, 0px),
      #000 calc(100% - var(--fade-bottom, 32px)),
      transparent
    );
  }
  .home-scroll::-webkit-scrollbar {
    display: none;
  }
  /* Positioned: the cards' offsets (their grid slots) are relative to it. */
  .cards {
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
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.28);
    cursor: grab;
    touch-action: none;
  }
  /* The card being dragged: above the rest, tracking the pointer exactly,
     and opaque so the card under it doesn't show through. */
  .card.lifted {
    position: relative;
    z-index: 5;
    background: var(--panel);
    box-shadow: var(--glass-shadow);
  }
  /* Where the dragged card lands. */
  .slot {
    position: absolute;
    z-index: 1;
    border: 1px dashed var(--muted);
    border-radius: var(--radius-lg);
    background: var(--hover);
    pointer-events: none;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    height: 24px;
    padding: 0 4px 0 8px;
    user-select: none;
  }
  .card .widget {
    cursor: default;
  }
  .cards.dragging,
  .cards.dragging * {
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
