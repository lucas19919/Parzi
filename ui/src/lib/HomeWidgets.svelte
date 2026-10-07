<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { isUsable } from "./providerRows";
  import type { ProviderStatus, SessionMeta } from "./api";

  export let threads: SessionMeta[] = [];
  export let running: Set<string> = new Set();
  export let board: ProviderStatus[] = [];

  const dispatch = createEventDispatcher<{ allSessions: void }>();

  $: live = threads.filter((t) => running.has(t.id) || t.status === "active").length;
  $: ready = board.filter((b) => isUsable(b)).length;
</script>

<div class="strip" aria-label="Status">
  <div class="stat" class:live={live > 0}>
    <span class="dot" class:on={live > 0} />
    <span class="num">{live}</span>
    <span class="lbl">running</span>
  </div>
  <button class="stat" title="All sessions" on:click={() => dispatch("allSessions")}>
    <span class="num">{threads.length}</span>
    <span class="lbl">sessions</span>
  </button>
  <div class="stat">
    <span class="num">{ready}</span>
    <span class="lbl">agents ready</span>
  </div>
</div>

<style>
  .strip {
    display: flex;
    gap: 8px;
    width: 100%;
  }
  .stat {
    flex: 1;
    display: flex;
    align-items: baseline;
    justify-content: center;
    gap: 7px;
    padding: 10px 8px;
    background: color-mix(in srgb, var(--panel) 55%, transparent);
    -webkit-backdrop-filter: var(--glass-strong-blur);
    backdrop-filter: var(--glass-strong-blur);
    border: 1px solid color-mix(in srgb, var(--line) 65%, transparent);
    border-radius: var(--radius-lg);
    color: var(--muted);
    font: inherit;
  }
  button.stat {
    cursor: pointer;
  }
  button.stat:hover {
    color: var(--text);
  }
  .dot {
    width: 7px;
    height: 7px;
    align-self: center;
    border-radius: 50%;
    background: var(--faint);
    flex: none;
  }
  .dot.on {
    background: var(--ok);
  }
  .num {
    font-size: 17px;
    font-weight: 650;
    letter-spacing: -0.02em;
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }
  .lbl {
    font-size: 11.5px;
  }
  .stat.live .num {
    color: var(--ok);
  }
</style>
