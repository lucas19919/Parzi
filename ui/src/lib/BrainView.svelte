<script lang="ts">
  import { onMount, tick } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import Icon from "./Icon.svelte";
  import { api, brain, EVERYWHERE, type NoteMeta, type ObsidianState, type Project } from "./api";
  import { renderMarkdown } from "./md";
  import { folderName } from "./tabs";
  import { toast, toastError } from "./toast";

  const UNUSED = "unused";
  const OPEN_KEY = "parzi.brain.open.v2";
  const DRAG_START = 5;

  type Folder = { key: string; title: string; project: Project | null; items: NoteMeta[] };

  let notes: NoteMeta[] = [];
  let projects: Project[] = [];
  let folder = EVERYWHERE;
  let selected = "";
  let raw = "";
  let editing = false;
  let dirty = false;
  let saveTimer = 0;
  let query = "";
  let creatingIn = "";
  let draftName = "";
  let draftField: HTMLInputElement | null = null;
  let obsidian: ObsidianState = "missing";
  let expanded = new Set<string>(loadExpanded());
  let press: { path: string; from: string; title: string; x: number; y: number } | null = null;
  let dragging = false;
  let dragX = 0;
  let dragY = 0;
  let dropOn = "";

  $: byPath = new Map(notes.map((n) => [n.path, n]));
  $: folders = buildFolders(notes, projects, query);
  $: current = folders.find((f) => f.key === folder) ?? null;
  $: note = byPath.get(selected) ?? null;
  $: noteProject = projects.find((p) => p.note === selected) ?? null;

  function buildFolders(list: NoteMeta[], all: Project[], q: string): Folder[] {
    const map = new Map(list.map((n) => [n.path, n]));
    const used = new Set<string>();
    const pick = (paths: string[]) => paths.map((p) => map.get(p)).filter((n): n is NoteMeta => !!n);
    const everywhere = list.filter(inEverywhere);
    everywhere.forEach((n) => used.add(n.path));
    const projectFolders = [...all]
      .sort((a, b) => a.title.localeCompare(b.title))
      .map((p) => {
        p.notes.forEach((path) => used.add(path));
        return { key: p.slug, title: p.title, project: p, items: pick(p.notes) };
      });
    const unused = list.filter((n) => !isProjectNote(n) && !used.has(n.path));
    const out: Folder[] = [
      { key: EVERYWHERE, title: "All sessions", project: null, items: everywhere },
      ...projectFolders,
      { key: UNUSED, title: "Not used", project: null, items: unused },
    ];
    const term = q.trim().toLowerCase();
    return out.map((f) => ({
      ...f,
      items: f.items.filter((n) => !term || n.title.toLowerCase().includes(term) || f.title.toLowerCase().includes(term)).sort(byTitle),
    }));
  }

  function inEverywhere(n: NoteMeta) {
    return n.projects.some((p) => p.toLowerCase() === EVERYWHERE);
  }

  function isProjectNote(n: NoteMeta) {
    return n.path.startsWith("projects/");
  }

  function byTitle(a: NoteMeta, b: NoteMeta) {
    return Number(b.pinned) - Number(a.pinned) || a.title.localeCompare(b.title);
  }

  function twin(n: NoteMeta, f: Folder) {
    if (!f.items.some((x) => x !== n && x.title === n.title)) return "";
    return /^imported\/([^/]+)\//.exec(n.path)?.[1] ?? "";
  }

  function loadExpanded(): string[] {
    try {
      const v = JSON.parse(localStorage.getItem(OPEN_KEY) ?? `["${EVERYWHERE}"]`);
      return Array.isArray(v) ? v.filter((x) => typeof x === "string") : [];
    } catch {
      return [EVERYWHERE];
    }
  }

  function toggle(key: string, open?: boolean) {
    const next = new Set(expanded);
    if (open ?? !next.has(key)) next.add(key);
    else next.delete(key);
    expanded = next;
    try {
      localStorage.setItem(OPEN_KEY, JSON.stringify([...next]));
    } catch {}
  }

  async function refresh() {
    try {
      [notes, projects] = await Promise.all([brain.list(), brain.projects()]);
    } catch (e) {
      toastError(e);
    }
  }

  async function onFocus() {
    await refresh();
    if (selected && !dirty) raw = await brain.read(selected).catch(() => raw);
  }

  function onVisible() {
    if (document.visibilityState === "visible") void onFocus();
  }

  function isCurrent(f: Folder, sel: string, at: string) {
    return f.project ? sel === f.project.note : at === f.key && !sel;
  }

  async function openFolder(f: Folder) {
    toggle(f.key, !(expanded.has(f.key) && isCurrent(f, selected, folder)));
    folder = f.key;
    if (f.project && byPath.has(f.project.note)) {
      await openNote(f.project.note);
      return;
    }
    await flush();
    selected = "";
  }

  async function openNote(path: string) {
    await flush();
    try {
      raw = await brain.read(path);
      selected = path;
      editing = false;
      dirty = false;
    } catch (e) {
      toastError(e);
    }
  }

  function onEdit() {
    dirty = true;
    clearTimeout(saveTimer);
    saveTimer = window.setTimeout(() => void flush(), 600);
  }

  async function flush() {
    clearTimeout(saveTimer);
    if (!dirty || !selected) return;
    dirty = false;
    try {
      const meta = await brain.write(selected, raw);
      notes = notes.map((n) => (n.path === meta.path ? meta : n));
      projects = await brain.projects();
    } catch (e) {
      toastError(e);
    }
  }

  async function togglePin() {
    if (!note) return;
    await flush();
    try {
      const meta = await brain.pin(note.path, !note.pinned);
      notes = notes.map((x) => (x.path === meta.path ? meta : x));
      raw = await brain.read(meta.path);
    } catch (e) {
      toastError(e);
    }
  }

  function viaLink(n: NoteMeta, key: string) {
    const p = projects.find((x) => x.slug === key);
    return !!p && n.links.includes(p.note) && !n.projects.some((s) => s.toLowerCase() === key.toLowerCase());
  }

  async function moveNote(path: string, from: string, to: string, copy: boolean) {
    const n = byPath.get(path);
    if (!n || from === to) return;
    if (!copy && from !== UNUSED && viaLink(n, from)) {
      toast("This note links to that project, so it stays there. Remove the [[link]] to move it.");
      return;
    }
    await flush();
    try {
      if (to !== UNUSED) await brain.map(path, to, true);
      if (!copy && from !== UNUSED) await brain.map(path, from, false);
      await refresh();
      if (selected === path) raw = await brain.read(path);
      if (to !== UNUSED) toggle(to, true);
    } catch (e) {
      toastError(e);
    }
  }

  function onRowDown(e: PointerEvent, n: NoteMeta, from: string) {
    if (e.button !== 0) return;
    press = { path: n.path, from, title: n.title, x: e.clientX, y: e.clientY };
  }

  function onMove(e: PointerEvent) {
    if (!press) return;
    if (!dragging && Math.hypot(e.clientX - press.x, e.clientY - press.y) < DRAG_START) return;
    dragging = true;
    dragX = e.clientX;
    dragY = e.clientY;
    const hit = document.elementFromPoint(e.clientX, e.clientY)?.closest<HTMLElement>("[data-folder]");
    dropOn = hit?.dataset.folder ?? "";
  }

  function onUp(e: PointerEvent) {
    if (!press) return;
    const p = press;
    const wasDrag = dragging;
    const target = dropOn;
    press = null;
    dragging = false;
    dropOn = "";
    if (!wasDrag) {
      void openNote(p.path);
      return;
    }
    if (target && target !== p.from) void moveNote(p.path, p.from, target, e.ctrlKey);
  }

  async function startNote(key: string) {
    creatingIn = key;
    draftName = "";
    toggle(key, true);
    await tick();
    draftField?.focus();
  }

  async function createNote() {
    const name = draftName.trim().replace(/[\\/:*?"<>|]/g, "-");
    const key = creatingIn;
    creatingIn = "";
    if (!name) return;
    const path = `notes/${name}.md`;
    const head = key && key !== UNUSED ? `---\nprojects: [${key}]\n---\n\n` : "";
    try {
      await brain.write(path, `${head}# ${name}\n\n`);
      await refresh();
      await openNote(path);
      editing = true;
    } catch (e) {
      toastError(e);
    }
  }

  async function newProject() {
    try {
      const dir = await api.pickFolder();
      if (!dir) return;
      const known = projects.find((p) => p.folder.toLowerCase() === dir.toLowerCase());
      const p = known ?? (await brain.upsertProject(folderName(dir), dir));
      await refresh();
      folder = p.slug;
      toggle(p.slug, true);
      await openNote(p.note);
    } catch (e) {
      toastError(e);
    }
  }

  async function removeProject(p: Project) {
    const ok = await ask(`Remove the project "${p.title}"? Its notes stay in the brain.`, { title: "Remove project", kind: "warning" }).catch(() => false);
    if (!ok) return;
    try {
      await brain.remove(p.note);
      await refresh();
      folder = EVERYWHERE;
      selected = "";
      raw = "";
      dirty = false;
    } catch (e) {
      toastError(e);
    }
  }

  async function removeNote() {
    if (!note) return;
    if (noteProject) {
      await removeProject(noteProject);
      return;
    }
    const ok = await ask(`Delete "${note.title}"?`, { title: "Delete note", kind: "warning" }).catch(() => false);
    if (!ok) return;
    try {
      await brain.remove(note.path);
      selected = "";
      raw = "";
      dirty = false;
      await refresh();
    } catch (e) {
      toastError(e);
    }
  }

  async function openOutside(path: string, target: "file" | "folder" | "obsidian") {
    if (target === "obsidian" && obsidian === "unregistered") {
      toast("In Obsidian, choose Open folder as vault and pick the brain folder once. After that this opens the note directly.");
      target = "folder";
      path = "";
    }
    await flush();
    brain.open(path, target).catch(toastError);
  }

  function resolve(target: string): string | null {
    const t = target.toLowerCase().replace(/\.md$/, "");
    const exact = notes.find((n) => n.path.toLowerCase().replace(/\.md$/, "") === t);
    if (exact) return exact.path;
    const stem = notes.filter((n) => (n.path.split("/").pop() ?? "").toLowerCase().replace(/\.md$/, "") === t);
    return stem.length === 1 ? stem[0].path : null;
  }

  function body(text: string) {
    return text.replace(/^---\r?\n[\s\S]*?\r?\n---\r?\n?/, "");
  }

  function withLinks(text: string) {
    return text.replace(/\[\[([^\]|#]+)(?:#[^\]|]*)?(?:\|([^\]]+))?\]\]/g, (_m, target: string, alias?: string) => {
      const label = (alias ?? target).trim();
      return `[${label}](parzi:note/${encodeURIComponent(target.trim())})`;
    });
  }

  function onPreviewClick(e: MouseEvent) {
    const a = (e.target as HTMLElement).closest("a");
    if (!a) return;
    e.preventDefault();
    const href = a.getAttribute("href") ?? "";
    if (href.startsWith("parzi:note/")) {
      const path = resolve(decodeURIComponent(href.slice("parzi:note/".length)));
      if (path) void openNote(path);
      else toast("That note doesn't exist yet");
    }
  }

  onMount(() => {
    void refresh();
    brain.obsidian().then((s) => (obsidian = s)).catch(() => {});
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onVisible);
      void flush();
    };
  });
</script>

<svelte:window on:pointermove={onMove} on:pointerup={onUp} on:pointercancel={() => ((press = null), (dragging = false), (dropOn = ""))} />

<div class="brain" class:dragging>
  <aside>
    <div class="top">
      <div class="search">
        <Icon name="search" size={13} />
        <input bind:value={query} placeholder="Search" spellcheck="false" />
      </div>
      <button class="icon" title="Open the brain folder" on:click={() => openOutside("", "folder")}><Icon name="folder" size={14} /></button>
    </div>

    <div class="tree">
      {#each folders as f (f.key)}
        {@const open = !!query.trim() || expanded.has(f.key)}
        {#if f.key !== UNUSED || f.items.length || !query.trim()}
          <div class="folder" class:on={isCurrent(f, selected, folder)} class:drop={dragging && dropOn === f.key} data-folder={f.key}>
            <button class="folder-name" title={f.project?.folder ?? ""} aria-expanded={open} on:click={() => openFolder(f)}>
              <span class="chev"><Icon name={open ? "chevDown" : "chevRight"} size={11} stroke={2} /></span>
              <Icon name={f.key === EVERYWHERE ? "globe" : "folder"} size={14} />
              <span class="name">{f.title}</span>
            </button>
            {#if f.key !== UNUSED}
              <button class="add" title={`New note in ${f.title}`} on:click={() => startNote(f.key)}><Icon name="plus" size={12} /></button>
            {/if}
          </div>
          {#if open}
            <div class="kids" data-folder={f.key}>
              {#each f.items as n (n.path)}
                <button
                  class="note"
                  class:on={n.path === selected}
                  title={n.summary || n.path}
                  on:pointerdown={(e) => onRowDown(e, n, f.key)}
                  on:keydown={(e) => e.key === "Enter" && openNote(n.path)}
                >
                  <span class="name">{n.title}</span>
                  {#if twin(n, f)}<span class="from">{twin(n, f)}</span>{/if}
                  {#if n.pinned}<span class="pin" title="Always included in full"><Icon name="pin" size={11} /></span>{/if}
                </button>
              {/each}
              {#if creatingIn === f.key}
                <form class="draft" on:submit|preventDefault={createNote}>
                  <input bind:this={draftField} bind:value={draftName} placeholder="Note name" on:blur={createNote} />
                </form>
              {:else if !f.items.length}
                <p class="empty">{f.key === UNUSED ? "Nothing here." : "Empty. Drag notes here."}</p>
              {/if}
            </div>
          {/if}
        {/if}
      {/each}
      <button class="new-project" on:click={newProject}><Icon name="plus" size={12} /> New project</button>
    </div>
  </aside>

  <section>
    {#if note}
      <div class="bar">
        <div class="titles">
          <h2>{note.title}</h2>
          <span class="path">{noteProject ? `Project · ${noteProject.folder}` : note.path}</span>
        </div>
        {#if !noteProject}
          <button
            class="icon"
            class:pinned={note.pinned}
            title={note.pinned ? "Pinned: the agent always gets the whole note. Click to unpin." : "Not pinned: the agent sees the title and opens the note when it needs it. Click to pin."}
            on:click={togglePin}
          >
            <Icon name="pin" size={14} />
          </button>
        {/if}
        <div class="seg">
          <button class:on={!editing} on:click={() => (editing = false)}>Read</button>
          <button class:on={editing} on:click={() => (editing = true)}>Edit</button>
        </div>
        {#if obsidian !== "missing"}
          <button class="icon" title="Open in Obsidian" on:click={() => openOutside(note?.path ?? "", "obsidian")}><Icon name="arrowRight" size={14} /></button>
        {/if}
        <button class="icon" title="Open the file" on:click={() => openOutside(note?.path ?? "", "file")}><Icon name="file" size={14} /></button>
        <button class="icon danger" title={noteProject ? "Remove project (its notes stay)" : "Delete note"} on:click={removeNote}><Icon name="trash" size={14} /></button>
      </div>
      <div class="content">
        {#if editing}
          <textarea bind:value={raw} on:input={onEdit} on:blur={flush} spellcheck="true" />
        {:else}
          <!-- svelte-ignore a11y-click-events-have-key-events a11y-no-static-element-interactions -->
          <div class="msg doc" on:click={onPreviewClick}>{@html renderMarkdown(withLinks(body(raw)), false)}</div>
        {/if}
      </div>
    {:else if current}
      <div class="page">
        <h2>{current.title}</h2>
        <p class="lead">
          {#if current.key === UNUSED}
            No session gets these notes. Drag one onto a folder to use it.
          {:else if current.project}
            Sessions started in <span class="mono">{current.project.folder}</span> get these notes.
          {:else}
            Every session gets these notes.
          {/if}
        </p>
        <p class="hint">Drag a note onto a folder to move it; hold Ctrl to copy it. Pin a note to always send all of it.</p>
      </div>
    {/if}
  </section>

  {#if dragging && press}
    <div class="ghost" style={`left:${dragX + 14}px;top:${dragY + 10}px`}>{press.title}</div>
  {/if}
</div>

<style>
  .brain {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
  }
  .brain.dragging,
  .brain.dragging * {
    cursor: grabbing !important;
    user-select: none;
  }
  aside {
    width: 270px;
    flex: none;
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--line);
  }
  .top {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 10px 8px 6px 10px;
  }
  .search {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 30px;
    padding: 0 10px;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius);
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
  .tree {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 4px 6px 16px;
  }
  .folder {
    display: flex;
    align-items: center;
    border: 1px solid transparent;
    border-radius: var(--radius);
  }
  .folder:hover,
  .folder.on {
    background: var(--line);
  }
  .folder.drop {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }
  .add {
    width: 22px;
    height: 28px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--faint);
    cursor: pointer;
  }
  .add:hover {
    color: var(--text);
  }
  .add {
    opacity: 0;
  }
  .folder:hover .add,
  .add:focus-visible {
    opacity: 1;
  }
  .folder-name {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 7px;
    height: 30px;
    padding: 0 4px 0 4px;
    background: none;
    border: none;
    color: var(--muted);
    font-size: 13px;
    text-align: left;
    cursor: pointer;
  }
  .folder-name :global(svg) {
    flex: none;
    color: var(--faint);
  }
  .folder:hover .folder-name,
  .folder.on .folder-name {
    color: var(--text);
  }
  .folder.on .folder-name :global(svg) {
    color: var(--accent);
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chev {
    width: 12px;
    display: inline-flex;
    justify-content: center;
  }
  .kids {
    margin: 1px 0 6px 15px;
    padding-left: 6px;
    border-left: 1px solid var(--line);
  }
  .note {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    height: 27px;
    padding: 0 8px;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    font-size: 13px;
    text-align: left;
    cursor: pointer;
    touch-action: none;
  }
  .note:hover {
    background: color-mix(in srgb, var(--text) 5%, transparent);
    color: var(--text);
  }
  .note.on {
    background: var(--line);
    color: var(--text);
  }
  .from {
    flex: none;
    font-size: 11px;
    color: var(--faint);
  }
  .pin {
    flex: none;
    display: inline-flex;
    color: var(--accent);
  }
  .empty {
    margin: 2px 8px 4px;
    font-size: 12px;
    color: var(--faint);
  }
  .draft {
    padding: 2px 0;
  }
  .draft input {
    width: 100%;
    height: 27px;
    padding: 0 8px;
    background: var(--bg);
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    color: var(--text);
    font-size: 13px;
  }
  .new-project {
    display: flex;
    align-items: center;
    gap: 7px;
    width: 100%;
    height: 30px;
    margin-top: 6px;
    padding: 0 8px;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--faint);
    font-size: 12.5px;
    cursor: pointer;
  }
  .new-project:hover {
    background: color-mix(in srgb, var(--text) 5%, transparent);
    color: var(--text);
  }
  section {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .bar {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 56px;
    padding: 8px 12px 8px 24px;
    border-bottom: 1px solid var(--line);
  }
  .titles {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .titles h2 {
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 15px;
  }
  .path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--mono);
    font-size: 11px;
    color: var(--faint);
  }
  .seg {
    flex: none;
    display: inline-flex;
    padding: 2px;
    background: var(--panel);
    border-radius: 999px;
  }
  .seg button {
    padding: 3px 10px;
    background: none;
    border: none;
    border-radius: 999px;
    color: var(--faint);
    font-size: 12px;
    cursor: pointer;
  }
  .seg button.on {
    background: var(--line);
    color: var(--text);
  }
  .icon {
    width: 30px;
    height: 30px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--faint);
    cursor: pointer;
  }
  .icon:hover {
    background: var(--line);
    color: var(--text);
  }
  .icon.danger:hover {
    color: var(--bad);
  }
  .icon.pinned {
    color: var(--accent);
  }
  .content {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
  .doc {
    max-width: 760px;
    padding: 20px 28px 40px;
    font-size: 14px;
  }
  textarea {
    width: 100%;
    height: 100%;
    padding: 20px 28px;
    background: transparent;
    border: none;
    outline: none;
    resize: none;
    box-shadow: none !important;
    color: var(--text);
    font-family: var(--mono);
    font-size: var(--mono-size);
    line-height: 1.6;
  }
  .page {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 28px 32px 48px;
  }
  .page > * {
    max-width: 720px;
  }
  .page h2 {
    margin: 0 0 8px;
    font-size: 18px;
    font-weight: 600;
  }
  .lead {
    margin: 0 0 8px;
    font-size: 13.5px;
    line-height: 1.55;
    color: var(--muted);
  }
  .hint {
    margin: 0 0 16px;
    font-size: 12.5px;
    line-height: 1.55;
    color: var(--faint);
  }
  .mono {
    font-family: var(--mono);
    font-size: 12px;
  }
  .ghost {
    position: fixed;
    z-index: 900;
    max-width: 240px;
    padding: 5px 10px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    background: var(--panel);
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
    color: var(--text);
    font-size: 12.5px;
    pointer-events: none;
  }
</style>
