<script lang="ts">
  import { createEventDispatcher, onMount, tick } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import Icon from "./Icon.svelte";
  import { bookmarks, clearHistory, faviconUrl, history, removeBookmark, removeVisit, type Bookmark, type Visit } from "./browserData";
  import { bare } from "./suggest";

  const dispatch = createEventDispatcher<{ open: { url: string } }>();
  const DAY = 86_400_000;

  let view: "history" | "bookmarks" = "history";
  let query = "";
  let field: HTMLInputElement | null = null;
  let broken = new Set<string>();

  $: q = query.trim().toLowerCase();
  $: days = byDay($history.filter((v) => matches(v.title, v.url, q)));
  $: folders = byFolder($bookmarks.filter((b) => matches(b.title, b.url, q)));

  function matches(title: string, url: string, term: string) {
    return !term || title.toLowerCase().includes(term) || url.toLowerCase().includes(term);
  }

  function dayLabel(at: number) {
    const start = new Date();
    start.setHours(0, 0, 0, 0);
    if (at >= start.getTime()) return "Today";
    if (at >= start.getTime() - DAY) return "Yesterday";
    return new Date(at).toLocaleDateString(undefined, { weekday: "long", month: "long", day: "numeric" });
  }

  function byDay(list: Visit[]) {
    const out: { label: string; items: Visit[] }[] = [];
    for (const v of [...list].sort((a, b) => b.at - a.at)) {
      const label = dayLabel(v.at);
      const last = out[out.length - 1];
      if (last?.label === label) last.items.push(v);
      else out.push({ label, items: [v] });
    }
    return out;
  }

  function byFolder(list: Bookmark[]) {
    const map = new Map<string, Bookmark[]>();
    for (const b of list) {
      const name = b.folder || "Bookmarks";
      map.set(name, [...(map.get(name) ?? []), b]);
    }
    return [...map.entries()].map(([label, items]) => ({ label, items }));
  }

  function time(at: number) {
    return new Date(at).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  }

  async function clearAll() {
    const ok = await ask("Clear all browsing history?", { title: "Clear history", kind: "warning" }).catch(() => false);
    if (ok) clearHistory();
  }

  async function show(next: "history" | "bookmarks") {
    view = next;
    await tick();
    field?.focus();
  }

  onMount(() => {
    field?.focus();
  });
</script>

<div class="library">
  <header>
    <div class="seg">
      <button class:on={view === "history"} on:click={() => show("history")}>History</button>
      <button class:on={view === "bookmarks"} on:click={() => show("bookmarks")}>Bookmarks</button>
    </div>
    <div class="search">
      <Icon name="search" size={13} />
      <input bind:this={field} bind:value={query} placeholder={view === "history" ? "Search history" : "Search bookmarks"} spellcheck="false" />
    </div>
    {#if view === "history" && $history.length}
      <button class="link" on:click={clearAll}>Clear history</button>
    {/if}
  </header>

  <div class="list">
    {#if view === "history"}
      {#each days as day (day.label)}
        <h3>{day.label}</h3>
        {#each day.items as v (v.url)}
          <div class="row">
            <button class="open" title={v.url} on:click={() => dispatch("open", { url: v.url })}>
              <span class="time">{time(v.at)}</span>
              <span class="icon">
                {#if faviconUrl(v.url) && !broken.has(v.url)}
                  <img src={faviconUrl(v.url)} alt="" on:error={() => (broken = new Set(broken).add(v.url))} />
                {:else}
                  <Icon name="globe" size={13} />
                {/if}
              </span>
              <span class="title">{v.title || bare(v.url)}</span>
              <span class="url">{bare(v.url)}</span>
            </button>
            <button class="x" title="Remove from history" on:click={() => removeVisit(v.url)}><Icon name="close" size={12} /></button>
          </div>
        {/each}
      {:else}
        <p class="empty">{q ? "Nothing matches." : "Pages you visit show up here."}</p>
      {/each}
    {:else}
      {#each folders as folder (folder.label)}
        <h3>{folder.label}</h3>
        {#each folder.items as b (b.url)}
          <div class="row">
            <button class="open" title={b.url} on:click={() => dispatch("open", { url: b.url })}>
              <span class="icon">
                {#if faviconUrl(b.url) && !broken.has(b.url)}
                  <img src={faviconUrl(b.url)} alt="" on:error={() => (broken = new Set(broken).add(b.url))} />
                {:else}
                  <Icon name="globe" size={13} />
                {/if}
              </span>
              <span class="title">{b.title || bare(b.url)}</span>
              <span class="url">{bare(b.url)}</span>
            </button>
            <button class="x" title="Remove bookmark" on:click={() => removeBookmark(b.url)}><Icon name="close" size={12} /></button>
          </div>
        {/each}
      {:else}
        <p class="empty">{q ? "Nothing matches." : "Bookmarks you import during setup show up here."}</p>
      {/each}
    {/if}
  </div>
</div>

<style>
  .library {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  header {
    flex: none;
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    max-width: 820px;
    margin: 0 auto;
    padding: 20px 24px 12px;
  }
  .seg {
    flex: none;
    display: inline-flex;
    padding: 2px;
    background: var(--panel);
    border-radius: 999px;
  }
  .seg button {
    padding: 4px 12px;
    background: none;
    border: none;
    border-radius: 999px;
    color: var(--muted);
    font-size: 12.5px;
    cursor: pointer;
  }
  .seg button:hover {
    color: var(--text);
  }
  .seg button.on {
    background: var(--line);
    color: var(--text);
  }
  .search {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    padding: 0 12px;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--faint);
  }
  .search input {
    flex: 1;
    min-width: 0;
    background: none;
    border: none;
    outline: none;
    box-shadow: none !important;
    color: var(--text);
    font-size: 13px;
  }
  .link {
    flex: none;
    padding: 4px 8px;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    font-size: 12.5px;
    cursor: pointer;
  }
  .link:hover {
    color: var(--bad);
  }
  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    width: 100%;
    max-width: 820px;
    margin: 0 auto;
    padding: 0 24px 40px;
  }
  h3 {
    margin: 18px 0 6px 10px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--faint);
  }
  .row {
    display: flex;
    align-items: center;
    border-radius: var(--radius);
  }
  .row:hover {
    background: color-mix(in srgb, var(--text) 5%, transparent);
  }
  .open {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    height: 34px;
    padding: 0 10px;
    background: none;
    border: none;
    color: var(--text);
    font-size: 13px;
    text-align: left;
    cursor: pointer;
  }
  .time {
    flex: none;
    min-width: 44px;
    white-space: nowrap;
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
    color: var(--faint);
  }
  .icon {
    flex: none;
    width: 16px;
    display: inline-flex;
    justify-content: center;
    color: var(--faint);
  }
  .icon img {
    width: 14px;
    height: 14px;
  }
  .title {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .url {
    flex: 1 1 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    color: var(--faint);
  }
  .x {
    width: 28px;
    height: 28px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    margin-right: 4px;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--faint);
    cursor: pointer;
    opacity: 0;
  }
  .row:hover .x,
  .x:focus-visible {
    opacity: 1;
  }
  .x:hover {
    background: var(--line);
    color: var(--text);
  }
  .empty {
    margin: 40px 0;
    text-align: center;
    font-size: 13px;
    color: var(--faint);
  }
</style>
