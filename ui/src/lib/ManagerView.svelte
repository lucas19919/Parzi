<script lang="ts">
  import { createEventDispatcher, onMount, tick } from "svelte";
  import Icon from "./Icon.svelte";
  import Page from "./Page.svelte";
  import ProviderLogo from "./ProviderLogo.svelte";
  import { activity, sessionMarks } from "./activity";
  import { brain, type Project, type SessionMeta } from "./api";
  import { hasMark } from "./providerMarks";
  import { nameOf } from "./providerRows";
  import { folderName } from "./tabs";
  import { byDay, formatTime } from "./time";
  import { tooltip } from "./tooltip";

  /** Sessions, by the questions a person brings here: what needs me, what is
   *  working, and where is the one from earlier. Type to find, arrows and
   *  Enter to open. */
  export let threads: SessionMeta[] = [];

  const dispatch = createEventDispatcher<{
    openSession: { id: string };
    deleteSession: { id: string };
    newSession: void;
  }>();

  let query = "";
  let field: HTMLInputElement | null = null;
  let list: HTMLElement | null = null;
  let only: string | null = null;
  let projects: Project[] = [];
  let pick = 0;

  onMount(() => {
    field?.focus();
    brain
      .projects()
      .then((p) => (projects = p ?? []))
      .catch(() => {});
  });

  type Mark = "waiting" | "running" | "";
  const ownMark = (s: SessionMeta, marks: Record<string, string>): Mark =>
    marks[s.id] === "waiting" ? "waiting" : marks[s.id] === "running" || s.status === "active" ? "running" : "";

  // Helpers (subagents) live under the session that started them; their
  // work marks that session. One whose parent is gone stays visible.
  // Drafts aren't sessions yet: they stay on the card that wrote them.
  $: byId = new Map(threads.map((t) => [t.id, t]));
  $: tops = threads.filter((t) => t.status !== "draft" && (!t.parent_id || !byId.has(t.parent_id)));
  $: topOf = (t: SessionMeta): SessionMeta => {
    const seen = new Set<string>();
    let cur = t;
    while (cur.parent_id && byId.has(cur.parent_id) && !seen.has(cur.id)) {
      seen.add(cur.id);
      cur = byId.get(cur.parent_id) as SessionMeta;
    }
    return cur;
  };
  $: marks = (() => {
    const m = new Map<string, Mark>();
    for (const t of threads) {
      const own = ownMark(t, $sessionMarks);
      if (!own) continue;
      const top = topOf(t).id;
      // A helper's work marks the session above it.
      if (own === "waiting" || !m.get(top)) m.set(top, own);
    }
    return m;
  })();
  /** What a working session is doing now: its newest tool call, its own or a helper's. */
  $: doing = (() => {
    const m = new Map<string, { label: string; at: number }>();
    for (const [id, a] of $activity) {
      const t = byId.get(id);
      if (!t) continue;
      const top = topOf(t).id;
      if ((m.get(top)?.at ?? 0) < a.at) m.set(top, a);
    }
    return m;
  })();

  const projectOf = (t: SessionMeta): string =>
    (t.project && projects.find((x) => x.slug === t.project)?.title) || "";
  const placeOf = (t: SessionMeta): string => {
    const p = projectOf(t);
    if (p) return p;
    const leaf = folderName(t.cwd ?? "");
    return leaf && !/scratch/i.test(t.cwd ?? "") ? leaf : "";
  };
  const agentOf = (t: SessionMeta) => t.model.split("/")[0] ?? "";

  $: places = (() => {
    const m = new Map<string, number>();
    for (const t of tops) {
      const p = projectOf(t);
      if (p) m.set(p, (m.get(p) ?? 0) + 1);
    }
    return [...m].sort((a, b) => b[1] - a[1]);
  })();

  $: q = query.trim().toLowerCase();
  $: shown = tops
    .filter((t) => !only || projectOf(t) === only)
    .filter((t) => !q || t.title.toLowerCase().includes(q) || placeOf(t).toLowerCase().includes(q) || (t.cwd ?? "").toLowerCase().includes(q))
    .map((t) => ({ ...t, at: Date.parse(t.updated) || 0, mark: marks.get(t.id) ?? ("" as Mark) }))
    .sort((a, b) => b.at - a.at);
  $: groups = [
    { label: "Needs you", items: shown.filter((t) => t.mark === "waiting") },
    { label: "Working", items: shown.filter((t) => t.mark === "running") },
    ...byDay(shown.filter((t) => !t.mark)),
  ].filter((g) => g.items.length);
  $: flat = groups.flatMap((g) => g.items);
  $: if (pick >= flat.length) pick = Math.max(0, flat.length - 1);
  // A new search starts at the top.
  $: q, only, (pick = 0);

  function lineOf(t: (typeof shown)[number]): string {
    const where = placeOf(t);
    const what =
      t.mark === "waiting" ? "Waiting for you" : t.mark === "running" ? doing.get(t.id)?.label || "Working" : "";
    const steps = t.plan?.items ?? [];
    const left = steps.some((i) => i.state !== "done");
    const progress = steps.length && left ? `${steps.filter((i) => i.state === "done").length} of ${steps.length}` : "";
    return [where, what, progress].filter(Boolean).join(" · ");
  }

  function whenOf(t: (typeof shown)[number]): string {
    if (t.mark === "waiting" || t.mark === "running") return "";
    return Date.now() - t.at < 86_400_000 * 2 ? formatTime(t.at) : new Date(t.at).toLocaleDateString(undefined, { month: "short", day: "numeric" });
  }

  async function onKey(e: KeyboardEvent) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      pick = Math.max(0, Math.min(flat.length - 1, pick + (e.key === "ArrowDown" ? 1 : -1)));
      await tick();
      list?.querySelector(".sess.on")?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "Enter" && flat[pick]) {
      e.preventDefault();
      dispatch("openSession", { id: flat[pick].id });
    }
  }
</script>

<Page>
  <div slot="bar" class="page-tabs"><span class="page-title">Sessions</span></div>

  <div class="page-search">
    <Icon name="search" />
    <input
      bind:this={field}
      bind:value={query}
      on:keydown={onKey}
      placeholder="Find a session"
      aria-label="Find a session"
      spellcheck="false"
    />
  </div>

  {#if places.length > 1}
    <div class="chips" role="group" aria-label="Project">
      <button class="chip" class:on={!only} aria-pressed={!only} on:click={() => (only = null)}>All</button>
      {#each places as [p, n] (p)}
        <button class="chip" class:on={only === p} aria-pressed={only === p} on:click={() => (only = only === p ? null : p)}>{p}<span class="n">{n}</span></button>
      {/each}
    </div>
  {/if}

  <div class="groups" bind:this={list}>
    {#each groups as g (g.label)}
      <div class="rows">
        <div class="row row-title"><span>{g.label}</span>{#if g.label === "Needs you" || g.label === "Working"}<span class="t-hint">{g.items.length}</span>{/if}</div>
        {#each g.items as s (s.id)}
          {@const agent = agentOf(s)}
          <div class="row sess" class:on={flat[pick]?.id === s.id}>
            <button class="row-main" title={s.title} on:click={() => dispatch("openSession", { id: s.id })} on:mouseenter={() => (pick = flat.indexOf(s))}>
              <span class="agent" use:tooltip={nameOf(agent)}>
                {#if hasMark(agent)}<ProviderLogo provider={agent} size={14} mono />{:else}<Icon name="chat" size={14} />{/if}
                {#if s.mark === "waiting" || s.mark === "running"}<span class="mark {s.mark}" aria-label={s.mark === "waiting" ? "Waiting for you" : "Working"} role="img" />{/if}
              </span>
              <span class="row-label">
                {s.title || "Untitled session"}
                {#if lineOf(s)}<small class:warn={s.mark === "waiting"}>{lineOf(s)}</small>{/if}
              </span>
              {#if whenOf(s)}<span class="when">{whenOf(s)}</span>{/if}
            </button>
            <button class="icon-btn x" use:tooltip={"Delete session"} aria-label={`Delete ${s.title || "session"}`} on:click={() => dispatch("deleteSession", { id: s.id })}>
              <Icon name="trash" size={12} />
            </button>
          </div>
        {/each}
      </div>
    {:else}
      <p class="empty t-hint">{q || only ? "Nothing matches." : "No sessions yet. Start one with + in the tab bar."}</p>
    {/each}
  </div>
</Page>

<style>
  .page-search {
    flex: none;
    height: var(--h-field);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: calc(var(--space) * 2);
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    height: var(--h);
    padding: 0 calc(var(--space) * 3);
    background: var(--fill);
    border: 1px solid transparent;
    border-radius: var(--radius);
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-sm);
    font-weight: 500;
    cursor: pointer;
  }
  .chip:hover {
    color: var(--text);
  }
  .chip.on {
    background: var(--accent);
    color: var(--bg);
  }
  .n {
    opacity: 0.7;
    font-variant-numeric: tabular-nums;
  }
  .groups {
    display: flex;
    flex-direction: column;
    gap: var(--gap);
  }
  .sess.on {
    background: var(--hover);
  }
  .agent {
    position: relative;
    flex: none;
    width: 28px;
    height: 28px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius);
    background: var(--fill);
    color: var(--text);
  }
  /* The tab bar's marks, on the corner of the agent: they take no room, so
     the list never shifts when something starts or stops. One tone, like the
     tab bar's spinner: the arc in the muted colour, its track the same, fainter. */
  .mark {
    position: absolute;
    right: -3px;
    top: -3px;
    box-sizing: border-box;
    border-radius: 50%;
  }
  .mark.running {
    width: 11px;
    height: 11px;
    border: 1.6px solid color-mix(in srgb, var(--muted) 30%, transparent);
    border-top-color: var(--muted);
    animation: spin 0.8s linear infinite;
  }
  .mark.waiting {
    width: 9px;
    height: 9px;
    background: var(--warn);
  }
  @media (prefers-reduced-motion: reduce) {
    .mark.running {
      animation: none;
    }
  }
  small.warn {
    color: var(--warn-text);
  }
  .when {
    flex: none;
    font-size: var(--fs-sm);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .x {
    opacity: 0;
  }
  .sess:hover .x,
  .sess:focus-within .x {
    opacity: 1;
  }
  .x:hover:not(:disabled) {
    color: var(--bad-text);
  }
  .empty {
    margin: 0;
    padding: calc(var(--space) * 8) calc(var(--space) * 4);
    text-align: center;
  }
</style>
