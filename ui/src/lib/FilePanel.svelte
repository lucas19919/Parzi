<script lang="ts" context="module">
  import type { ArtifactDoc } from "./tabs";

  export const FILE_MIN = 220;
  export const FILE_MAX = 640;
  const MD = /\.(md|markdown)$/i;
  const ABSOLUTE = /^([a-zA-Z]:[\\/]|\/|\\\\)/;

  /** A file read from disk as a card document: markdown renders, the rest stays plain. */
  export function fileDoc(id: string, content: string): ArtifactDoc {
    const name = id.split(/[\\/]/).pop() || id;
    const md = MD.test(name);
    return { id, title: md ? name.replace(MD, "") || name : name, kind: md ? "markdown" : "text", content, version: 1 };
  }
</script>

<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import InfoCard, { unfold } from "./InfoCard.svelte";
  import Icon from "./Icon.svelte";
  import ResizeGrip from "./ResizeGrip.svelte";
  import { api, brain } from "./api";
  import { saveFile } from "./saveFile";
  import { codeBlockClick, mdHtml, richReady } from "./md";
  import { handleLinkClick } from "./links";
  import { toast, toastError } from "./toast";
  import { tooltip } from "./tooltip";

  export let doc: ArtifactDoc | null = null;
  export let folder = "";
  export let width = 420;
  export let shown = true;

  const dispatch = createEventDispatcher<{
    close: void;
    pick: ArtifactDoc;
  }>();

  let copied = false;
  let picker: HTMLInputElement | null = null;

  // ── Browsing the project when no file is open ──
  let query = "";
  let files: string[] = [];
  // The folder as a tree: what each opened folder holds, and which are open.
  type Entry = { name: string; dir: boolean };
  let kids: Record<string, Entry[]> = {};
  let open = new Set<string>();
  let treeOf = "";
  let changed: { path: string; label: string }[] = [];
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
      changed = [];
      return;
    }
    if (treeOf !== at) {
      treeOf = at;
      kids = {};
      open = new Set();
    }
    const q = query.trim();
    const [all, git] = await Promise.all([
      q ? api.listFiles(at, q).catch(() => [] as string[]) : Promise.resolve([] as string[]),
      api.gitStatus(at).catch(() => null),
      load(at),
    ]);
    if (at !== folder) return;
    files = all;
    // Changed files show relative to the folder when they are inside it.
    const top = git?.top || at;
    const inside = norm(at).startsWith(norm(top)) ? norm(at).slice(norm(top).length).replace(/^\//, "") : "";
    const ql = q.toLowerCase();
    changed = (git?.files ?? [])
      .map((rel) => {
        const path = join(top, rel);
        const r = rel.replace(/\\/g, "/");
        const label = inside && r.toLowerCase().startsWith(`${inside}/`) ? r.slice(inside.length + 1) : r;
        return { path, label };
      })
      .filter((f) => !ql || f.label.toLowerCase().includes(ql));
  }

  // Lists again when the folder or the search changes, and when the card
  // comes back to the list.
  $: wanted = shown && !doc ? `${folder}\n${query}` : "";
  $: if (wanted && wanted !== listed) {
    listed = wanted;
    if (listing) clearTimeout(listing);
    listing = setTimeout(() => void list(), query ? 150 : 0);
  }
  $: if (!wanted) listed = "";

  async function openPath(path: string) {
    try {
      show(path, await api.readTextFile(path));
    } catch {
      // Not text (an image, a PDF): open it the system's way.
      await api.openFilePath(path).catch(toastError);
    }
  }

  $: markdown = !!doc && (doc.kind === "markdown" || doc.kind === "md");
  // A file's id is its absolute path, a note's id its vault path.
  $: revealable = !!doc && /[\\/]/.test(doc.id);
  $: onDisk = !!doc && ABSOLUTE.test(doc.id);
  $: isNote = !!doc && !onDisk && MD.test(doc.id);
  // Relative links resolve beside the open file or note, not at the project root.
  $: linkFolder = onDisk ? doc!.id.replace(/[\\/][^\\/]*$/, "") : folder;
  $: noteDir = isNote ? doc!.id.replace(/\\/g, "/").replace(/\/?[^/]*$/, "") : null;
  $: if (doc) unfold("reader");

  const show = (id: string, content: string) => dispatch("pick", fileDoc(id, content));

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

  function copy() {
    if (!doc) return;
    navigator.clipboard.writeText(doc.content).then(
      () => {
        copied = true;
        setTimeout(() => (copied = false), 1200);
      },
      (e) => toast(`Couldn't copy: ${e}`, true),
    );
  }

  function save() {
    if (!doc) return;
    // Markdown titles drop ".md"; other files keep their own name.
    void saveFile(markdown ? `${doc.title || "note"}.md` : doc.title || "file.txt", doc.content);
  }

  function reveal() {
    if (!doc) return;
    if (onDisk) void api.openFilePath(linkFolder || doc.id).catch(toastError);
    else void brain.open(doc.id, "folder").catch(toastError);
  }

  function onDocClick(e: MouseEvent) {
    void (async () => {
      if (await handleLinkClick(e, linkFolder, noteDir)) return;
      codeBlockClick(e.target as HTMLElement);
    })();
  }
</script>

<aside class="files" class:shut={!shown} class:reading={!!doc} aria-label="Files" style="width: {shown ? width : 0}px" inert={!shown}>
  <ResizeGrip label="Resize files" min={FILE_MIN} max={FILE_MAX} on:resize />
  <div class="col">
  <InfoCard id="reader" title={doc?.title || (folder ? nameOf(folder) : "Files")}>
    <svelte:fragment slot="actions">
      {#if doc}
        <button class="icon-btn" aria-label="Back to files" use:tooltip={"Back to files"} on:click={() => dispatch("close")}><Icon name="arrowLeft" /></button>
        <button class="icon-btn" aria-label={copied ? "Copied" : "Copy"} use:tooltip={copied ? "Copied" : "Copy"} on:click={copy}><Icon name={copied ? "check" : "copy"} /></button>
        <button class="icon-btn" aria-label="Save as file" use:tooltip={"Save as file"} on:click={save}><Icon name="download" /></button>
        {#if revealable}
          <button class="icon-btn" aria-label="Open in file explorer" use:tooltip={"Open in file explorer"} on:click={reveal}><Icon name="folder" /></button>
        {/if}
      {:else}
        <button class="icon-btn" aria-label="Open a file from anywhere" use:tooltip={"Open a file from anywhere"} on:click={browse}><Icon name="folder" /></button>
      {/if}
    </svelte:fragment>
    {#if doc}
      <!-- svelte-ignore a11y-no-static-element-interactions a11y-click-events-have-key-events -->
      <div class="doc msg" on:click={onDocClick}>
        {#key `${doc.id}:${doc.version}:${$richReady}`}
          {#if markdown}
            {@html mdHtml(doc.content, $richReady)}
          {:else}
            <pre class="plain">{doc.content}</pre>
          {/if}
        {/key}
      </div>
    {:else if folder}
      <label class="find">
        <Icon name="search" size={12} />
        <input bind:value={query} placeholder="Find a file" aria-label="Find a file in {nameOf(folder)}" spellcheck="false" autocomplete="off" />
      </label>
      {#if changed.length}
        <div class="group">Changed</div>
        {#each changed as f (f.path)}
          <button class="info-line row" use:tooltip={f.label} on:click={() => openPath(f.path)}>
            <Icon name="file" size={12} />
            <span class="info-v">{nameOf(f.label)}</span>
            {#if dirOf(f.label)}<span class="dir">{dirOf(f.label)}</span>{/if}
          </button>
        {/each}
      {/if}
      {#if query.trim()}
        {#if files.length}
          {#if changed.length}<div class="group">Matches</div>{/if}
          {#each files as rel (rel)}
            <button class="info-line row" use:tooltip={rel} on:click={() => openPath(join(folder, rel))}>
              <Icon name="file" size={12} />
              <span class="info-v">{nameOf(rel)}</span>
              {#if dirOf(rel)}<span class="dir">{dirOf(rel)}</span>{/if}
            </button>
          {/each}
        {:else if !changed.length}
          <div class="info-line quiet">Nothing matches "{query.trim()}"</div>
        {/if}
      {:else}
        {#if changed.length}<div class="group">All files</div>{/if}
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
  </div>
  <input class="pick" bind:this={picker} type="file" tabindex="-1" aria-hidden="true" on:change={onPick} />
</aside>

<style>
  /* The column itself must not clip: the resize grip sits just left of it. */
  .files {
    position: relative;
    flex: none;
    align-self: stretch;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    margin: 0 var(--gap) 0 0;
    background: none;
    transition: width 300ms var(--ease-out), margin-right 300ms var(--ease-out);
  }
  .col {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    padding-bottom: calc(var(--space) * 3);
    overflow-x: hidden;
    overflow-y: auto;
    scrollbar-width: none;
  }
  .files.shut {
    margin-right: 0;
  }
  .files :global(.card) {
    min-height: 0;
    min-width: 0;
    max-width: 100%;
    display: flex;
    flex-direction: column;
  }
  .files.reading :global(.card.open) {
    flex: 1;
  }
  .files.reading :global(.card.open > .body) {
    flex: 1;
    min-width: 0;
    min-height: 0;
    max-width: 100%;
    overflow: auto;
  }
  .files :global(.bar .title) {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .files :global(.aside:empty) {
    display: none;
  }
  .doc {
    min-width: 0;
    max-width: 100%;
    overflow-wrap: break-word;
    word-break: break-word;
  }
  .doc :global(.codeblock),
  .doc :global(.md-table),
  .doc :global(pre) {
    max-width: 100%;
  }
  .plain {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font: inherit;
    font-size: var(--fs-md);
  }
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
  .group {
    margin-top: calc(var(--space) * 2);
    color: var(--muted);
    font-size: var(--fs-xs);
    font-weight: 600;
  }
  .find + .group {
    margin-top: 0;
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
