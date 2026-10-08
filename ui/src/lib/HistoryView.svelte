<script lang="ts">
  import { createEventDispatcher, onMount, tick } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import Icon from "./Icon.svelte";
  import ProviderLogo from "./ProviderLogo.svelte";
  import { hasMark } from "./providerMarks";
  import { folderName } from "./tabs";
  import type { SessionMeta } from "./api";
  import { bookmarks, clearHistory, faviconUrl, history, removeBookmark, removeVisit, type Bookmark, type Visit } from "./browserData";
  import { bare } from "./suggest";

  export let threads: SessionMeta[] = [];
  export let running: Set<string> = new Set();
  export let view: "sessions" | "history" | "bookmarks" = "sessions";

  const dispatch = createEventDispatcher<{ open: { url: string }; openSession: { id: string }; deleteSession: { id: string }; clearSessions: void }>();
  const DAY = 86_400_000;
  const PLACEHOLDER = { sessions: "Search sessions", history: "Search history", bookmarks: "Search bookmarks" };

  let query = "";
  let field: HTMLInputElement | null = null;
  let broken = new Set<string>();

  $: q = query.trim().toLowerCase();
  $: sessionDays = byDay(
    threads.filter((t) => matches(t.title, whereOf(t), q)).map((t) => ({ ...t, at: Date.parse(t.updated) || 0 })),
  );
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

  function agentOf(s: SessionMeta) {
    return s.model.split("/")[0];
  }

  function laneOf(s: SessionMeta): string {
    const l = s.lane ?? "";
    if (l === "code") return "build";
    if (l === "research") return "work";
    return l;
  }

  function laneIcon(s: SessionMeta): "brain" | "bot" | "chat" {
    const l = laneOf(s);
    return l === "work" ? "brain" : l === "build" ? "bot" : "chat";
  }

  function whereOf(s: SessionMeta) {
    if (!s.cwd || /[\\/]\.parzi[\\/]scratch[\\/]/.test(s.cwd)) return "";
    return folderName(s.cwd);
  }

  function byDay<T extends { at: number }>(list: T[]) {
    const out: { label: string; items: T[] }[] = [];
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
    <div class="seg">
      <button class:on={view === "sessions"} on:click={() => show("sessions")}>Sessions</button>
      <button class:on={view === "history"} on:click={() => show("history")}>Pages</button>
      <button class:on={view === "bookmarks"} on:click={() => show("bookmarks")}>Bookmarks</button>
    </div>
    <div class="search">
      <Icon name="search" size={13} />
      <input bind:this={field} bind:value={query} placeholder={PLACEHOLDER[view]} spellcheck="false" />
    </div>
    {#if view === "sessions" && threads.length}
      <button class="link" on:click={() => dispatch("clearSessions")}>Clear sessions</button>
    {/if}
    {#if view === "history" && $history.length}
      <button class="link" on:click={clearAll}>Clear history</button>
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
                {#if hasMark(agentOf(s))}<ProviderLogo provider={agentOf(s)} size={13} />{:else}<Icon name={laneIcon(s)} size={13} />{/if}
              </span>
              <span class="title">{s.title || "Untitled session"}</span>
              <span class="url" class:live={running.has(s.id)}>{running.has(s.id) ? "Running" : whereOf(s)}</span>
            </button>
            <button class="x" title="Delete session" on:click={() => dispatch("deleteSession", { id: s.id })}><Icon name="close" size={12} /></button>
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
  .url.live {
    color: var(--ok);
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
