<script lang="ts">
  import { createEventDispatcher, tick } from "svelte";
  import { flip } from "svelte/animate";
  import Icon from "./Icon.svelte";
  import type { Tab } from "./tabs";
  import { faviconUrl } from "./browserData";

  export let tabs: Tab[] = [];
  export let activeTabId = "";

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
  let press: { id: string; x: number; grab: number; pointer: number } | null = null;
  let dragId = "";
  let dragX = 0;
  let order: string[] | null = null;

  $: shown = order ? order.map((id) => tabs.find((t) => t.id === id)).filter((t): t is Tab => !!t) : tabs;

  function shift(node: Element, rects: { from: DOMRect; to: DOMRect }, params: { skip: boolean }) {
    return params.skip ? { duration: 0 } : flip(node, rects, { duration: 160 });
  }

  async function onKey(e: KeyboardEvent, id: string) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      dispatch("select", { id });
    } else if (e.ctrlKey && e.shiftKey && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
      e.preventDefault();
      const at = tabs.findIndex((t) => t.id === id);
      dispatch("move", { id, to: Math.max(0, at + (e.key === "ArrowLeft" ? -1 : 1)) });
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
    press = { id, x: e.clientX, grab: e.clientX - el.getBoundingClientRect().left, pointer: e.pointerId };
  }

  function onMove(e: PointerEvent) {
    if (!press) return;
    if (!dragId) {
      if (Math.abs(e.clientX - press.x) < DRAG_START) return;
      dragId = press.id;
      order = tabs.map((t) => t.id);
      try {
        els[dragId]?.setPointerCapture(press.pointer);
      } catch {}
    }
    void follow(e.clientX);
  }

  async function follow(clientX: number) {
    const el = els[dragId];
    if (!el || !order || !press) return;
    const box = strip.getBoundingClientRect();
    const end = Math.max(...order.map((id) => (els[id] ? els[id].offsetLeft + els[id].offsetWidth : 0)));
    const left = Math.max(0, Math.min(clientX - box.left + strip.scrollLeft - press.grab, end - el.offsetWidth));
    const right = left + el.offsetWidth;
    const mine = order.indexOf(dragId);
    const before = order.filter((id, i) => {
      if (id === dragId || !els[id]) return false;
      const center = els[id].offsetLeft + els[id].offsetWidth / 2;
      return i < mine ? left >= center : right > center;
    });
    const next = [...before, dragId, ...order.filter((id) => id !== dragId && !before.includes(id))];
    if (next.join() !== order.join()) {
      order = next;
      await tick();
    }
    dragX = left - el.offsetLeft;
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

<div class="tabs" role="tablist" bind:this={strip}>
  {#each shown as tab (tab.id)}
    <div
      bind:this={els[tab.id]}
      animate:shift={{ skip: tab.id === dragId }}
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
          <Icon name={tab.kind === "page" ? "globe" : tab.kind === "brain" ? "brain" : tab.kind === "history" ? "clock" : "chat"} size={12} />
        {/if}
      </span>
      <span class="title">{tab.title || (tab.kind === "page" ? "New page" : "New session")}</span>
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
