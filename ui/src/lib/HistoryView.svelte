<script lang="ts">
  import { createEventDispatcher, onMount, tick } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import Icon from "./Icon.svelte";
  import ProviderLogo from "./ProviderLogo.svelte";
  import { hasMark } from "./providerMarks";
  import type { SessionMeta } from "./api";
  import { bookmarks, clearHistory, faviconUrl, history, removeBookmark, removeVisit, type Bookmark } from "./browserData";
  import { downloads, type Download } from "./api";
  import { downloadList } from "./browserState";
  import { downloadNote, downloadShare } from "./pageRules";
  import { toast } from "./toast";
  import { bare } from "./suggest";
  import { agentOf, whereOf } from "./sessions";
  import { laneIcon, normLane } from "./lanes";
  import { byDay, formatTime as time } from "./time";
  import { tooltip } from "./tooltip";

  export let threads: SessionMeta[] = [];
  export let running: Set<string> = new Set();
  export let view: "sessions" | "history" | "bookmarks" | "downloads" = "sessions";

  const dispatch = createEventDispatcher<{ open: { url: string }; openSession: { id: string }; deleteSession: { id: string }; clearSessions: void }>();
  const PLACEHOLDER = { sessions: "Search sessions", history: "Search history", bookmarks: "Search bookmarks", downloads: "Search downloads" };

  // Lists render a page at a time; "Show more" adds the next page.
  const PAGE = 200;

  let query = "";
  let field: HTMLInputElement | null = null;
  let broken = new Set<string>();
  let limit = PAGE;

  $: q = query.trim().toLowerCase();
  $: q, view, (limit = PAGE);
  $: sessionHits = threads
    .filter((t) => matches(t.title, whereOf(t), q))
    .map((t) => ({ ...t, at: Date.parse(t.updated) || 0 }))
    .sort((a, b) => b.at - a.at);
  $: sessionDays = byDay(sessionHits.slice(0, limit));
  $: visitHits = $history.filter((v) => matches(v.title, v.url, q));
  $: days = byDay(visitHits.slice(0, limit));
  $: markHits = $bookmarks.filter((b) => matches(b.title, b.url, q));
  $: folders = byFolder(markHits.slice(0, limit));
  $: fileHits = $downloadList.filter((d) => matches(d.name, d.url, q));
  $: fileDays = byDay(fileHits.slice(0, limit).map((d) => ({ ...d, at: d.started })));
  $: total =
    view === "sessions" ? sessionHits.length : view === "history" ? visitHits.length : view === "bookmarks" ? markHits.length : fileHits.length;

  function matches(title: string, url: string, term: string) {
    return !term || title.toLowerCase().includes(term) || url.toLowerCase().includes(term);
  }

  function laneOf(s: SessionMeta): string {
    return normLane(s.lane ?? "");
  }

  function laneIconFor(s: SessionMeta): "brain" | "bot" | "chat" {
    return laneIcon(s.lane ?? "");
  }

  function byFolder(list: Bookmark[]) {
    const map = new Map<string, Bookmark[]>();
    for (const b of list) {
      const name = b.folder || "Bookmarks";
      map.set(name, [...(map.get(name) ?? []), b]);
    }
    return [...map.entries()].map(([label, items]) => ({ label, items }));
  }

  async function clearAll() {
    const ok = await ask("Clear all browsing history?", { title: "Clear history", kind: "warning" }).catch(() => false);
    if (ok) clearHistory();
  }

  async function reveal(d: Download) {
    try {
      await downloads.reveal(d.id);
    } catch (e) {
      toast(String(e), true);
    }
  }

  async function cancel(d: Download) {
    try {
      await downloads.cancel(d.id);
    } catch (e) {
      toast(String(e), true);
    }
  }

  async function clearFiles() {
    try {
      downloadList.set(await downloads.clear());
    } catch (e) {
      toast(String(e), true);
    }
  }

  async function show(next: typeof view) {
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
    <div class="seg" role="group" aria-label="Library" data-ui-segmented>
      <button class:on={view === "sessions"} aria-pressed={view === "sessions"} on:click={() => show("sessions")}>Sessions</button>
      <button class:on={view === "history"} aria-pressed={view === "history"} on:click={() => show("history")}>Pages</button>
      <button class:on={view === "bookmarks"} aria-pressed={view === "bookmarks"} on:click={() => show("bookmarks")}>Bookmarks</button>
      <button class:on={view === "downloads"} aria-pressed={view === "downloads"} on:click={() => show("downloads")}>Downloads</button>
    </div>
    <div class="search">
      <Icon name="search" />
      <input bind:this={field} bind:value={query} placeholder={PLACEHOLDER[view]} aria-label={PLACEHOLDER[view]} spellcheck="false" />
    </div>
    {#if view === "sessions" && threads.length}
      <button class="link" on:click={() => dispatch("clearSessions")}>Clear sessions</button>
    {/if}
    {#if view === "history" && $history.length}
      <button class="link" on:click={clearAll}>Clear history</button>
    {/if}
    {#if view === "downloads" && $downloadList.some((d) => d.state !== "progress")}
      <button class="link" use:tooltip={"Clears the list; the files stay"} on:click={clearFiles}>Clear list</button>
    {/if}
  </header>

  <div class="list">
    {#if view === "sessions"}
      {#each sessionDays as day (day.label)}
        <h3>{day.label}</h3>
        {#each day.items as s (s.id)}
          <div class="row">
            <button class="open" title={s.title} on:click={() => dispatch("openSession", { id: s.id })}>
              <span class="time">{time(s.at)}</span>
              <span class="icon">
                {#if hasMark(agentOf(s))}<ProviderLogo provider={agentOf(s)} size={13} />{:else}<Icon name={laneIconFor(s)} />{/if}
              </span>
              <span class="title">{s.title || "Untitled session"}</span>
              <span class="url" class:live={running.has(s.id)}>{running.has(s.id) ? "Running" : whereOf(s)}</span>
            </button>
            <button class="x" use:tooltip={"Delete session"} on:click={() => dispatch("deleteSession", { id: s.id })}><Icon name="close" size={12} /></button>
          </div>
        {/each}
      {:else}
        <p class="empty">{q ? "Nothing matches." : "Your sessions show up here."}</p>
      {/each}
    {:else if view === "history"}
      {#each days as day (day.label)}
        <h3>{day.label}</h3>
        {#each day.items as v (v.url)}
          <div class="row">
            <button class="open" title={v.url} on:click={() => dispatch("open", { url: v.url })}>
              <span class="time">{time(v.at)}</span>
              <span class="icon">
                {#if faviconUrl(v.url) && !broken.has(v.url)}
                  <img src={faviconUrl(v.url)} alt="" loading="lazy" on:error={() => (broken = new Set(broken).add(v.url))} />
                {:else}
                  <Icon name="globe" />
                {/if}
              </span>
              <span class="title">{v.title || bare(v.url)}</span>
              <span class="url">{bare(v.url)}</span>
            </button>
            <button class="x" use:tooltip={"Remove from history"} on:click={() => removeVisit(v.url)}><Icon name="close" size={12} /></button>
          </div>
        {/each}
      {:else}
        <p class="empty">{q ? "Nothing matches." : "Pages you visit show up here."}</p>
      {/each}
    {:else if view === "downloads"}
      {#each fileDays as day (day.label)}
        <h3>{day.label}</h3>
        {#each day.items as d (d.id)}
          {@const share = downloadShare(d)}
          <div class="row">
            <button class="open" title={d.path} disabled={d.state !== "done"} on:click={() => reveal(d)}>
              <span class="time">{time(d.at)}</span>
              <span class="icon"><Icon name="download" /></span>
              <span class="title">{d.name}</span>
              <span class="url" class:live={d.state === "progress"} class:bad={d.state === "failed"}>{downloadNote(d)}</span>
              {#if share !== undefined}<span class="meter" aria-hidden="true"><span style:width="{Math.round(share * 100)}%" /></span>{/if}
            </button>
            {#if d.state === "progress"}
              <button class="x shown" use:tooltip={"Cancel download"} on:click={() => cancel(d)}><Icon name="close" size={12} /></button>
            {:else if d.state === "done"}
              <button class="x" use:tooltip={"Show in folder"} on:click={() => reveal(d)}><Icon name="folder" size={12} /></button>
            {/if}
          </div>
        {/each}
      {:else}
        <p class="empty">{q ? "Nothing matches." : "Files you download from pages show up here."}</p>
      {/each}
    {:else}
      {#each folders as folder (folder.label)}
        <h3>{folder.label}</h3>
        {#each folder.items as b (b.url)}
          <div class="row">
            <button class="open" title={b.url} on:click={() => dispatch("open", { url: b.url })}>
              <span class="icon">
                {#if faviconUrl(b.url) && !broken.has(b.url)}
                  <img src={faviconUrl(b.url)} alt="" loading="lazy" on:error={() => (broken = new Set(broken).add(b.url))} />
                {:else}
                  <Icon name="globe" />
                {/if}
              </span>
              <span class="title">{b.title || bare(b.url)}</span>
              <span class="url">{bare(b.url)}</span>
            </button>
            <button class="x" use:tooltip={"Remove bookmark"} on:click={() => removeBookmark(b.url)}><Icon name="close" size={12} /></button>
          </div>
        {/each}
      {:else}
        <p class="empty">{q ? "Nothing matches." : "Bookmarks you import during setup show up here."}</p>
      {/each}
    {/if}
    {#if total > limit}
      <button class="more" on:click={() => (limit += PAGE)}>Show more</button>
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
    font-size: var(--fs-md);
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
    color: var(--muted);
  }
  .search input {
    flex: 1;
    min-width: 0;
    background: none;
    border: none;
    outline: none;
    box-shadow: none !important;
    color: var(--text);
    font-size: var(--fs-md);
  }
  .link {
    flex: none;
    padding: 4px 8px;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--fs-md);
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
    font-size: var(--fs-md);
    font-weight: 600;
    color: var(--muted);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
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
    font-size: var(--fs-md);
    text-align: left;
    cursor: pointer;
  }
  .time {
    flex: none;
    min-width: 44px;
    white-space: nowrap;
    font-size: var(--fs-sm);
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }
  .icon {
    flex: none;
    width: 16px;
    display: inline-flex;
    justify-content: center;
    color: var(--muted);
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
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .url.live {
    color: var(--ok);
  }
  .url.bad {
    color: var(--bad);
  }
  .open:disabled {
    cursor: default;
  }
  .meter {
    flex: none;
    width: 80px;
    height: 4px;
    border-radius: 999px;
    background: var(--line);
    overflow: hidden;
  }
  .meter span {
    display: block;
    height: 100%;
    background: var(--accent);
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
    color: var(--muted);
    cursor: pointer;
    opacity: 0;
  }
  .row:hover .x,
  .x.shown,
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
    font-size: var(--fs-md);
    color: var(--muted);
  }
  .more {
    display: block;
    margin: 16px auto 0;
    padding: 5px 14px;
    background: none;
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--muted);
    font-size: var(--fs-md);
    cursor: pointer;
    transition: color 140ms ease, background 140ms ease;
  }
  .more:hover {
    background: var(--line);
    color: var(--text);
  }
  .more:active {
    transform: scale(0.97);
  }
</style>
