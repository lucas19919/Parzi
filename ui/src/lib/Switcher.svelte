<script lang="ts">
  import { createEventDispatcher, tick } from "svelte";
  import { fade, fly } from "svelte/transition";
  import Icon from "./Icon.svelte";
  import type { IconName } from "./icons";
  import type { SessionMeta } from "./api";
  import { folderName, type Tab } from "./tabs";
  import { setOverlay } from "./overlay";

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
    title: string;
    sub: string;
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

  function tokens(t: SessionMeta) {
    const n = (t.tokens_in || 0) + (t.tokens_out || 0);
    return n >= 1000 ? `${Math.round(n / 1000)}k tokens` : `${n} tokens`;
  }

  $: q = query.trim().toLowerCase();
  $: openSessions = new Set(tabs.map((t) => t.sessionId).filter(Boolean));
  $: items = [
    ...[
      { id: "new-session", icon: "chat", title: "New session", sub: "Start a draft", run: () => dispatch("newSession") },
      { id: "new-page", icon: "globe", title: "New page", sub: "Open a web page", run: () => dispatch("newPage") },
      { id: "history", icon: "clock", title: "History", sub: "Pages you visited and bookmarks", run: () => dispatch("history") },
      { id: "brain", icon: "brain", title: "Brain", sub: "Notes and projects", run: () => dispatch("brain") },
      { id: "previews", icon: "spark", title: "Component previews", sub: "Live Omnibar variants, side by side", run: () => dispatch("previews") },
      { id: "settings", icon: "settings", title: "Settings", sub: "Agents, appearance, system", run: () => dispatch("settings") },
      { id: "setup", icon: "spark", title: "Set up Parzi", sub: "Agents, browser, tools, brain, remote", run: () => dispatch("setup") },
    ]
      .filter((c) => hit(q, c.title, c.sub))
      .map((c) => ({ ...c, group: "Actions" }) as Item),
    ...tabs
      .filter((t) => hit(q, t.title, t.url))
      .map(
        (t): Item => ({
          id: `tab:${t.id}`,
          group: "Tabs",
          icon: t.kind === "page" ? "globe" : "chat",
          title: t.title || "New session",
          sub: t.id === activeTabId ? "Current tab" : t.url || "Open tab",
          run: () => dispatch("selectTab", { id: t.id }),
        }),
      ),
    ...threads
      .filter((t) => !openSessions.has(t.id) && hit(q, t.title, t.model, t.cwd))
      .map(
        (t): Item => ({
          id: `session:${t.id}`,
          group: "Sessions",
          icon: "chat",
          title: t.title || "Untitled session",
          sub: [t.model && t.model !== "auto" ? t.model : "no model yet", t.cwd ? folderName(t.cwd) : "", tokens(t)].filter(Boolean).join(" · "),
          run: () => dispatch("openSession", { id: t.id }),
          remove: () => dispatch("deleteSession", { id: t.id }),
        }),
      ),
  ];
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
    <div class="box" role="dialog" aria-modal="true" aria-label="Search" transition:fly={{ y: -10, duration: 160 }}>
      <div class="head">
        <Icon name="search" />
        <input bind:this={inputEl} bind:value={query} placeholder="Search sessions, tabs, or actions" />
        <kbd>esc</kbd>
      </div>

      <div class="list" role="listbox">
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
              <span class="ico"><Icon name={item.icon} size={13} /></span>
              <span class="info">
                <span class="title">{item.title}</span>
                <span class="sub">{item.sub}</span>
              </span>
            </button>
            {#if item.remove}
              <button class="del" title="Delete session" on:click={item.remove}>
                <Icon name="trash" size={12} />
              </button>
            {/if}
          </div>
        {:else}
          <div class="empty">Nothing matches</div>
        {/each}
      </div>

      <div class="foot">
        <span><kbd>↑</kbd><kbd>↓</kbd> move</span>
        <span><kbd>↵</kbd> open</span>
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
    background: rgba(0, 0, 0, 0.6);
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
    box-shadow: var(--shadow);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 16px;
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
    font-size: 14px;
  }
  .head input::placeholder {
    color: var(--faint);
  }
  kbd {
    padding: 1px 5px;
    margin-right: 3px;
    font-size: 10px;
    color: var(--muted);
    background: var(--line);
    border: 1px solid var(--line);
    border-radius: 4px;
  }
  .list {
    flex: 1;
    overflow-y: auto;
    padding: 6px;
  }
  .group {
    padding: 8px 10px 4px;
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--faint);
  }
  .row {
    display: flex;
    align-items: center;
    border-radius: 7px;
  }
  .row.on {
    background: var(--line);
  }
  .pick {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
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
    border-radius: 6px;
    background: var(--line);
    color: var(--muted);
  }
  .info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .title,
  .sub {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .title {
    font-size: 12.5px;
    font-weight: 500;
    color: var(--text);
  }
  .sub {
    font-size: 11px;
    color: var(--muted);
  }
  .del {
    width: 26px;
    height: 26px;
    margin-right: 6px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    border-radius: 5px;
    color: var(--faint);
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
    padding: 32px 16px;
    text-align: center;
    font-size: 13px;
    color: var(--muted);
  }
  .foot {
    display: flex;
    gap: 14px;
    padding: 8px 16px;
    font-size: 10.5px;
    color: var(--muted);
    border-top: 1px solid var(--line);
  }
</style>
