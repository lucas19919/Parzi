<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { fade } from "svelte/transition";
  import type { SessionMeta } from "./api";
  import { folderName } from "./tabs";

  export let threads: SessionMeta[] = [];
  export let running: Set<string> = new Set();

  const dispatch = createEventDispatcher<{ open: { url: string }; openSession: { id: string }; allSessions: void }>();

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
    <div class="recent-head">
      <span>Recent sessions</span>
      <button class="all" on:click={() => dispatch("allSessions")}>All sessions</button>
    </div>
    <div class="sessions">
      {#each recent as s (s.id)}
        <button class="session" on:click={() => dispatch("openSession", { id: s.id })}>
          <span class="dot" class:live={running.has(s.id)} />
          <span class="title">{s.title || "Untitled session"}</span>
          <span class="meta">{[s.cwd ? folderName(s.cwd) : "", ago(s.updated)].filter(Boolean).join(" · ")}</span>
          <span class="go">›</span>
        </button>
      {/each}
    </div>
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
    /* The hero composer is bottom-anchored and overlaps the top of the
       home box; clear it so the disclosure never slides underneath. */
    padding-top: 48px;
  }
  .recent-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    padding: 0 10px 2px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--faint);
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
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 8px 10px;
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
  .dot {
    width: 6px;
    height: 6px;
    flex: none;
    border-radius: 50%;
    background: var(--faint);
    opacity: 0.6;
  }
  .dot.live {
    background: var(--ok);
    opacity: 1;
  }
  .go {
    flex: none;
    color: var(--faint);
    font-size: 15px;
    line-height: 1;
  }
  .session:hover .go {
    color: var(--text);
  }
</style>
