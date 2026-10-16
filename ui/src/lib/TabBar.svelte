<script lang="ts">
  import { createEventDispatcher, tick } from "svelte";
  import Icon from "./Icon.svelte";
  import type { Tab } from "./tabs";
  import { faviconUrl } from "./browserData";
  import { buttonHeld, tabDragStarted, tabRelease } from "./gestures";
  import { laneIcon, normLane } from "./lanes";
  import { tooltip } from "./tooltip";

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

  let strip: HTMLElement;
  const els: Record<string, HTMLElement> = {};
  let broken = new Set<string>();
  let press: { id: string; x: number; y: number; grab: number; pointer: number } | null = null;
  let dragId = "";
  let dragX = 0;
  let order: string[] | null = null;
  // Set when a drag swallowed the click, so that click does not select again.
  let swallow = "";

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
      els[id]?.querySelector<HTMLElement>('[role="tab"]')?.focus();
    }
  }

  function onAux(e: MouseEvent, id: string) {
    if (e.button === 1) dispatch("close", { id });
  }

  function onPick(id: string) {
    if (swallow === id) {
      swallow = "";
      return;
    }
    dispatch("select", { id });
  }

  function onDown(e: PointerEvent, id: string) {
    swallow = "";
    if (e.button !== 0 || (e.target as HTMLElement).closest(".close")) return;
    const el = els[id];
    if (!el) return;
    const r = el.getBoundingClientRect();
    press = vertical
      ? { id, x: e.clientX, y: e.clientY, grab: e.clientY - r.top, pointer: e.pointerId }
      : { id, x: e.clientX, y: e.clientY, grab: e.clientX - r.left, pointer: e.pointerId };
  }

  // A press left over from a pointerup the page window ate. Cleared before
  // the tab's own handler runs, so a new press still starts.
  function onDownAnywhere() {
    if (press) onCancel();
  }

  function onMove(e: PointerEvent) {
    if (!press || e.pointerId !== press.pointer) return;
    if (!buttonHeld(e.buttons)) {
      onUp(e);
      return;
    }
    if (!dragId) {
      if (!tabDragStarted(press.x, press.y, e.clientX, e.clientY, vertical)) return;
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
    if (!press || !dragId) return;
    dragX = pos - start(dragId);
  }

  function release(el: HTMLElement | undefined, pointer: number | undefined) {
    if (!el || pointer === undefined) return;
    try {
      el.releasePointerCapture(pointer);
    } catch {}
  }

  function onUp(e?: PointerEvent) {
    if (press && e && e.pointerId !== press.pointer) return;
    const result = tabRelease({
      pressId: press?.id ?? "",
      dragId,
      order,
      ids: tabs.map((t) => t.id),
    });
    const el = dragId ? els[dragId] : undefined;
    const pointer = press?.pointer;
    press = null;
    dragId = "";
    dragX = 0;
    order = null;
    release(el, pointer);
    if (result.kind === "move") {
      swallow = result.id;
      dispatch("move", { id: result.id, to: result.to });
      dispatch("select", { id: result.id });
    } else if (result.kind === "select") {
      swallow = result.id;
      dispatch("select", { id: result.id });
    }
  }

  function onCancel() {
    const el = dragId ? els[dragId] : undefined;
    const pointer = press?.pointer;
    press = null;
    dragId = "";
    dragX = 0;
    order = null;
    swallow = "";
    release(el, pointer);
  }

  function onLost(e: PointerEvent) {
    if (!press || e.pointerId !== press.pointer) return;
    onCancel();
  }
</script>

<svelte:window
  on:pointerdown|capture={onDownAnywhere}
  on:pointermove={onMove}
  on:pointerup={onUp}
  on:pointercancel={onCancel}
  on:lostpointercapture={onLost}
  on:blur={onCancel}
/>

<!-- A tablist may only own tabs, so each tab is a plain pill holding the
     role="tab" element and, beside it, pointer-only caret and close
     buttons (hidden from assistive tech; Ctrl+W closes the active tab). -->
<div class="tabs" class:vertical role="tablist" bind:this={strip}>
  {#each groups.lone as tab (tab.id)}
    {@const lane = laneOf(tab)}
    {@const attached = tab.kind === "session" && tab.sessionId ? (groups.kids.get(tab.sessionId) ?? []) : []}
    {@const shut = tab.sessionId ? collapsed.has(tab.sessionId) : false}
    {@const linked = `${attached.length} linked page${attached.length === 1 ? "" : "s"}`}
    {#if attached.length && !shut}
      <div class="group">
        <div
          bind:this={els[tab.id]}
          class="tab"
          class:active={tab.id === activeTabId}
          class:dragging={tab.id === dragId}
          style={tab.id === dragId ? `transform: ${vertical ? "translateY" : "translateX"}(${dragX}px)` : ""}
          on:pointerdown={(e) => onDown(e, tab.id)}
          on:auxclick={(e) => onAux(e, tab.id)}
        >
          <div
            class="pick"
            role="tab"
            tabindex="0"
            aria-selected={tab.id === activeTabId}
            title={tab.title}
            on:click={() => onPick(tab.id)}
            on:keydown={(e) => onKey(e, tab.id)}
          >
            <span class="kind">
              {#if tab.loading}
                <span class="spinner" />
              {:else}
                <Icon name={laneIcon(lane)} />
              {/if}
            </span>
            <span class="title">{tab.title || "New session"}</span>
          </div>
          <button
            class="close group-caret"
            tabindex="-1"
            aria-hidden="true"
            use:tooltip={`Hide ${linked}`}
            on:click|stopPropagation={() => tab.sessionId && toggleGroup(tab.sessionId)}
          >
            <Icon name="chevDown" size={12} />
            <span class="n">{attached.length}</span>
          </button>
          {#if tabs.length > 1}
            <button
              class="close"
              tabindex="-1"
              aria-hidden="true"
              use:tooltip={"Close tab"}
              on:click|stopPropagation={() => dispatch("close", { id: tab.id })}
            >
              <Icon name="close" size={12} />
            </button>
          {/if}
        </div>
        {#each attached as sub (sub.id)}
          <div class="tab sub" class:active={sub.id === activeTabId} on:auxclick={(e) => onAux(e, sub.id)}>
            <div
              class="pick"
              role="tab"
              tabindex="0"
              aria-selected={sub.id === activeTabId}
              title={sub.url || sub.title}
              on:click={() => onPick(sub.id)}
              on:keydown={(e) => onKey(e, sub.id)}
            >
              <span class="kind">
                {#if sub.loading}
                  <span class="spinner" />
                {:else if sub.url && !broken.has(sub.url)}
                  <img src={sub.favicon || faviconUrl(sub.url)} alt="" on:error={() => sub.url && (broken = new Set(broken).add(sub.url))} />
                {:else}
                  <Icon name="globe" />
                {/if}
              </span>
              <span class="title">{sub.title || "New page"}</span>
            </div>
            <button
              class="close"
              tabindex="-1"
              aria-hidden="true"
              use:tooltip={"Close tab"}
              on:click|stopPropagation={() => dispatch("close", { id: sub.id })}
            >
              <Icon name="close" size={12} />
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
      style={tab.id === dragId ? `transform: ${vertical ? "translateY" : "translateX"}(${dragX}px)` : ""}
      on:pointerdown={(e) => onDown(e, tab.id)}
      on:auxclick={(e) => onAux(e, tab.id)}
    >
      <div
        class="pick"
        role="tab"
        tabindex="0"
        aria-selected={tab.id === activeTabId}
        title={tab.kind === "page" ? tab.url || tab.title : tab.title}
        on:click={() => onPick(tab.id)}
        on:keydown={(e) => onKey(e, tab.id)}
      >
        <span class="kind">
          {#if tab.loading}
            <span class="spinner" />
          {:else if tab.kind === "page" && tab.url && !broken.has(tab.url)}
            <img src={tab.favicon || faviconUrl(tab.url)} alt="" on:error={() => tab.url && (broken = new Set(broken).add(tab.url))} />
          {:else}
            <Icon name={tab.kind === "page" ? "globe" : tab.kind === "brain" ? "brain" : tab.kind === "history" ? "clock" : tab.kind === "settings" ? "settings" : tab.kind === "preview" ? "spark" : tab.sessionId?.startsWith("r:") || tab.sessionId?.startsWith("m:") ? "server" : laneIcon(lane)} />
          {/if}
        </span>
        <span class="title">{tab.title || (tab.kind === "page" ? "New page" : "New session")}</span>
      </div>
      {#if attached.length}
        <button
          class="close group-caret shut"
          tabindex="-1"
          aria-hidden="true"
          use:tooltip={`Show ${linked}`}
          on:click|stopPropagation={() => tab.sessionId && toggleGroup(tab.sessionId)}
        >
          <Icon name="chevDown" size={12} />
          <span class="n">{attached.length}</span>
        </button>
      {/if}
      {#if tabs.length > 1}
        <button
          class="close"
          tabindex="-1"
          aria-hidden="true"
          use:tooltip={"Close tab"}
          on:click|stopPropagation={() => dispatch("close", { id: tab.id })}
        >
          <Icon name="close" size={12} />
        </button>
      {/if}
    </div>
    {/if}
  {/each}
</div>
{#if !vertical}
  <button class="new" use:tooltip={"New tab (Ctrl+T)"} on:click={() => dispatch("newTab")}>
    <Icon name="plus" />
  </button>
{/if}

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
  /* Equal widths keep the strip's rhythm even; the fill is the only state. */
  .tab {
    display: inline-flex;
    align-items: center;
    height: 28px;
    width: 220px;
    flex: 0 1 auto;
    min-width: 96px;
    padding: 0 4px 0 0;
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--fs-sm);
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
    box-shadow: 0 6px 18px rgba(0, 0, 0, 0.4);
    cursor: grabbing;
    transition: background 140ms ease, color 140ms ease;
  }
  .tab:hover {
    background: var(--hover);
    color: var(--text);
  }
  /* Glass tinted with the theme's panel, not a white veil: the title stays
     readable when a light theme's dark text sits over a dimmed picture. */
  .tab.active {
    background: color-mix(in srgb, var(--panel) 82%, transparent);
    -webkit-backdrop-filter: var(--glass-blur);
    backdrop-filter: var(--glass-blur);
    color: var(--text);
  }
  /* The tab proper: fills the pill up to its buttons. Focus rings the pill. */
  .pick {
    flex: 1;
    align-self: stretch;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 6px 0 10px;
    border-radius: inherit;
  }
  .pick:focus-visible {
    outline: none;
  }
  .tab:has(> .pick:focus-visible) {
    outline: 2px solid color-mix(in srgb, var(--accent) 70%, transparent);
    outline-offset: 2px;
  }
  .kind {
    display: inline-flex;
    flex: none;
    color: var(--muted);
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
    width: 160px;
    min-width: 72px;
    font-size: var(--fs-xs);
    opacity: 0.85;
  }
  .group {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 2px;
    border-radius: var(--radius-lg);
    background: var(--hover);
  }
  .close.group-caret {
    opacity: 1;
    gap: 1px;
    width: auto;
    min-width: 24px;
    padding: 4px 2px;
  }
  .group-caret :global(svg) {
    transition: transform 140ms ease;
  }
  .group-caret.shut :global(svg) {
    transform: rotate(-90deg);
  }
  .group-caret .n {
    font-size: var(--fs-xs);
    font-variant-numeric: tabular-nums;
  }
  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* 24px hit area; the padding is clipped so the hover chip stays 16px. */
  .close {
    width: 24px;
    height: 24px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 4px;
    background: transparent;
    background-clip: content-box;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
    opacity: 0;
  }
  .close + .close {
    margin-left: 8px;
  }
  .tab:hover .close,
  .tab.active .close {
    opacity: 1;
  }
  /* An idle tab lends its close button's room to the title. */
  .tab:not(:hover):not(.active) .close:not(.group-caret) {
    display: none;
  }
  .close:hover {
    background: var(--selected);
    color: var(--text);
  }
  .new {
    width: 28px;
    height: 28px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .new:hover {
    background: var(--hover);
    color: var(--text);
  }
</style>
