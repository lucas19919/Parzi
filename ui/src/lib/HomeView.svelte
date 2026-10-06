<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { fade } from "svelte/transition";
  import Icon from "./Icon.svelte";
  import type { SessionMeta } from "./api";
  import { folderName } from "./tabs";

  export let threads: SessionMeta[] = [];

  const dispatch = createEventDispatcher<{ open: { url: string }; openSession: { id: string }; allSessions: void }>();

  let showRecent = false;

  $: recent = threads.slice(0, 5);

  function ago(iso: string) {
    const t = Date.parse(iso);
    if (!t) return "";
    const s = (Date.now() - t) / 1000;
    if (s < 60) return "just now";
    if (s < 3600) return `${Math.floor(s / 60)}m ago`;
    if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
    if (s < 172800) return "yesterday";
    return new Date(t).toLocaleDateString(undefined, { month: "short", day: "numeric" });
  }
</script>

<div class="home">
  {#if recent.length}
    <div class="recent-foot">
      <button class="disclosure" aria-expanded={showRecent} on:click={() => (showRecent = !showRecent)}>
        <span class="tri" class:open={showRecent}>▸</span>
        <span>Recent sessions</span>
      </button>
      <button class="all" on:click={() => dispatch("allSessions")}>All sessions</button>
    </div>
    {#if showRecent}
      <div class="sessions" transition:fade={{ duration: 180 }}>
        {#each recent as s (s.id)}
          <button class="session" on:click={() => dispatch("openSession", { id: s.id })}>
            <span class="title">{s.title || "Untitled session"}</span>
            <span class="meta">{[s.cwd ? folderName(s.cwd) : "", ago(s.updated)].filter(Boolean).join(" · ")}</span>
          </button>
        {/each}
      </div>
    {/if}
  {/if}
</div>

<style>
  .home {
    position: relative;
    z-index: 1;
    display: flex;
    flex-direction: column;
    gap: 22px;
    width: 100%;
  }
  .recent-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 4px;
  }
  .disclosure {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 2px 0;
    background: none;
    border: none;
    color: var(--faint);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    cursor: pointer;
  }
  .disclosure:hover {
    color: var(--text);
  }
  .tri {
    display: inline-block;
    font-size: 10px;
    transition: transform 140ms ease;
  }
  .tri.open {
    transform: rotate(90deg);
  }
  .all {
    padding: 0 4px;
    background: none;
    border: none;
    color: var(--faint);
    font-size: 11.5px;
    cursor: pointer;
  }
  .all:hover {
    color: var(--text);
  }
  .sessions {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .session {
    display: flex;
    align-items: baseline;
    gap: 12px;
    width: 100%;
    padding: 7px 10px;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    text-align: left;
    cursor: pointer;
  }
  .session:hover {
    background: var(--line);
    color: var(--text);
  }
  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
  }
  .meta {
    flex: none;
    font-size: 11.5px;
    color: var(--faint);
  }
</style>
