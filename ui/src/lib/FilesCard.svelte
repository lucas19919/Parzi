<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import InfoCard from "./InfoCard.svelte";
  import Icon from "./Icon.svelte";
  import { api } from "./api";
  import { fileDoc, imageDoc, isImagePath } from "./FileReader.svelte";
  import { toast, toastError } from "./toast";
  import { tooltip } from "./tooltip";
  import type { ArtifactDoc } from "./tabs";

  /** The session's files, in the side bar: a search and the folder as a
   *  tree (what changed lives in the Changes card). A file opens beside the
   *  cards, in the side bar's second column (FileReader). */
  export let folder = "";
  /** The side bar is open: lists only then. */
  export let shown = true;
  /** The file open beside the cards, marked in the list. */
  export let current = "";

  const dispatch = createEventDispatcher<{ open: ArtifactDoc }>();

  let picker: HTMLInputElement | null = null;

  // The project as a tree, and a search.
  let query = "";
  let files: string[] = [];
  // The folder as a tree: what each opened folder holds, and which are open.
  type Entry = { name: string; dir: boolean };
  let kids: Record<string, Entry[]> = {};
  let open = new Set<string>();
  let treeOf = "";
  let listed = "";
  let listing: ReturnType<typeof setTimeout> | null = null;

  const sep = (base: string) => (base.includes("\\") ? "\\" : "/");
  const join = (base: string, rel: string) => `${base.replace(/[\\/]+$/, "")}${sep(base)}${rel.replace(/\//g, sep(base))}`;
  const nameOf = (rel: string) => rel.split(/[\\/]/).pop() || rel;
  const dirOf = (rel: string) => rel.split(/[\\/]/).slice(0, -1).join("/");
  const norm = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();

  async function load(dir: string) {
    const entries = await api.listDir(dir).catch(() => [] as Entry[]);
    kids = { ...kids, [dir]: entries };
  }

  async function toggle(dir: string) {
    if (open.has(dir)) open.delete(dir);
    else {
      open.add(dir);
      if (!kids[dir]) await load(dir);
    }
    open = open;
  }

  // Rows as shown: each open folder's entries under it, indented.
  type Row = { path: string; name: string; dir: boolean; depth: number };
  function rowsOf(have: Record<string, Entry[]>, shown: Set<string>, dir: string, depth: number): Row[] {
    return (have[dir] ?? []).flatMap((e) => {
      const path = join(dir, e.name);
      const row = { path, name: e.name, dir: e.dir, depth };
      return e.dir && shown.has(path) ? [row, ...rowsOf(have, shown, path, depth + 1)] : [row];
    });
  }
  $: tree = folder ? rowsOf(kids, open, folder, 0) : [];

  async function list() {
    const at = folder;
    if (!at) {
      files = [];
      return;
    }
    if (treeOf !== at) {
      treeOf = at;
      kids = {};
      open = new Set();
    }
    const q = query.trim();
    const [all] = await Promise.all([
      q ? api.listFiles(at, q).catch(() => [] as string[]) : Promise.resolve([] as string[]),
      load(at),
    ]);
    if (at !== folder) return;
    files = all;
  }

  // Lists again when the folder or the search changes, and when the side
  // bar opens.
  $: wanted = shown ? `${folder}\n${query}` : "";
  $: if (wanted && wanted !== listed) {
    listed = wanted;
    if (listing) clearTimeout(listing);
    listing = setTimeout(() => void list(), query ? 150 : 0);
  }
  $: if (!wanted) listed = "";

  async function openPath(path: string) {
    if (isImagePath(path)) {
      showDoc(imageDoc(path));
      return;
    }
    try {
      show(path, await api.readTextFile(path));
    } catch {
      // Not text (a PDF): open it the system's way.
      await api.openFilePath(path).catch(toastError);
    }
  }

  const show = (id: string, content: string) => dispatch("open", fileDoc(id, content));
  const showDoc = (doc: ArtifactDoc) => dispatch("open", doc);

  async function browse() {
    // The dev mock has no system dialog, so it uses the browser's picker.
    if (location.pathname.endsWith("mock.html")) return picker?.click();
    try {
      const picked = await api.pickTextFile(folder || undefined);
      if (picked) show(picked.path, picked.content);
    } catch (err) {
      toast(`Couldn't open that file: ${err}`, true);
    }
  }

  async function onPick(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    if (file.size > 1_000_000) return toast("That file is too large to open here", true);
    try {
      show(file.name, await file.text());
    } catch (err) {
      toast(`Couldn't read that file: ${err}`, true);
    }
  }

</script>

<InfoCard id="files" title={folder ? nameOf(folder) : "Files"}>
  <svelte:fragment slot="actions">
    <button class="icon-btn" aria-label="Open a file from anywhere" use:tooltip={"Open a file from anywhere"} on:click={browse}><Icon name="folder" /></button>
  </svelte:fragment>
  {#if folder}
    <label class="find">
      <Icon name="search" size={12} />
      <input bind:value={query} placeholder="Find a file" aria-label="Find a file in {nameOf(folder)}" spellcheck="false" autocomplete="off" />
    </label>
    {#if query.trim()}
      {#if files.length}
        {#each files as rel (rel)}
          <button class="info-line row" class:on={norm(join(folder, rel)) === norm(current)} use:tooltip={rel} on:click={() => openPath(join(folder, rel))}>
            <Icon name="file" size={12} />
            <span class="info-v">{nameOf(rel)}</span>
            {#if dirOf(rel)}<span class="dir">{dirOf(rel)}</span>{/if}
          </button>
        {/each}
      {:else}
        <div class="info-line quiet">Nothing matches "{query.trim()}"</div>
      {/if}
    {:else}
      {#each tree as r (r.path)}
        <button
          class="info-line row node"
          style:--depth={r.depth}
          aria-expanded={r.dir ? open.has(r.path) : undefined}
          on:click={() => (r.dir ? toggle(r.path) : openPath(r.path))}
        >
          {#if r.dir}
            <span class="chev" class:down={open.has(r.path)}><Icon name="chevRight" size={12} /></span>
            <Icon name="folder" size={12} />
          {:else}
            <span class="chev" />
            <Icon name="file" size={12} />
          {/if}
          <span class="info-v">{r.name}</span>
        </button>
      {/each}
    {/if}
  {:else}
    <button class="info-line row" on:click={browse}>
      <Icon name="file" size={12} />
      <span class="info-v">Open a file</span>
    </button>
  {/if}
</InfoCard>
<input class="pick" bind:this={picker} type="file" tabindex="-1" aria-hidden="true" on:change={onPick} />

<style>
  .pick {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: 0;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }
  /* The file open beside the cards. */
  .on {
    background: var(--selected);
  }
  /* Search, quiet until focused, the same height as a line. */
  .find {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    height: var(--h);
    margin-bottom: var(--space);
    padding: 0 calc(var(--space) * 2);
    background: var(--hover);
    border-radius: var(--radius);
    color: var(--muted);
  }
  .find:focus-within {
    background: var(--selected);
    color: var(--text);
  }
  .find input {
    flex: 1;
    min-width: 0;
    padding: 0;
    background: none;
    border: none;
    outline: none;
    box-shadow: none !important;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
  }
  /* A tree row: indented by its depth, a chevron before folders. */
  .node {
    padding-left: calc(var(--space) * 2 + var(--depth, 0) * var(--space) * 4);
  }
  .chev {
    display: inline-flex;
    flex: none;
    width: 12px;
    color: var(--muted);
    transition: transform var(--dur) ease;
  }
  .chev.down {
    transform: rotate(90deg);
  }
  /* The folder a file is in, after its name. */
  .dir {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    color: var(--muted);
    font-size: var(--fs-xs);
  }
</style>
