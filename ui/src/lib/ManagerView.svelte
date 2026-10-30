<script lang="ts">
  import { createEventDispatcher, onDestroy, onMount } from "svelte";
  import { fade } from "svelte/transition";
  import Icon from "./Icon.svelte";
  import { brain, type Project, type SessionMeta } from "./api";
  import { folderName } from "./tabs";
  import { byDay } from "./time";
  import { tooltip } from "./tooltip";
  import { toast } from "./toast";
  import { setOverlay } from "./overlay";

  export let threads: SessionMeta[] = [];

  const dispatch = createEventDispatcher<{
    openSession: { id: string };
    deleteSession: { id: string };
    newSession: void;
    settings: void;
    setup: void;
    close: void;
  }>();

  function close() {
    dispatch("close");
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape" && !e.defaultPrevented) {
      e.preventDefault();
      close();
    }
  }

  let query = "";
  let field: HTMLInputElement | null = null;
  let projectFilter: string | null = null;
  let projects: Project[] = [];
  let projectsError = false;
  let adding = false;
  let newTitle = "";
  let newFolder = "";
  let saving = false;

  async function loadProjects() {
    projectsError = false;
    try {
      projects = (await brain.projects()) ?? [];
    } catch {
      projects = [];
      projectsError = true;
    }
  }

  async function addProject() {
    const title = newTitle.trim();
    if (!title || saving) return;
    saving = true;
    try {
      await brain.upsertProject(title, newFolder.trim());
      newTitle = "";
      newFolder = "";
      adding = false;
      await loadProjects();
    } catch (e) {
      toast(`Could not add the project: ${e}`, true);
    } finally {
      saving = false;
    }
  }

  onMount(() => {
    setOverlay("manager", true);
    field?.focus();
    void loadProjects();
  });
  onDestroy(() => {
    setOverlay("manager", false);
  });

  const projectOf = (slug: string): Project | undefined => projects.find((p) => p.slug === slug);
  const placeOf = (t: SessionMeta): string => {
    const p = t.project ? projectOf(t.project) : undefined;
    if (p) return p.title;
    const leaf = folderName(t.cwd ?? "");
    return leaf && !/scratch/i.test(t.cwd ?? "") ? leaf : "";
  };
  const initialOf = (t: SessionMeta): string => {
    const label = placeOf(t) || t.title || "?";
    return label.trim().charAt(0).toUpperCase() || "?";
  };
  const hueOf = (key: string): number => {
    let h = 0;
    for (const c of key) h = (h * 31 + c.charCodeAt(0)) % 360;
    return h;
  };

  $: q = query.trim().toLowerCase();
  $: hits = threads
    .filter((t) => !t.parent_id || threads.some((p) => p.id === t.parent_id))
    .filter((t) => !projectFilter || t.project === projectFilter)
    .filter((t) => {
      if (!q) return true;
      const p = t.project ? projectOf(t.project) : undefined;
      return (
        t.title.toLowerCase().includes(q) ||
        (p?.title ?? "").toLowerCase().includes(q) ||
        (t.cwd ?? "").toLowerCase().includes(q)
      );
    })
    .map((t) => ({ ...t, at: Date.parse(t.updated) || 0 }))
    .sort((a, b) => b.at - a.at);
  $: days = byDay(hits);
  $: counts = (() => {
    const m = new Map<string, number>();
    for (const t of threads) m.set(t.project || "", (m.get(t.project || "") ?? 0) + 1);
    return m;
  })();

  function toggleProject(slug: string) {
    projectFilter = projectFilter === slug ? null : slug;
  }
</script>

<svelte:window on:keydown={onKey} />

<!-- svelte-ignore a11y-no-static-element-interactions a11y-click-events-have-key-events -->
<div class="backdrop" on:click|self={close} transition:fade={{ duration: 140 }}>
  <div class="box" role="dialog" aria-modal="true" aria-label="Session manager" transition:fade={{ duration: 140 }}>
<div class="manager">
  <aside class="rail" aria-label="Projects">
    <div class="rail-head">
      <span>Projects</span>
      <button class="mini-btn" aria-label="Add project" use:tooltip={"Add project"} on:click={() => (adding = !adding)}>
        <Icon name="plus" size={12} />
      </button>
    </div>
    {#if adding}
      <div class="add-form">
        <input class="tin" bind:value={newTitle} placeholder="Project name" aria-label="Project name" spellcheck="false" />
        <input class="tin" bind:value={newFolder} placeholder="Folder (optional)" aria-label="Project folder" spellcheck="false" />
        <div class="add-acts">
          <button class="sbtn" on:click={() => (adding = false)}>Cancel</button>
          <button class="sbtn primary" disabled={saving || !newTitle.trim()} on:click={addProject}>{saving ? "Adding…" : "Add"}</button>
        </div>
      </div>
    {/if}
    <div class="proj-list">
      {#if projectsError}
        <div class="rail-note">
          <span>Couldn't load projects</span>
          <button class="sbtn" on:click={loadProjects}>Retry</button>
        </div>
      {:else}
        {#each projects as p (p.slug)}
          <button
            class="proj"
            class:on={projectFilter === p.slug}
            title={projectFilter === p.slug ? "Show all sessions" : `Show only ${p.title}`}
            on:click={() => toggleProject(p.slug)}
          >
            <span class="badge" style="--badge-h: {hueOf(p.slug)}">{p.title.trim().charAt(0).toUpperCase()}</span>
            <span class="name">{p.title}</span>
            {#if counts.get(p.slug)}<span class="count">{counts.get(p.slug)}</span>{/if}
          </button>
        {:else}
          <p class="rail-note">No projects yet</p>
        {/each}
      {/if}
    </div>
    <div class="rail-foot">
      <button class="foot" on:click={() => dispatch("settings")}><Icon name="settings" size={12} /><span>Settings</span></button>
      <button class="foot" on:click={() => dispatch("setup")}><Icon name="alert" size={12} /><span>Help</span></button>
    </div>
  </aside>
  <div class="main">
    <div class="top">
      <div class="page-search">
        <Icon name="search" />
        <input bind:this={field} bind:value={query} placeholder="Search sessions" aria-label="Search sessions" spellcheck="false" />
      </div>
      <button class="sbtn new" on:click={() => dispatch("newSession")}><Icon name="pencil" size={12} /><span>New session</span></button>
    </div>
    <div class="list" data-ui-list>
      {#each days as day (day.label)}
        <h3>{day.label}</h3>
        {#each day.items as s (s.id)}
          {@const place = placeOf(s)}
          <div class="row">
            <button class="open" title={s.title} on:click={() => dispatch("openSession", { id: s.id })}>
              <span class="badge" style="--badge-h: {hueOf(s.project || place || s.id)}">{initialOf(s)}</span>
              <span class="title">{s.title || "Untitled session"}</span>
              {#if place}<span class="tag">{place}</span>{/if}
            </button>
            <button class="x" use:tooltip={"Delete session"} aria-label={`Delete ${s.title || "session"}`} on:click={() => dispatch("deleteSession", { id: s.id })}>
              <Icon name="trash" size={12} />
            </button>
          </div>
        {/each}
      {:else}
        <p class="empty">{q || projectFilter ? "Nothing matches." : "No sessions yet. Start one above."}</p>
      {/each}
    </div>
  </div>
</div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 1000;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: calc(var(--space) * 6);
    background: var(--scrim);
  }
  .box {
    width: min(920px, 100%);
    height: min(680px, 100%);
    min-height: 0;
    flex: none;
    display: flex;
    flex-direction: column;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    overflow: hidden;
    outline: none;
  }
  .manager {
    flex: 1;
    min-height: 0;
    display: flex;
    background: transparent;
  }
  .rail {
    width: 220px;
    flex: none;
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-right: 1px solid var(--line);
  }
  .rail-head {
    flex: none;
    display: flex;
    align-items: center;
    /* Same 64px band as .top (16px inset + 32px control) so rail and search share a line. */
    min-height: calc(var(--space) * 16);
    padding: calc(var(--space) * 4);
    color: var(--muted);
    font-size: var(--fs-md);
    font-weight: 600;
  }
  .rail-head > span {
    flex: 1;
  }
  .mini-btn {
    width: 28px;
    height: 28px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .mini-btn:hover {
    background: var(--hover);
    color: var(--text);
  }
  .add-form {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: var(--space);
    padding: var(--space) calc(var(--space) * 2);
  }
  .add-form .tin {
    width: 100%;
  }
  .add-acts {
    display: flex;
    justify-content: flex-end;
    gap: var(--space);
  }
  .add-acts .sbtn {
    height: calc(var(--space) * 7);
  }
  .rail-note {
    margin: 0;
    padding: var(--space) calc(var(--space) * 3);
    color: var(--muted);
    font-size: var(--fs-sm);
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    align-items: flex-start;
  }
  .proj-list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    padding: 0 var(--space);
  }
  .proj {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    min-height: 36px;
    padding: var(--space) calc(var(--space) * 2);
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-sm);
    text-align: left;
    cursor: pointer;
  }
  .proj:hover {
    background: var(--hover);
    color: var(--text);
  }
  .proj.on {
    background: transparent;
    box-shadow: inset 0 0 0 1px var(--line);
    color: var(--text);
  }
  .badge {
    flex: none;
    width: 20px;
    height: 20px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-sm);
    background: hsl(var(--badge-h, 210) 40% 36%);
    color: var(--text);
    font-size: var(--fs-xs);
    font-weight: 600;
  }
  .proj .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .proj .count {
    flex: none;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }
  .rail-foot {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    padding: calc(var(--space) * 2) var(--space);
    border-top: 1px solid var(--line);
  }
  .foot {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    min-height: 32px;
    padding: 0 calc(var(--space) * 2);
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-sm);
    text-align: left;
    cursor: pointer;
  }
  .foot:hover {
    background: var(--hover);
    color: var(--text);
  }
  .main {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .top {
    flex: none;
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    min-height: 36px;
    padding: calc(var(--space) * 4);
  }
  .top .page-search {
    flex: 1;
    height: calc(var(--space) * 8);
  }
  .new {
    flex: none;
    height: calc(var(--space) * 8);
  }
  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: var(--space);
    padding: 0 calc(var(--space) * 4) calc(var(--space) * 4);
  }
  h3 {
    margin: calc(var(--space) * 3) 0 var(--space);
    font-size: var(--fs-md);
    font-weight: 500;
    color: var(--muted);
  }
  h3:first-child {
    margin-top: var(--space);
  }
  .row {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    border-radius: var(--radius);
    min-height: 36px;
  }
  .row:hover {
    background: var(--hover);
  }
  .open {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 3);
    min-height: 36px;
    padding: 0 var(--space) 0 calc(var(--space) * 2);
    background: none;
    border: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-md);
    font-weight: 600;
    color: var(--text);
  }
  .tag {
    flex: none;
    font-size: var(--fs-sm);
    color: var(--muted);
    white-space: nowrap;
  }
  .x {
    width: 28px;
    height: 28px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
    opacity: 0;
  }
  .row:hover .x,
  .row:focus-within .x {
    opacity: 1;
  }
  .x:hover {
    background: var(--selected);
    color: var(--bad);
  }
  .empty {
    padding: calc(var(--space) * 8) calc(var(--space) * 4);
    text-align: center;
    font-size: var(--fs-md);
    color: var(--muted);
  }
</style>
