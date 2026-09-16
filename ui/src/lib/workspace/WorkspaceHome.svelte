<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import type { Project } from "../api";

  export let workspace = "";
  export let projects: Project[] = [];

  const dispatch = createEventDispatcher<{
    open: { slug: string };
    create: void;
  }>();

  const statusOf = (p: Project) => String(p.status).toLowerCase();
</script>

<div class="home">
  <div class="head">
    <h1>{workspace}</h1>
    <button class="new" on:click={() => dispatch("create")}>New project</button>
  </div>

  {#if !projects.length}
    <div class="empty">
      <span class="e-title">No project yet</span>
      <button class="new ghost" on:click={() => dispatch("create")}>New project</button>
    </div>
  {:else}
    <div class="rows">
      {#each projects as p (p.slug)}
        <button class="row" on:click={() => dispatch("open", { slug: p.slug })}>
          <span class="dot s-{statusOf(p)}" />
          <span class="title">{p.title || p.slug}</span>
          <span class="st">{statusOf(p)}</span>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .home { display: flex; flex-direction: column; gap: 18px; padding: 28px 28px 40px; min-width: 0; }
  .head { display: flex; align-items: center; gap: 12px; }
  h1 {
    margin: 0; flex: 1; min-width: 0;
    font-size: 22px; font-weight: 600; letter-spacing: -0.3px; color: var(--text);
  }
  .new {
    flex: none; background: var(--accent); border: none; border-radius: var(--radius-2);
    color: var(--accent-ink); font: inherit; font-size: 12.5px; font-weight: 600;
    padding: 7px 14px; cursor: pointer;
  }
  .new:hover { filter: brightness(1.08); }
  .new.ghost {
    background: var(--surface-2); color: var(--text); border: 1px solid var(--line-2);
  }
  .empty {
    display: flex; flex-direction: column; align-items: flex-start; gap: 10px;
    padding: 12px 4px; color: var(--text-3);
  }
  .e-title { font-size: 13px; font-weight: 600; color: var(--text-2); }
  .rows { display: flex; flex-direction: column; gap: 6px; max-width: 560px; }
  .row {
    display: flex; align-items: center; gap: 10px; text-align: left;
    background: var(--surface-1); border: 1px solid var(--line-2); border-radius: var(--radius-3);
    color: var(--text-2); font: inherit; padding: 12px 14px; cursor: pointer;
  }
  .row:hover { background: var(--surface-2); color: var(--text); }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--text-4); flex: none; }
  .dot.s-running { background: var(--ok); }
  .dot.s-drafting { background: var(--warn); }
  .dot.s-planned { background: var(--info); }
  .title { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 550; }
  .st { flex: none; font-size: 11px; color: var(--text-4); }
</style>
