<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import Icon from "./Icon.svelte";

  export let activeSection = "general";
  export let authedCount = 0;

  const dispatch = createEventDispatcher<{
    select: { id: string };
    back: void;
  }>();

  const I = {
    search: "M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16zM21 21l-4.3-4.3",
    sliders: "M4 21v-7M4 10V3M12 21v-9M12 8V3M20 21v-5M20 12V3M1 14h6M9 8h6M17 16h6",
    eye: "M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7zM12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z",
    box: "M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z",
    gear: "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19 12a7 7 0 0 0-.1-1.2l2-1.6-2-3.4-2.4 1a7 7 0 0 0-2-1.2L14 3h-4l-.5 2.6a7 7 0 0 0-2 1.2l-2.4-1-2 3.4 2 1.6A7 7 0 0 0 5 12c0 .4 0 .8.1 1.2l-2 1.6 2 3.4 2.4-1a7 7 0 0 0 2 1.2L10 21h4l.5-2.6a7 7 0 0 0 2-1.2l2.4 1 2-3.4-2-1.6c.1-.4.1-.8.1-1.2z",
    back: "M19 12H5M12 19l-7-7 7-7",
    plug: "M9 7V3M15 7V3M7 7h10v5a5 5 0 0 1-10 0V7zM12 17v4",
    tools: "M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z",
    spark: "M12 3l1.9 5.6L19.5 10l-5.6 1.9L12 17.5l-1.9-5.6L4.5 10l5.6-1.4Z",
    layers: "M12 2l9 4.9-9 4.9-9-4.9L12 2zM3 12l9 4.9 9-4.9M3 17l9 4.9 9-4.9",
  };

  const SECTIONS = [
    { id: "general", label: "General", icon: I.sliders },
    { id: "appearance", label: "Appearance", icon: I.eye },
    { id: "models", label: "Models", icon: I.box },
    { id: "connectors", label: "Connectors", icon: I.plug },
    { id: "tools", label: "Agent tools", icon: I.tools },
    { id: "skills", label: "Skills", icon: I.spark },
    { id: "context", label: "Context", icon: I.layers },
    { id: "system", label: "System", icon: I.gear },
  ];

  let query = "";

  $: visible = SECTIONS.filter((s) =>
    !query.trim() || s.label.toLowerCase().includes(query.trim().toLowerCase())
  );
</script>

<aside class="sb settings-nav">
  <div class="sb-head" data-tauri-drag-region>
    <button class="icon-btn" title="Back to workspace" on:click={() => dispatch("back")}>
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d={I.back} /></svg>
    </button>
    <img class="sb-mark" src="/mark.svg" alt="" width="22" height="22" />
    <span class="sb-wordmark">Parzi</span>
    <span class="sn-sub">Settings</span>
  </div>

  <div class="search-row">
    <Icon d={I.search} size={12} />
    <input placeholder="Search" bind:value={query} />
    <span class="search-kbd">/</span>
  </div>

  <div class="sb-scroll">
    {#each visible as s}
      <button class="nav-row" class:on={s.id === activeSection} on:click={() => dispatch("select", { id: s.id })}>
        <Icon d={s.icon} size={13} /><span>{s.label}</span>
        {#if s.id === "models" && authedCount > 0}
          <span class="count-ok">{authedCount}</span>
        {/if}
      </button>
    {/each}
    {#if !visible.length}
      <div class="empty-note">No matching sections</div>
    {/if}
  </div>
</aside>

<style>
  .sb {
    width: 248px; min-width: 248px; height: 100%;
    background: linear-gradient(180deg, color-mix(in srgb, var(--parzi-sidebar) 95%, transparent), color-mix(in srgb, var(--parzi-sidebar) 88%, transparent));
    border-right: 1px solid var(--line-2);
    display: flex; flex-direction: column;
    font-size: 13px; color: var(--text-2);
  }
  .sb-head {
    display: flex; align-items: center; gap: 6px;
    padding: 10px 10px 4px;
  }
  .sb-wordmark {
    font-family: "Instrument Serif", Georgia, serif; font-style: italic; font-size: 17px;
    color: var(--text); padding: 0 2px; user-select: none;
  }
  .sb-mark { width: 22px; height: 22px; border-radius: 6px; flex: none; }
  .sn-sub { font-size: 11px; color: var(--text-3); }
  .icon-btn {
    display: inline-flex; align-items: center; justify-content: center;
    min-width: 26px; height: 26px; padding: 0 4px;
    background: transparent; border: none; border-radius: 6px;
    color: var(--text-3); cursor: pointer; font: inherit;
  }
  .icon-btn:hover { background: var(--surface-2); color: var(--text); }
  .search-row {
    display: flex; align-items: center; gap: 7px; margin: 6px 10px 4px; padding: 6px 9px;
    background: var(--surface-1); border: 1px solid var(--line-2);
    border-radius: 7px; color: var(--text-3);
  }
  .search-row input {
    flex: 1; min-width: 0; background: transparent; border: none; outline: none;
    color: var(--text); font: inherit; font-size: 12.5px;
  }
  .search-row input::placeholder { color: var(--text-4); }
  .search-kbd {
    font-family: var(--parzi-mono); font-size: 10px; padding: 1px 6px;
    border: 1px solid var(--line-3); border-radius: 4px; color: var(--text-3);
  }
  .sb-scroll { flex: 1; overflow-y: auto; padding: 2px 10px 8px; display: flex; flex-direction: column; }
  .nav-row {
    display: flex; align-items: center; gap: 9px; width: 100%;
    background: transparent; border: none; border-radius: 8px; color: var(--text-2);
    font: inherit; font-size: 13px; padding: 8px 10px; cursor: pointer; text-align: left;
  }
  .nav-row:hover { background: var(--surface-2); color: var(--text); }
  .nav-row.on { background: var(--surface-3); color: var(--text); font-weight: 500; }
  .count-ok {
    margin-left: auto; font-size: 10.5px; color: var(--ok);
    border: 1px solid var(--ok-line); border-radius: 5px; padding: 0 5px;
    font-variant-numeric: tabular-nums;
  }
  .empty-note { padding: 10px; font-size: 12px; color: var(--text-4); }
  @media (prefers-reduced-motion: reduce) {
    .nav-row { transition: none; }
  }
</style>
