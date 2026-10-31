<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import { brain, type Project, type SessionMeta } from "./api";
  import { folderName } from "./tabs";
  import { byDay } from "./time";
  import { tooltip } from "./tooltip";
  import { toast } from "./toast";
  import { invoke } from "@tauri-apps/api/core";

  export let threads: SessionMeta[] = [];

  const dispatch = createEventDispatcher<{
    openSession: { id: string };
    deleteSession: { id: string };
    newSession: void;
  }>();

  let query = "";
  let field: HTMLInputElement | null = null;
  let projectFilter: string | null = null;
  let projects: Project[] = [];
  let projectsError = "";
  let adding = false;
  let newTitle = "";
  let newFolder = "";
  let saving = false;

  async function loadProjects() {
    projectsError = "";
    try {
      projects = (await brain.projects()) ?? [];
    } catch (e) {
      projects = [];
      projectsError = String(e);
      void invoke("ui_error", { message: `projects did not load: ${e}` }).catch(() => {});
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
    field?.focus();
    void loadProjects();
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

<!-- Same cards as Settings: shared.css styles rows under .settings. -->
<div class="manager settings">
  <aside class="rail" aria-label="Projects">
    <div class="rail-head">
      <span class="t-section">Projects</span>
      <button class="icon-btn" aria-label="Add project" use:tooltip={"Add project"} on:click={() => (adding = !adding)}>
        <Icon name="plus" />
      </button>
    </div>
    {#if adding}
      <div class="add-form">
        <input class="tin" bind:value={newTitle} placeholder="Project name" aria-label="Project name" spellcheck="false" />
        <input class="tin" bind:value={newFolder} placeholder="Folder (optional)" aria-label="Project folder" spellcheck="false" />
        <div class="add-acts">
          <button class="btn" on:click={() => (adding = false)}>Cancel</button>
          <button class="btn primary" disabled={saving || !newTitle.trim()} on:click={addProject}>{saving ? "Adding…" : "Add"}</button>
        </div>
      </div>
    {/if}
    <div class="proj-list">
      {#if projectsError}
        <div class="rail-note">
          <span class="t-hint" title={projectsError}>Couldn't load projects</span>
          <button class="btn" on:click={loadProjects}>Retry</button>
        </div>
      {:else}
        {#each projects as p (p.slug)}
          <button
            class="proj"
            class:on={projectFilter === p.slug}
            title={projectFilter === p.slug ? "Show all sessions" : `Show only ${p.title}`}
            on:click={() => toggleProject(p.slug)}
          >
            <span class="badge small" style="--badge-h: {hueOf(p.slug)}">{p.title.trim().charAt(0).toUpperCase()}</span>
            <span class="name t-label">{p.title}</span>
            {#if counts.get(p.slug)}<span class="t-hint">{counts.get(p.slug)}</span>{/if}
          </button>
        {:else}
          <p class="rail-note t-hint">No projects yet</p>
        {/each}
      {/if}
    </div>
  </aside>
  <div class="main">
    <div class="top page-bar"><span class="t-page">Sessions</span></div>
    <div class="body">
      <div class="column">
        <div class="tools">
          <div class="page-search">
            <Icon name="search" />
            <input bind:this={field} bind:value={query} placeholder="Search sessions" aria-label="Search sessions" spellcheck="false" />
          </div>
          <button class="btn new" on:click={() => dispatch("newSession")}><Icon name="pencil" size={12} /><span>New session</span></button>
        </div>
        <div class="days" data-ui-list>
          {#each days as day (day.label)}
            <div class="rows">
              <div class="row row-title"><span>{day.label}</span></div>
              {#each day.items as s (s.id)}
                {@const place = placeOf(s)}
                <div class="row sess">
                  <button class="open" title={s.title} on:click={() => dispatch("openSession", { id: s.id })}>
                    <span class="badge" style="--badge-h: {hueOf(s.project || place || s.id)}">{initialOf(s)}</span>
                    <span class="title t-label">{s.title || "Untitled session"}</span>
                    {#if place}<span class="t-hint">{place}</span>{/if}
                  </button>
                  <button class="icon-btn x" use:tooltip={"Delete session"} aria-label={`Delete ${s.title || "session"}`} on:click={() => dispatch("deleteSession", { id: s.id })}>
                    <Icon name="trash" size={12} />
                  </button>
                </div>
              {/each}
            </div>
          {:else}
            <p class="empty t-hint">{q || projectFilter ? "Nothing matches." : "No sessions yet. Start one above."}</p>
          {/each}
        </div>
      </div>
    </div>
  </div>
</div>

<style>
  .manager {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: row;
  }
  .rail {
    width: 220px;
    flex: none;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  /* The same band as the page bar beside it: 8px + 36px tab height + 8px. */
  .rail-head {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: calc(var(--space) * 13);
    padding: 0 calc(var(--space) * 2) 0 calc(var(--space) * 4);
  }
  .add-form {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    padding: 0 calc(var(--space) * 2) calc(var(--space) * 3) calc(var(--space) * 4);
  }
  .add-form .tin {
    width: 100%;
  }
  .add-acts {
    display: flex;
    justify-content: flex-end;
    gap: calc(var(--space) * 2);
  }
  .rail-note {
    margin: 0;
    padding: var(--space) calc(var(--space) * 2);
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
    gap: var(--space);
    padding: 0 calc(var(--space) * 2);
  }
  .proj {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    min-height: var(--h-row);
    padding: 0 calc(var(--space) * 2);
    background: none;
    border: none;
    border-radius: var(--radius);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .proj:hover {
    background: var(--hover);
  }
  .proj.on {
    background: var(--selected);
  }
  .badge {
    flex: none;
    width: 28px;
    height: 28px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-sm);
    background: hsl(var(--badge-h, 210) 40% 36%);
    color: var(--text);
    font-size: var(--fs-sm);
    font-weight: 600;
  }
  .badge.small {
    width: 20px;
    height: 20px;
    font-size: var(--fs-xs);
  }
  .proj .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .main {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .top {
    min-height: calc(var(--space) * 13);
  }
  /* The same column as Settings' content. */
  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
  .column {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 6);
    width: 100%;
    max-width: 840px;
    margin: 0 auto;
    padding: calc(var(--space) * 6) calc(var(--space) * 8) calc(var(--space) * 8);
  }
  .tools {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
  }
  .tools .page-search {
    flex: 1;
    height: calc(var(--space) * 8);
  }
  .new {
    flex: none;
    height: calc(var(--space) * 8);
    padding-block: 0;
  }
  .days {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 6);
  }
  .sess:hover {
    background: var(--hover);
  }
  /* The whole row opens the session, up to the delete button. */
  .open {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 3);
    align-self: stretch;
    margin: calc(var(--space) * -2) 0 calc(var(--space) * -2) calc(var(--space) * -4);
    padding: calc(var(--space) * 2) 0 calc(var(--space) * 2) calc(var(--space) * 4);
    background: none;
    border: none;
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
  }
  .x {
    opacity: 0;
  }
  .sess:hover .x,
  .sess:focus-within .x {
    opacity: 1;
  }
  .x:hover:not(:disabled) {
    color: var(--bad);
  }
  .empty {
    margin: 0;
    padding: calc(var(--space) * 8) calc(var(--space) * 4);
    text-align: center;
  }
</style>
