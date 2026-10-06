<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import Icon from "./Icon.svelte";
  import type { SessionMeta } from "./api";
  import { faviconUrl, frequentSites, history, pins, removePin, type Pin } from "./browserData";
  import { folderName, hostOf } from "./tabs";

  export let threads: SessionMeta[] = [];

  const dispatch = createEventDispatcher<{ open: { url: string }; openSession: { id: string }; allSessions: void }>();

  let broken = new Set<string>();

  $: sites = ($pins.length ? $pins : frequentSites($history)).slice(0, 8);
  $: pinnedMode = $pins.length > 0;
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

  function label(site: Pin) {
    return site.title || hostOf(site.url);
  }
</script>

<div class="home">
  {#if sites.length}
    <section>
      <h3>{pinnedMode ? "Pinned" : "Frequent"}</h3>
      <div class="sites">
        {#each sites as site (site.url)}
          <div class="site">
            <button class="tile" title={site.url} on:click={() => dispatch("open", { url: site.url })}>
              <span class="icon">
                {#if !broken.has(site.url)}
                  <img
                    src={faviconUrl(site.url)}
                    alt=""
                    on:error={() => (broken = new Set(broken).add(site.url))}
                  />
                {:else}
                  {hostOf(site.url).slice(0, 1).toUpperCase()}
                {/if}
              </span>
              <span class="name">{label(site)}</span>
            </button>
            {#if pinnedMode}
              <button class="unpin" title="Unpin" aria-label="Unpin {label(site)}" on:click={() => removePin(site.url)}>
                <Icon name="close" size={10} stroke={2.2} />
              </button>
            {/if}
          </div>
        {/each}
      </div>
    </section>
  {/if}

  {#if recent.length}
    <section>
      <div class="head">
        <h3>Recent sessions</h3>
        <button class="all" on:click={() => dispatch("allSessions")}>All sessions</button>
      </div>
      <div class="sessions">
        {#each recent as s (s.id)}
          <button class="session" on:click={() => dispatch("openSession", { id: s.id })}>
            <span class="title">{s.title || "Untitled session"}</span>
            <span class="meta">{[s.cwd ? folderName(s.cwd) : "", ago(s.updated)].filter(Boolean).join(" · ")}</span>
          </button>
        {/each}
      </div>
    </section>
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
  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
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
  h3 {
    margin: 0 0 8px 4px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--faint);
  }
  .sites {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(84px, 1fr));
    gap: 6px;
  }
  .site {
    position: relative;
  }
  .tile {
    width: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 7px;
    padding: 12px 6px 9px;
    background: none;
    border: none;
    border-radius: var(--radius-lg);
    color: var(--muted);
    cursor: pointer;
  }
  .tile:hover {
    background: var(--line);
    color: var(--text);
  }
  .icon {
    width: 36px;
    height: 36px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-lg);
    background: var(--panel);
    border: 1px solid var(--line);
    font-size: 14px;
    font-weight: 600;
    color: var(--muted);
  }
  .icon img {
    width: 22px;
    height: 22px;
    padding: 3px;
    background: #fff;
    border-radius: 6px;
  }
  .name {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11.5px;
  }
  .unpin {
    position: absolute;
    top: 4px;
    right: 6px;
    width: 18px;
    height: 18px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 50%;
    color: var(--muted);
    cursor: pointer;
    opacity: 0;
  }
  .site:hover .unpin {
    opacity: 1;
  }
  .unpin:hover {
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
