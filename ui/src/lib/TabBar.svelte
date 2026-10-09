<script lang="ts">
  import { createEventDispatcher, tick } from "svelte";
  import Icon from "./Icon.svelte";
  import type { Tab } from "./tabs";
  import { faviconUrl } from "./browserData";
  import { laneIcon, normLane } from "./lanes";

  export let tabs: Tab[] = [];
  export let activeTabId = "";
  export let sessionLanes: Record<string, string> = {};
  export let vertical = false;

  const dispatch = createEventDispatcher<{
    select: { id: string };
    close: { id: string };
    move: { id: string; to: number };
    newTab: void;
  }>();

  const DRAG_START = 5;

  let strip: HTMLElement;
  const els: Record<string, HTMLElement> = {};
  let broken = new Set<string>();
  let press: { id: string; x: number; y: number; grab: number; pointer: number } | null = null;
  let dragId = "";
  let dragX = 0;
  let order: string[] | null = null;

  $: shown = order ? order.map((id) => tabs.find((t) => t.id === id)).filter((t): t is Tab => !!t) : tabs;

  let collapsed = new Set<string>();
  let touched = new Set<string>();

  // Groups start collapsed (agent tabs arrive quietly); explicit
  // user toggles win from then on. Guarded: only reassign when the set
  // actually changed, otherwise `collapsed = collapsed` self-invalidates
  // (objects are always dirty) and loops the reactive flush forever,
  // freezing the UI on the next tabs change / keystroke.
  $: {
    let changed = false;
    for (const [sid, kids] of groups.kids) {
      if (kids.length && !touched.has(sid) && !collapsed.has(sid)) {
        collapsed.add(sid);
        changed = true;
      }
    }
    if (changed) collapsed = new Set(collapsed);
  }

  function laneOf(t: Tab): string {
    if (t.kind !== "session" || !t.sessionId) return "";
    return normLane(sessionLanes[t.sessionId] ?? "");
  }

  // Page tabs opened by an agent attach under their session tab instead
  // of cluttering the strip. Sessions with attached pages get a collapse
  // caret; attached pages render smaller and indented.
  $: groups = (() => {
    const bySession = new Map<string, Tab>();
    for (const t of shown) if (t.kind === "session" && t.sessionId) bySession.set(t.sessionId, t);
    const kids = new Map<string, Tab[]>();
    const lone: Tab[] = [];
    for (const t of shown) {
      if (t.kind === "page" && t.owner && bySession.has(t.owner)) {
        const arr = kids.get(t.owner) ?? [];
        arr.push(t);
        kids.set(t.owner, arr);
      } else {
        lone.push(t);
      }
    }
    return { bySession, kids, lone };
  })();

  function toggleGroup(sessionId: string) {
    touched = new Set(touched).add(sessionId);
    collapsed = new Set(collapsed);
    if (collapsed.has(sessionId)) collapsed.delete(sessionId);
    else collapsed.add(sessionId);
  }

  async function onKey(e: KeyboardEvent, id: string) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      dispatch("select", { id });
    } else if (e.ctrlKey && e.shiftKey && (e.key === "ArrowLeft" || e.key === "ArrowRight" || e.key === "ArrowUp" || e.key === "ArrowDown")) {
      e.preventDefault();
      const at = tabs.findIndex((t) => t.id === id);
      const back = e.key === "ArrowLeft" || e.key === "ArrowUp";
      dispatch("move", { id, to: Math.max(0, at + (back ? -1 : 1)) });
      await tick();
      els[id]?.focus();
    }
  }

  function onAux(e: MouseEvent, id: string) {
    if (e.button === 1) dispatch("close", { id });
  }

  function onDown(e: PointerEvent, id: string) {
    if (e.button !== 0 || (e.target as HTMLElement).closest(".close")) return;
    const el = els[id];
    if (!el) return;
    const r = el.getBoundingClientRect();
    press = vertical
      ? { id, x: e.clientX, y: e.clientY, grab: e.clientY - r.top, pointer: e.pointerId }
      : { id, x: e.clientX, y: e.clientY, grab: e.clientX - r.left, pointer: e.pointerId };
  }

  function onMove(e: PointerEvent) {
    if (!press) return;
    if (!dragId) {
      const moved = vertical ? Math.abs(e.clientY - press.y) : Math.abs(e.clientX - press.x);
      if (moved < DRAG_START) return;
      dragId = press.id;
      order = tabs.map((t) => t.id);
      try {
        els[dragId]?.setPointerCapture(press.pointer);
      } catch {}
    }
    void follow(vertical ? e.clientY : e.clientX);
  }

  async function follow(client: number) {
    const el = els[dragId];
    if (!el || !order || !press) return;
    const box = strip.getBoundingClientRect();
    const size = (id: string) => (els[id] ? (vertical ? els[id].offsetHeight : els[id].offsetWidth) : 0);
    const start = (id: string) => (els[id] ? (vertical ? els[id].offsetTop : els[id].offsetLeft) : 0);
    const end = Math.max(...order.map((id) => (els[id] ? start(id) + size(id) : 0)));
    const scroll = vertical ? strip.scrollTop : strip.scrollLeft;
    const origin = vertical ? box.top : box.left;
    const pos = Math.max(0, Math.min(client - origin + scroll - press.grab, end - size(dragId)));
    const far = pos + size(dragId);
    const mine = order.indexOf(dragId);
    const before = order.filter((id, i) => {
      if (id === dragId || !els[id]) return false;
      const center = start(id) + size(id) / 2;
      return i < mine ? pos >= center : far > center;
    });
    const next = [...before, dragId, ...order.filter((id) => id !== dragId && !before.includes(id))];
    if (next.join() !== order.join()) {
      order = next;
      await tick();
    }
    dragX = pos - start(dragId);
  }

  function onUp() {
    if (dragId && order) dispatch("move", { id: dragId, to: order.indexOf(dragId) });
    press = null;
    dragId = "";
    dragX = 0;
    order = null;
  }
</script>

<svelte:window on:pointermove={onMove} on:pointerup={onUp} on:pointercancel={onUp} />

<div class="tabs" class:vertical role="tablist" bind:this={strip}>
  {#each groups.lone as tab (tab.id)}
    {@const lane = laneOf(tab)}
    {@const attached = tab.kind === "session" && tab.sessionId ? (groups.kids.get(tab.sessionId) ?? []) : []}
    {@const shut = tab.sessionId ? collapsed.has(tab.sessionId) : false}
    {#if attached.length && !shut}
      <div class="group" role="group" aria-label="Linked tabs">
        <div
          bind:this={els[tab.id]}
          class="tab"
          class:active={tab.id === activeTabId}
          class:dragging={tab.id === dragId}
          style={tab.id === dragId ? `transform: ${vertical ? "translateY" : "translateX"}(${dragX}px)` : ""}
          role="tab"
          tabindex="0"
          aria-selected={tab.id === activeTabId}
          title={tab.title}
          on:pointerdown={(e) => onDown(e, tab.id)}
          on:click={() => dispatch("select", { id: tab.id })}
          on:keydown={(e) => onKey(e, tab.id)}
          on:auxclick={(e) => onAux(e, tab.id)}
        >
          <span class="kind">
            {#if tab.loading}
              <span class="spinner" />
            {:else}
              <Icon name={laneIcon(lane)} size={12} />
            {/if}
          </span>
          <span class="title">{tab.title || "New session"}</span>
          <button
            class="close group-caret"
            title={`Hide ${attached.length} linked page${attached.length === 1 ? "" : "s"}`}
            aria-label="Hide linked pages"
            on:click|stopPropagation={() => tab.sessionId && toggleGroup(tab.sessionId)}
          >
            <Icon name="chevDown" size={10} stroke={2.2} />
            <span class="n">{attached.length}</span>
          </button>
          {#if tabs.length > 1}
            <button
              class="close"
              title="Close tab"
              aria-label="Close tab"
              on:click|stopPropagation={() => dispatch("close", { id: tab.id })}
            >
              <Icon name="close" size={11} stroke={2.2} />
            </button>
          {/if}
        </div>
        {#each attached as sub (sub.id)}
          <div
            class="tab sub"
            class:active={sub.id === activeTabId}
            role="tab"
            tabindex="0"
            aria-selected={sub.id === activeTabId}
            title={sub.url || sub.title}
            on:click={() => dispatch("select", { id: sub.id })}
            on:keydown={(e) => onKey(e, sub.id)}
            on:auxclick={(e) => onAux(e, sub.id)}
          >
            <span class="kind">
              {#if sub.loading}
                <span class="spinner" />
              {:else if sub.url && !broken.has(sub.url)}
                <img src={faviconUrl(sub.url)} alt="" on:error={() => sub.url && (broken = new Set(broken).add(sub.url))} />
              {:else}
                <Icon name="globe" size={11} />
              {/if}
            </span>
            <span class="title">{sub.title || "New page"}</span>
            <button
              class="close"
              title="Close tab"
              aria-label="Close tab"
              on:click|stopPropagation={() => dispatch("close", { id: sub.id })}
            >
              <Icon name="close" size={10} stroke={2.2} />
            </button>
          </div>
        {/each}
      </div>
    {:else}
    <div
      bind:this={els[tab.id]}
      class="tab"
      class:active={tab.id === activeTabId}
      class:dragging={tab.id === dragId}
      style={tab.id === dragId ? `transform: translateX(${dragX}px)` : ""}
      role="tab"
      tabindex="0"
      aria-selected={tab.id === activeTabId}
      title={tab.kind === "page" ? tab.url || tab.title : tab.title}
      on:pointerdown={(e) => onDown(e, tab.id)}
      on:click={() => dispatch("select", { id: tab.id })}
      on:keydown={(e) => onKey(e, tab.id)}
      on:auxclick={(e) => onAux(e, tab.id)}
    >
      <span class="kind">
        {#if tab.loading}
          <span class="spinner" />
        {:else if tab.kind === "page" && tab.url && !broken.has(tab.url)}
          <img src={faviconUrl(tab.url)} alt="" on:error={() => tab.url && (broken = new Set(broken).add(tab.url))} />
        {:else}
          <Icon name={tab.kind === "page" ? "globe" : tab.kind === "brain" ? "brain" : tab.kind === "history" ? "clock" : tab.kind === "settings" ? "settings" : tab.kind === "preview" ? "spark" : laneIcon(lane)} size={12} />
        {/if}
      </span>
      <span class="title">{tab.title || (tab.kind === "page" ? "New page" : "New session")}</span>
      {#if attached.length}
        <button
          class="close group-caret shut"
          title={`Show ${attached.length} linked page${attached.length === 1 ? "" : "s"}`}
          aria-label="Show linked pages"
          on:click|stopPropagation={() => tab.sessionId && toggleGroup(tab.sessionId)}
        >
          <Icon name="chevDown" size={10} stroke={2.2} />
          <span class="n">{attached.length}</span>
        </button>
      {/if}
      {#if tabs.length > 1}
        <button
          class="close"
          title="Close tab"
          aria-label="Close tab"
          on:click|stopPropagation={() => dispatch("close", { id: tab.id })}
        >
          <Icon name="close" size={11} stroke={2.2} />
        </button>
      {/if}
    </div>
    {/if}
  {/each}
  <button class="new" title="New tab (Ctrl+T)" on:click={() => dispatch("newTab")}>
    <Icon name="plus" size={13} stroke={2.2} />
  </button>
</div>

<style>
  .tabs {
    position: relative;
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    overflow-x: auto;
  }
  .tabs.vertical {
    flex-direction: column;
    align-items: stretch;
    overflow-x: hidden;
    overflow-y: auto;
  }
  .tabs.vertical .tab {
    max-width: none;
    width: 100%;
    flex: none;
  }
  .tabs.vertical .group {
    width: 100%;
    flex-direction: column;
    align-items: stretch;
  }
  .tabs.vertical .tab.sub {
    max-width: none;
  }
  .tabs.vertical .new {
    display: none;
  }
  .tab {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    height: 29px;
    max-width: 220px;
    flex: 0 1 auto;
    min-width: 72px;
    padding: 0 6px 0 9px;
    border: 1px solid transparent;
    border-radius: var(--radius);
    color: var(--muted);
    font-size: 12px;
    font-weight: 500;
    white-space: nowrap;
    cursor: pointer;
    user-select: none;
    transition: background 140ms ease, color 140ms ease, transform 160ms ease;
  }
  .tab.dragging {
    position: relative;
    z-index: 2;
    background: var(--panel);
    border-color: var(--line);
    box-shadow: 0 6px 18px rgba(0, 0, 0, 0.4);
    cursor: grabbing;
    transition: background 140ms ease, color 140ms ease;
  }
  .tab:hover {
    background: color-mix(in srgb, var(--panel) 65%, transparent);
    color: var(--text);
  }
  .tab.active {
    background: var(--panel);
    border-color: var(--line);
    color: var(--text);
  }
  .kind {
    display: inline-flex;
    flex: none;
    color: var(--faint);
  }
  .kind img {
    width: 14px;
    height: 14px;
  }
  .spinner {
    width: 11px;
    height: 11px;
    border-radius: 50%;
    border: 1.6px solid var(--line);
    border-top-color: var(--accent);
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .tab.active .kind {
    color: var(--accent);
  }
  .tab.sub {
    height: 24px;
    max-width: 170px;
    min-width: 60px;
    font-size: 11px;
    opacity: 0.85;
  }
  .group {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 2px;
    border-radius: var(--radius-lg);
    background: color-mix(in srgb, var(--text) 6%, transparent);
    border: 1px solid color-mix(in srgb, var(--line) 65%, transparent);
  }
  .group .tab {
    border-color: transparent;
  }
  .group .tab.active {
    border-color: var(--line);
  }
  .group:has(.tab.active) {
    border-color: color-mix(in srgb, var(--accent) 38%, transparent);
  }
  .group-caret {
    opacity: 1;
    gap: 1px;
    width: auto;
    padding: 0 3px;
  }
  .group-caret :global(svg) {
    transition: transform 140ms ease;
  }
  .group-caret.shut :global(svg) {
    transform: rotate(-90deg);
  }
  .group-caret .n {
    font-size: 10px;
    font-variant-numeric: tabular-nums;
  }
  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .close {
    width: 16px;
    height: 16px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--faint);
    cursor: pointer;
    opacity: 0;
  }
  .tab:hover .close,
  .tab.active .close {
    opacity: 1;
  }
  .close:hover {
    background: var(--line);
    color: var(--text);
  }
  .new {
    width: 26px;
    height: 26px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .new:hover {
    background: var(--panel);
    border-color: var(--line);
    color: var(--text);
  }
</style>
