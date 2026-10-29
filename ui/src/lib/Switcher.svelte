<script lang="ts">
  import { createEventDispatcher, tick } from "svelte";
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import Icon from "./Icon.svelte";
  import ProviderLogo from "./ProviderLogo.svelte";
  import { hasMark } from "./providerMarks";
  import type { IconName } from "./icons";
  import type { SessionMeta } from "./api";
  import { shownOf } from "./providerRows";
  import { board } from "./providerStore";
  import { folderName, type Tab } from "./tabs";
  import { setOverlay } from "./overlay";
  import { tooltip } from "./tooltip";

  export let open = false;
  export let threads: SessionMeta[] = [];
  export let tabs: Tab[] = [];
  export let activeTabId = "";

  const dispatch = createEventDispatcher<{
    close: void;
    selectTab: { id: string };
    openSession: { id: string };
    deleteSession: { id: string };
    newSession: void;
    newPage: void;
    settings: void;
    brain: void;
    history: void;
    previews: void;
    setup: void;
  }>();

  interface Item {
    id: string;
    group: string;
    icon: IconName;
    // The agent behind a session row: its logo instead of the generic bot.
    provider?: string;
    title: string;
    sub: string;
    // Words that find it without showing (actions carry no subtitle).
    keys?: string;
    run: () => void;
    remove?: () => void;
  }

  let query = "";
  let index = 0;
  let inputEl: HTMLInputElement | null = null;

  $: setOverlay("switcher", open);
  $: if (open) void reset();

  async function reset() {
    query = "";
    index = 0;
    await tick();
    inputEl?.focus();
  }

  function hit(q: string, ...fields: (string | undefined)[]) {
    return !q || fields.some((f) => f?.toLowerCase().includes(q));
  }

  // The component gallery is a design tool: dev builds only.
  const ACTIONS: Omit<Item, "group">[] = [
    { id: "new-session", icon: "chat", title: "New session", sub: "", keys: "draft chat", run: () => dispatch("newSession") },
    { id: "new-page", icon: "globe", title: "New page", sub: "", keys: "web browser", run: () => dispatch("newPage") },
    { id: "history", icon: "clock", title: "History", sub: "", keys: "pages visited bookmarks downloads", run: () => dispatch("history") },
    { id: "brain", icon: "brain", title: "Brain", sub: "", keys: "notes projects", run: () => dispatch("brain") },
    ...(import.meta.env.DEV
      ? [{ id: "previews", icon: "spark" as IconName, title: "Component previews", sub: "", keys: "omnibar gallery", run: () => dispatch("previews") }]
      : []),
    { id: "settings", icon: "settings", title: "Settings", sub: "", keys: "agents providers appearance theme system connectors apps remote mcp", run: () => dispatch("settings") },
    { id: "setup", icon: "wrench", title: "Set up Parzi", sub: "", keys: "agents browser tools remote wizard", run: () => dispatch("setup") },
  ];
  const MAX_SESSIONS = 50;

  const agentOf = (model: string | undefined): string => (model ? model.split("/")[0] : "");

  function build(q: string, tabs: Tab[], threads: SessionMeta[], activeTabId: string): Item[] {
    const openSessions = new Set(tabs.map((t) => t.sessionId).filter(Boolean));
    const byId = new Map(threads.map((t) => [t.id, t]));
    return [
      ...ACTIONS.filter((c) => hit(q, c.title, c.keys)).map((c): Item => ({ ...c, group: "Actions" })),
      ...tabs
        .filter((t) => hit(q, t.title, t.url))
        .map(
          (t): Item => ({
            id: `tab:${t.id}`,
            group: "Tabs",
            icon: t.kind === "page" ? "globe" : "chat",
            provider: t.kind === "session" ? agentOf(t.sessionId ? byId.get(t.sessionId)?.model : t.composer?.model) : undefined,
            title: t.title || "New session",
            sub: t.id === activeTabId ? "Current tab" : t.url || "Open tab",
            run: () => dispatch("selectTab", { id: t.id }),
          }),
        ),
      ...threads
        .filter((t) => !openSessions.has(t.id) && hit(q, t.title, t.model, t.cwd))
        .slice(0, MAX_SESSIONS)
        .map(
          (t): Item => ({
            id: `session:${t.id}`,
            group: "Sessions",
            icon: "chat",
            provider: agentOf(t.model),
            title: t.title || "Untitled session",
            sub: [t.model && t.model !== "auto" ? shownOf(t.model, $board).name : "", t.cwd ? folderName(t.cwd) : ""].filter(Boolean).join(" · "),
            run: () => dispatch("openSession", { id: t.id }),
            remove: () => dispatch("deleteSession", { id: t.id }),
          }),
        ),
    ];
  }

  $: q = query.trim().toLowerCase();
  // Closed, the list costs nothing: tab and thread churn (every page
  // event, every refresh) only rebuilds it while the switcher is up.
  $: items = open ? build(q, tabs, threads, activeTabId) : [];
  $: if (index >= items.length) index = 0;

  function choose(item: Item | undefined) {
    if (!item) return;
    item.run();
    dispatch("close");
  }

  function onKey(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === "Escape") {
      e.preventDefault();
      dispatch("close");
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      index = (index + 1) % Math.max(1, items.length);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      index = (index - 1 + items.length) % Math.max(1, items.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      choose(items[index]);
    }
  }
</script>

<svelte:window on:keydown={onKey} />

{#if open}
  <div class="backdrop" transition:fade={{ duration: 120 }} on:click|self={() => dispatch("close")} role="presentation">
    <div class="box" role="dialog" aria-modal="true" aria-label="Search" transition:fly={{ y: -6, duration: 140, easing: cubicOut }}>
      <div class="head">
        <Icon name="search" />
        <input bind:this={inputEl} bind:value={query} aria-label="Search" placeholder="Search sessions, tabs, or actions" />
        <kbd>esc</kbd>
      </div>

      <!-- A listbox may only own options: delete is pointer-only here (sessions
           can also be deleted from the session's own menu), and the empty
           state lives outside it. -->
      <div class="list" role="listbox" aria-label="Results" hidden={!items.length}>
        {#each items as item, i (item.id)}
          {#if i === 0 || items[i - 1].group !== item.group}
            <div class="group">{item.group}</div>
          {/if}
          <div class="row" class:on={i === index}>
            <button
              class="pick"
              role="option"
              aria-selected={i === index}
              on:click={() => choose(item)}
              on:mouseenter={() => (index = i)}
            >
              <span class="ico">{#if item.provider && hasMark(item.provider)}<ProviderLogo provider={item.provider} size={16} />{:else}<Icon name={item.icon} />{/if}</span>
              <span class="info">
                <span class="title">{item.title}</span>
                {#if item.sub}<span class="sub">{item.sub}</span>{/if}
              </span>
            </button>
            {#if item.remove}
              <button class="del" tabindex="-1" aria-hidden="true" use:tooltip={"Delete session"} on:click={item.remove}>
                <Icon name="trash" size={12} />
              </button>
            {/if}
          </div>
        {/each}
      </div>
      {#if !items.length}
        <div class="empty">Nothing matches</div>
      {/if}

      <div class="foot">
        <span><kbd><Icon name="arrowUp" size={12} /></kbd><kbd><Icon name="arrowDown" size={12} /></kbd> move</span>
        <span><kbd>enter</kbd> open</span>
        <span><kbd>esc</kbd> close</span>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 1000;
    display: flex;
    justify-content: center;
    padding-top: 14vh;
    background: var(--scrim);
  }
  .box {
    width: min(580px, 92vw);
    max-height: 480px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
  }
  .head {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 3);
    padding: calc(var(--space) * 3) calc(var(--space) * 4);
    color: var(--muted);
    border-bottom: 1px solid var(--line);
  }
  .head input {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    box-shadow: none;
    color: var(--text);
    font-size: var(--fs-base);
  }
  .head input::placeholder {
    color: var(--muted);
  }
  kbd {
    display: inline-flex;
    align-items: center;
    height: 17px;
    vertical-align: middle;
    padding: 0 var(--space);
    margin-right: var(--space);
    font-size: var(--fs-xs);
    color: var(--muted);
    background: var(--line);
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
  }
  .list {
    flex: 1;
    overflow-y: auto;
    padding: calc(var(--space) * 2);
  }
  .group {
    padding: calc(var(--space) * 2) calc(var(--space) * 3) var(--space);
    font-size: var(--fs-xs);
    font-weight: 600;
    color: var(--muted);
  }
  .row {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    border-radius: var(--radius);
  }
  .row.on {
    background: var(--hover);
  }
  .pick {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 3);
    padding: calc(var(--space) * 2) calc(var(--space) * 3);
    background: transparent;
    border: none;
    color: inherit;
    text-align: left;
    cursor: pointer;
  }
  .ico {
    width: 22px;
    height: 22px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius);
    background: var(--line);
    color: var(--muted);
  }
  .info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space);
  }
  .title,
  .sub {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .title {
    font-size: var(--fs-md);
    font-weight: 500;
    color: var(--text);
  }
  .sub {
    font-size: var(--fs-xs);
    color: var(--muted);
  }
  .del {
    width: 26px;
    height: 26px;
    margin-right: calc(var(--space) * 2);
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--muted);
    cursor: pointer;
    opacity: 0;
  }
  .row:hover .del,
  .row.on .del {
    opacity: 1;
  }
  .del:hover {
    color: var(--bad);
    background: transparent;
  }
  .empty {
    padding: calc(var(--space) * 8) calc(var(--space) * 4);
    text-align: center;
    font-size: var(--fs-md);
    color: var(--muted);
  }
  .foot {
    display: flex;
    gap: calc(var(--space) * 4);
    padding: calc(var(--space) * 2) calc(var(--space) * 4);
    font-size: var(--fs-xs);
    color: var(--muted);
    border-top: 1px solid var(--line);
  }
</style>
