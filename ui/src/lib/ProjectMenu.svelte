<script lang="ts">
  import { DUR } from "./motion";
  import { createEventDispatcher, onMount } from "svelte";
  import { fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import Icon from "./Icon.svelte";
  import { api, brain, type Project } from "./api";
  import { remote as server } from "./remote";
  import { folderName } from "./tabs";
  import { sameDir } from "./paths";
  import { popover, placeAbove } from "./popover";
  import { tooltip } from "./tooltip";

  /** The project picker: the composer's "Let's build in" menu, and the
   *  Environment card's. Picking a project or a folder says where to work;
   *  the caller decides what that moves. */
  export let anchor: HTMLElement | null = null;
  export let folder = "";
  export let project: { slug: string; title: string } | null = null;
  export let branch = "";
  export let remote = false;
  export let remoteHost = "";
  export let remoteCwd = "";
  /** Offer "No project" (a new session can start without one). */
  export let none = true;

  const dispatch = createEventDispatcher<{
    close: void;
    error: { text: string };
    folder: { path: string };
    project: { path?: string; slug?: string };
    remoteFolder: { path: string };
  }>();

  let style = placeAbove(anchor, 260);
  let projects: Project[] = [];
  let projErr = "";
  let delSlug: string | null = null;

  onMount(async () => {
    try {
      projects = remote ? await server.projects() : await brain.projects();
    } catch (e) {
      projects = [];
      projErr = String(e);
    }
  });

  const close = () => dispatch("close");
  const fail = (e: unknown) => dispatch("error", { text: String(e) });

  async function chooseProject(p: Project | null) {
    close();
    try {
      if (remote) {
        let path = p?.folder ?? "";
        if (p && !path) {
          const typed = window.prompt(`Folder for ${p.title} on ${remoteHost}`, "/home/");
          if (!typed?.trim()) return;
          path = (await server.projectFolder(p.slug, p.title, typed.trim())).folder;
        }
        dispatch("remoteFolder", { path });
        return;
      }
      let path = p?.folder ?? "";
      if (p && !path) {
        const picked = await api.pickFolder(folder);
        if (!picked) return;
        path = (await brain.upsertProject(p.title, picked, p.slug)).folder;
      }
      dispatch("folder", { path });
      dispatch("project", { path, slug: p?.slug });
    } catch (e) {
      fail(e);
    }
  }

  async function newRemoteProject() {
    close();
    const typed = window.prompt(`Folder on ${remoteHost} for the new project`, "/home/");
    if (!typed?.trim()) return;
    try {
      const p = await server.projectFolder(null, folderName(typed.trim()), typed.trim());
      dispatch("remoteFolder", { path: p.folder });
    } catch (e) {
      fail(e);
    }
  }

  async function adopt(dir: string) {
    const known = projects.find((p) => sameDir(p.folder, dir));
    const slug = known ? known.slug : (await brain.upsertProject(folderName(dir), dir)).slug;
    if (!sameDir(dir, folder)) dispatch("folder", { path: dir });
    dispatch("project", { path: dir, slug });
  }

  async function newProject() {
    close();
    try {
      const picked = await api.pickFolder(folder);
      if (picked) await adopt(picked);
    } catch (e) {
      fail(e);
    }
  }

  async function makeProject() {
    close();
    try {
      await adopt(folder);
    } catch (e) {
      fail(e);
    }
  }

  async function deleteProject(p: Project) {
    if (delSlug !== p.slug) {
      delSlug = p.slug;
      return;
    }
    delSlug = null;
    try {
      await brain.remove(p.note);
      projects = await brain.projects();
      if (project?.slug === p.slug) dispatch("project", {});
    } catch (e) {
      fail(e);
    }
  }
</script>

<div class="menu-pop" data-ui-list {style} use:popover={{ anchor, close }} transition:fly={{ y: style.includes("bottom:") ? 6 : -6, duration: DUR, easing: cubicOut }}>
  {#each projects as p (p.slug)}
    <div class="opt-row" class:on={remote ? !!p.folder && p.folder === remoteCwd : project?.slug === p.slug}>
      <button class="opt grow" on:click={() => chooseProject(p)}>
        <Icon name="project" />
        <span class="name truncate">{p.title}</span>
        {#if !p.folder}<span class="count">No folder yet</span>{:else if !remote && sameDir(p.folder, folder) && branch}<span class="count">{branch}</span>{/if}
      </button>
      {#if !remote}
        <button
          class="del"
          class:armed={delSlug === p.slug}
          use:tooltip={delSlug === p.slug ? "Click again to delete" : `Delete ${p.title}`}
          aria-label={delSlug === p.slug ? `Confirm delete ${p.title}` : `Delete ${p.title}`}
          on:click={() => deleteProject(p)}
        >{#if delSlug === p.slug}Sure?{:else}<Icon name="trash" size={12} />{/if}</button>
      {/if}
    </div>
  {:else}
    {#if projErr}
      <p class="pop-empty err">Couldn't load projects: {projErr}</p>
    {:else}
      <p class="pop-empty">No projects yet.</p>
    {/if}
  {/each}
  <div class="sep" />
  {#if remote}
    <button class="opt" on:click={newRemoteProject}><Icon name="plus" /><span class="name">New project…</span></button>
  {:else}
    {#if folder && !project}
      <button class="opt" on:click={makeProject}><Icon name="plus" /><span class="name truncate">Make {folderName(folder)} a project</span></button>
    {/if}
    <button class="opt" on:click={newProject}><Icon name="plus" /><span class="name">New project…</span></button>
  {/if}
  {#if none}
    <button class="opt" class:on={remote ? !remoteCwd : !folder} on:click={() => chooseProject(null)}><Icon name="close" /><span class="name">No project</span></button>
  {/if}
</div>

<style>
  .truncate {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .menu-pop {
    width: 260px;
    padding: var(--space);
    overflow-y: auto;
  }
  .pop-empty {
    margin: 0;
    padding: calc(var(--space) * 2);
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .pop-empty.err {
    color: var(--bad-text);
  }
  .sep {
    height: 1px;
    margin: var(--space) calc(var(--space) * 2);
    background: var(--line);
  }
  .count {
    flex: none;
    font-size: var(--fs-xs);
    color: var(--muted);
  }
  .opt {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    width: 100%;
    height: var(--h-field);
    padding: 0 calc(var(--space) * 2);
    background: transparent;
    border: none;
    border-radius: var(--radius-in);
    color: var(--text);
    text-align: left;
    cursor: pointer;
  }
  .opt:hover:not(.on) {
    background: var(--hover);
  }
  .opt.on {
    background: var(--selected);
  }
  .opt.on :global(svg) {
    color: var(--accent);
  }
  .opt-row {
    display: flex;
    align-items: center;
    border-radius: var(--radius-in);
  }
  .opt-row:hover:not(.on) {
    background: var(--hover);
  }
  .opt-row.on {
    background: var(--selected);
  }
  .opt-row .opt.grow {
    flex: 1;
    min-width: 0;
    width: auto;
  }
  .opt-row:hover:not(.on) .opt.grow,
  .opt-row.on .opt.grow {
    background: transparent;
  }
  .opt-row .del {
    flex: none;
    min-width: 20px;
    height: 20px;
    margin-right: var(--space);
    padding: 0 var(--space);
    display: none;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--fs-xs);
    cursor: pointer;
  }
  .opt-row:hover .del,
  .opt-row .del:focus-visible,
  .opt-row .del.armed {
    display: inline-flex;
  }
  .opt-row .del:hover {
    color: var(--text);
  }
  .opt-row .del.armed {
    color: var(--bad-text);
    font-weight: 600;
  }
  .name {
    flex: 1;
    min-width: 0;
    font-size: var(--fs-md);
  }
</style>
