<script lang="ts">
  // The column on the left: your sessions grouped by project, one card
  // each like the session column's, the project with the newest work
  // first. Helpers stay under their session (the right column lists them).
  import { createEventDispatcher, onDestroy, onMount } from "svelte";
  import InfoCard from "./InfoCard.svelte";
  import { brain, type Project, type Question, type SessionMeta } from "./api";
  import type { Approval } from "./live";
  import { sessionTone, TONE_WORDS } from "./planModel";
  import { ago } from "./format";
  import { tooltip } from "./tooltip";

  export let threads: SessionMeta[] = [];
  export let running: Set<string> = new Set();
  export let approvals: Approval[] = [];
  export let questions: Question[] = [];
  // The session in front.
  export let current = "";

  // A card shows this many until asked for the rest; this many cards, then
  // All sessions.
  const PER = 5;
  const CARDS = 8;
  const dispatch = createEventDispatcher<{ open: { id: string }; all: void }>();

  // Project names, from the brain; a slug it doesn't know reads as itself.
  let titles = new Map<string, string>();
  let asked = "";
  async function loadTitles(key: string) {
    if (key === asked) return;
    asked = key;
    const list: Project[] = await brain.projects().catch(() => []);
    titles = new Map(list.map((p) => [p.slug, p.title || p.slug]));
  }
  onMount(() => void loadTitles("start"));
  // A session in a project not named yet: ask again, once per new set.
  $: unnamed = [...new Set(threads.map((t) => t.project).filter((p) => p && !titles.has(p)))].sort().join("|");
  $: if (unnamed) void loadTitles(unnamed);

  type Group = { key: string; title: string; items: SessionMeta[]; newest: number };
  const time = (s: SessionMeta) => Date.parse(s.updated) || 0;

  $: waiting = new Set([...approvals.map((a) => a.session), ...questions.map((q) => q.session)]);
  $: top = threads.filter((t) => !t.parent_id).sort((a, b) => time(b) - time(a));
  $: pinned = top.filter((t) => t.pinned);
  $: groups = (() => {
    const by = new Map<string, Group>();
    for (const s of top) {
      if (s.pinned) continue;
      // Sessions without a project of their own ("default") go under Other.
      const key = s.project && s.project !== "default" ? s.project : "";
      const g = by.get(key) ?? { key, title: key ? (titles.get(key) ?? key) : "Other", items: [], newest: 0 };
      g.items.push(s);
      g.newest = Math.max(g.newest, time(s));
      by.set(key, g);
    }
    // Newest work first; sessions with no project last.
    return [...by.values()].sort((a, b) => Number(!a.key) - Number(!b.key) || b.newest - a.newest);
  })();
  $: shownGroups = groups.slice(0, CARDS);

  // Cards opened past their first few rows.
  let more = new Set<string>();
  function toggleMore(key: string) {
    const next = new Set(more);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    more = next;
  }

  $: live = (list: SessionMeta[]) => list.filter((t) => sessionTone(t, running, waiting) === "live").length;

  // Ticks the ages over once a minute.
  let now = Date.now();
  const timer = setInterval(() => (now = Date.now()), 60_000);
  onDestroy(() => clearInterval(timer));
  $: age = (at: string) => ago(Date.parse(at), now).replace(" ago", "").replace("just now", "now");
</script>

<aside class="rail" aria-label="Sessions by project">
  {#if pinned.length}
    <InfoCard id="recent:pinned" title="Pinned">
      <svelte:fragment slot="aside">{live(pinned) ? `${live(pinned)} running` : ""}</svelte:fragment>
      {#each pinned as s (s.id)}
        {@const tone = sessionTone(s, running, waiting)}
        <button class="line row" class:here={s.id === current} aria-current={s.id === current ? "page" : undefined} use:tooltip={TONE_WORDS[tone]} on:click={() => dispatch("open", { id: s.id })}>
          <i class="dot t-{tone}" />
          <span class="v">{s.title || "Untitled"}</span>
          <span class="age">{age(s.updated)}</span>
        </button>
      {/each}
    </InfoCard>
  {/if}

  {#each shownGroups as g (g.key)}
    {@const busy = live(g.items)}
    {@const open = more.has(g.key)}
    <InfoCard id="recent:{g.key || 'other'}" title={g.title}>
      <svelte:fragment slot="aside">{busy ? `${busy} running` : g.items.length}</svelte:fragment>
      {#each open ? g.items : g.items.slice(0, PER) as s (s.id)}
        {@const tone = sessionTone(s, running, waiting)}
        <button class="line row" class:here={s.id === current} aria-current={s.id === current ? "page" : undefined} use:tooltip={TONE_WORDS[tone]} on:click={() => dispatch("open", { id: s.id })}>
          <i class="dot t-{tone}" />
          <span class="v">{s.title || "Untitled"}</span>
          <span class="age">{age(s.updated)}</span>
        </button>
      {/each}
      {#if g.items.length > PER}
        <button class="line row quiet" on:click={() => toggleMore(g.key)}>
          <span class="v">{open ? "Show less" : `Show ${g.items.length - PER} more`}</span>
        </button>
      {/if}
    </InfoCard>
  {:else}
    {#if !pinned.length}<p class="none">No sessions yet.</p>{/if}
  {/each}

  {#if groups.length > CARDS}
    <button class="pill quiet" on:click={() => dispatch("all")}>
      <span class="grow">All sessions</span>
      <span class="age">{top.length}</span>
    </button>
  {/if}
</aside>

<style>
  /* Floats beside the session, like the session column on the right. */
  .rail {
    /* Above the window's backdrop, like the session beside it. */
    position: relative;
    z-index: 1;
    flex: none;
    width: 248px;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 0 0 0 var(--gap);
    padding-bottom: 12px;
    overflow-y: auto;
    scrollbar-width: none;
    animation: enter 600ms var(--ease-out);
  }
  @keyframes enter {
    from {
      opacity: 0;
      transform: translateX(-8px);
    }
  }
  /* All sessions: a one-line bubble under the project cards. */
  .pill {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 36px;
    padding: 0 14px;
    background: var(--glass-bg-solid);
    border: var(--glass-border);
    border-radius: var(--radius-lg);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-md);
    font-weight: 600;
    text-align: left;
    cursor: pointer;
  }
  .pill:hover {
    background: var(--hover);
  }
  .grow {
    flex: 1;
  }
  .pill.quiet {
    font-weight: 400;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .line {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 28px;
    color: var(--text);
    font-size: var(--fs-sm);
  }
  .row {
    width: calc(100% + 8px);
    margin: 0 -4px;
    padding: 0 4px;
    background: none;
    border: none;
    border-radius: var(--radius);
    font: inherit;
    font-size: var(--fs-sm);
    text-align: left;
    cursor: pointer;
  }
  .row:hover {
    background: color-mix(in srgb, var(--text) 6%, transparent);
  }
  .row.here {
    background: var(--selected);
  }
  .quiet,
  .quiet .v {
    color: var(--muted);
  }
  .v {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .age {
    flex: none;
    font-size: var(--fs-xs);
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }
  .none {
    margin: 4px 14px;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .dot {
    width: 7px;
    height: 7px;
    flex: none;
    margin: 0 2px;
    border-radius: 50%;
    background: color-mix(in srgb, var(--muted) 60%, transparent);
  }
  .dot.t-live {
    background: var(--accent);
  }
  .dot.t-need {
    background: var(--warn);
  }
  .dot.t-bad {
    background: var(--bad);
  }
  /* Finished is the usual state: only what needs a look has a colour. */
</style>
