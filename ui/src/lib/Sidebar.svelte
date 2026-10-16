<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import Icon from "./Icon.svelte";
  import TabBar from "./TabBar.svelte";
  import { isSystemTab, type Tab } from "./tabs";

  export let tabs: Tab[] = [];
  export let activeTabId = "";
  export let sessionLanes: Record<string, string> = {};
  export let contextUsed = 0;
  export let contextLimit = 0;
  export let compacting = false;

  const dispatch = createEventDispatcher<{
    select: { id: string };
    close: { id: string };
    move: { id: string; to: number };
    context: void;
  }>();

  $: workTabs = tabs.filter((t) => !isSystemTab(t));
  $: systemTabs = tabs.filter((t) => isSystemTab(t));

  const RING = 2 * Math.PI * 9;
  $: contextPct = contextLimit > 0 ? Math.min(100, Math.round((contextUsed / contextLimit) * 100)) : 0;

  // Adjustable rail, remembered per device.
  const W_KEY = "parzi.sidebar.w";
  const W_MIN = 160;
  const W_MAX = 340;
  let width = 204;
  try {
    const saved = Number(localStorage.getItem(W_KEY));
    if (Number.isFinite(saved)) width = Math.max(W_MIN, Math.min(W_MAX, saved));
  } catch {}
  let aside: HTMLElement | null = null;
  let sizing = false;

  function sizeTo(clientX: number) {
    const box = aside?.getBoundingClientRect();
    if (!box) return;
    width = Math.max(W_MIN, Math.min(W_MAX, Math.round(clientX - box.left)));
  }

  let pointer = -1;

  function onGrip(e: PointerEvent) {
    sizing = true;
    pointer = e.pointerId;
    sizeTo(e.clientX);
    e.preventDefault();
    e.stopPropagation();
  }

  // No pointer capture: a capture the page window steals never comes back,
  // and then no click in the app lands. The window hears the move instead.
  function onMove(e: PointerEvent) {
    if (!sizing || e.pointerId !== pointer) return;
    if ((e.buttons & 1) === 0) {
      endSize();
      return;
    }
    sizeTo(e.clientX);
  }

  function endSize(e?: Event) {
    if (!sizing) return;
    if (e && e.type !== "blur" && e instanceof PointerEvent && e.pointerId !== pointer) return;
    sizing = false;
    try {
      localStorage.setItem(W_KEY, String(width));
    } catch {}
  }

  function onDownAnywhere() {
    if (sizing) endSize();
  }

  // Each TabBar reorders its own list; translate back to the global index.
  function moveWork(id: string, toLocal: number) {
    const globals = tabs.flatMap((t, i) => (isSystemTab(t) ? [] : [i]));
    dispatch("move", { id, to: globals[Math.max(0, Math.min(toLocal, globals.length - 1))] ?? 0 });
  }

  function moveSystem(id: string, toLocal: number) {
    const globals = tabs.flatMap((t, i) => (isSystemTab(t) ? [i] : []));
    dispatch("move", { id, to: globals[Math.max(0, Math.min(toLocal, globals.length - 1))] ?? 0 });
  }
</script>

<aside class="sidebar" bind:this={aside} style:width="{width}px" class:sizing>
  <div class="logo-row">
    {#if contextLimit > 0}
      <button
        class="logo-ring"
        class:warn={contextPct >= 70}
        class:bad={contextPct >= 90}
        title={`Context ${contextPct}% full. Open the session for details.`}
        on:click={() => dispatch("context")}
      >
        <svg width="22" height="22" viewBox="0 0 22 22" aria-hidden="true">
          <circle class="track" cx="11" cy="11" r="9" />
          <circle
            class="fill"
            cx="11"
            cy="11"
            r="9"
            stroke-dasharray={RING}
            stroke-dashoffset={compacting ? RING * 0.7 : RING * (1 - contextPct / 100)}
          />
        </svg>
        <span class="logo-pct">{contextPct}</span>
      </button>
    {:else}
      <span class="logo-idle" title="Parzi"><i /></span>
    {/if}
  </div>

  <div class="side-tabs">
    <TabBar
      tabs={workTabs}
      {activeTabId}
      {sessionLanes}
      vertical
      on:select={(e) => dispatch("select", e.detail)}
      on:close={(e) => dispatch("close", e.detail)}
      on:move={(e) => moveWork(e.detail.id, e.detail.to)}
    />
  </div>

  {#if systemTabs.length}
    <div class="side-foot">
      <TabBar
        tabs={systemTabs}
        {activeTabId}
        {sessionLanes}
        vertical
        on:select={(e) => dispatch("select", e.detail)}
        on:close={(e) => dispatch("close", e.detail)}
        on:move={(e) => moveSystem(e.detail.id, e.detail.to)}
      />
    </div>
  {/if}
  <!-- svelte-ignore a11y-no-static-element-interactions -->
  <div class="grip" title="Drag to resize the sidebar" on:pointerdown={onGrip} />
</aside>

<svelte:window
  on:pointerdown|capture={onDownAnywhere}
  on:pointermove={onMove}
  on:pointerup={endSize}
  on:pointercancel={endSize}
  on:blur={endSize}
/>

<style>
  /* One glass column from the window's top edge down: the top bar's menu,
     Home and New tab buttons sit on it (the top bar is drawn above). The
     negative margin lifts it under the 38px top bar; stretch keeps it full height. */
  .sidebar {
    position: relative;
    flex: none;
    margin-top: -38px;
    padding-top: 38px;
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--line);
    background: var(--glass-bg);
    -webkit-backdrop-filter: var(--glass-blur);
    backdrop-filter: var(--glass-blur);
    user-select: none;
  }
  /* The context ring lives here as the sidebar's logo: always visible,
     even on Home where no session header shows it. */
  .logo-row {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 10px 8px 2px;
  }
  .logo-ring {
    position: relative;
    width: 34px;
    height: 34px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: 50%;
    cursor: pointer;
    --ctx: var(--faint);
  }
  .logo-ring:hover {
    background: var(--panel);
  }
  .logo-ring svg {
    transform: rotate(-90deg);
  }
  .logo-ring .track {
    fill: none;
    stroke: var(--line);
    stroke-width: 2.5;
  }
  .logo-ring .fill {
    fill: none;
    stroke: var(--ctx);
    stroke-width: 2.5;
    stroke-linecap: round;
  }
  .logo-ring.warn {
    --ctx: var(--warn);
  }
  .logo-ring.bad {
    --ctx: var(--bad);
  }
  .logo-pct {
    position: absolute;
    font-size: 8.5px;
    font-weight: 700;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .logo-idle {
    width: 34px;
    height: 34px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .logo-idle i {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    border: 2.5px solid var(--faint);
  }
  .grip {
    position: absolute;
    top: 0;
    bottom: 0;
    right: -4px;
    width: 8px;
    cursor: ew-resize;
    touch-action: none;
    z-index: 5;
  }
  .grip:hover::after,
  .sidebar.sizing .grip::after {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    left: 3px;
    width: 2px;
    background: var(--accent);
    opacity: 0.7;
  }
  .side-tabs {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 8px 8px 4px;
  }
  /* Settings, Brain and History stay at the bottom, set apart by the space alone. */
  .side-foot {
    flex: none;
    max-height: 40%;
    overflow-y: auto;
    padding: 4px 8px 8px;
  }
</style>
