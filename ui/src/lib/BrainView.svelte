<script lang="ts">
  import { onMount, tick } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import Icon from "./Icon.svelte";
  import { api, brain, EVERYWHERE, type NoteMeta, type ObsidianState, type Project } from "./api";
  import { mdHtml, richReady } from "./md";
  import { folderName } from "./tabs";
  import { toast, toastError } from "./toast";
  import { targetBrainNote } from "./brainStore";
  import { noteMoveStarts, noteRelease, pressDrag, type PressDrag } from "./gestures";
  import { handleLinkClick } from "./links";
  import { safeDecode } from "./linkRules";
  import { tooltip } from "./tooltip";

  const UNUSED = "unused";
  const CONFLICTS = "conflicts";
  const OPEN_KEY = "parzi.brain.open.v2";
  type Folder = { key: string; title: string; project: Project | null; items: NoteMeta[] };

  let notes: NoteMeta[] = [];
  let projects: Project[] = [];
  // Pinned notes too long to send in full: path -> project slugs.
  let overflow: Record<string, string[]> = {};
  let conflictsShown = false;
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
  let press: NotePress | null = null;
  let dragging = false;
  let dragX = 0;
  let dragY = 0;
  let dropOn = "";

  $: byPath = new Map(notes.map((n) => [n.path, n]));
  $: folders = buildFolders(notes, projects, query);
  $: note = byPath.get(selected) ?? null;
  $: noteProject = projects.find((p) => p.note === selected) ?? null;
  $: copyOf = note?.conflict_of ?? "";
  $: copies = notes.filter((n) => n.conflict_of === selected);
  $: tooBig = note?.pinned ? (overflow[note.path] ?? []) : [];

  function buildFolders(list: NoteMeta[], all: Project[], q: string): Folder[] {
    // Sync conflict copies wait in their own folder until one version is kept.
    const conflicts = list.filter((n) => n.conflict_of);
    const plain = list.filter((n) => !n.conflict_of);
    const map = new Map(plain.map((n) => [n.path, n]));
    const used = new Set<string>();
    const pick = (paths: string[]) => paths.map((p) => map.get(p)).filter((n): n is NoteMeta => !!n);
    const everywhere = plain.filter(inEverywhere);
    everywhere.forEach((n) => used.add(n.path));
    const projectFolders = [...all]
      .sort((a, b) => a.title.localeCompare(b.title))
      .map((p) => {
        p.notes.forEach((path) => used.add(path));
        return { key: p.slug, title: p.title, project: p, items: pick(p.notes) };
      });
    const unused = plain.filter((n) => !isProjectNote(n) && !used.has(n.path));
    const out: Folder[] = [
      ...(conflicts.length ? [{ key: CONFLICTS, title: "Sync conflicts", project: null, items: conflicts }] : []),
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
    if (n.conflict_of) return device(n);
    if (!f.items.some((x) => x !== n && x.title === n.title)) return "";
    return /^imported\/([^/]+)\//.exec(n.path)?.[1] ?? "";
  }

  // The device a conflict copy came from: `a.conflict-<device>.md`.
  function device(n: NoteMeta) {
    return /\.conflict-([a-z0-9-]+)\.md$/i.exec(n.path)?.[1] ?? "another device";
  }

  function pinTip(n: NoteMeta, over: string[]) {
    if (!n.pinned) return "Not pinned: the agent sees the title and opens the note when it needs it. Click to pin.";
    if (!over.length) return "Pinned: the agent always gets the whole note. Click to unpin.";
    const names = over.filter((s) => s !== EVERYWHERE).map((s) => projects.find((p) => p.slug === s)?.title ?? s);
    const where = names.length ? [`${names.join(", ")} sessions`] : [];
    if (over.includes(EVERYWHERE)) where.push("sessions outside a project");
    return `Pinned, but too big to send in full in ${where.join(" and ")}, so the agent gets it listed instead. Click to unpin.`;
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
      [notes, projects, overflow] = await Promise.all([brain.list(), brain.projects(), brain.overflow().catch(() => overflow)]);
      // New conflicts open their folder once, so the user sees them.
      if (!conflictsShown && notes.some((n) => n.conflict_of)) {
        conflictsShown = true;
        toggle(CONFLICTS, true);
      }
    } catch (e) {
      toastError(e);
    }
  }

  async function onFocus() {
    await refresh();
    const sel = selected;
    if (!sel || dirty) return;
    // Background reload: a failed read keeps the text on screen. Drop the
    // result if the note changed or got edits while the read was out.
    const text = await brain.read(sel).catch(() => null);
    if (text !== null && selected === sel && !dirty) raw = text;
  }

  function onVisible() {
    if (document.visibilityState === "visible") void onFocus();
  }

  async function showList() {
    await flush();
    selected = "";
    raw = "";
    editing = false;
    dirty = false;
  }

  async function openNote(path: string) {
    await flush();
    try {
      if (!notes.length) await refresh();
      const normalized = path.replace(/\\/g, "/");
      const resolved = resolve(normalized) ?? resolve(safeDecode(normalized)) ?? normalized;
      raw = await brain.read(resolved);
      selected = resolved;
      editing = false;
      dirty = false;
      const holder = folders.find((f) => f.items.some((n) => n.path === resolved)) ?? folders.find((f) => f.project && resolved.startsWith(`projects/${f.key}/`));
      if (holder) toggle(holder.key, true);
      // Fallback so the note view renders even if the index hasn't caught up.
      if (!byPath.get(selected)) {
        const title = selected.split("/").pop()?.replace(/\.md$/, "") ?? selected;
        notes = [...notes, { path: selected, title, summary: "", projects: [], tags: [], links: [], pinned: false, modified: 0, bytes: 0 }];
      }
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
      [projects, overflow] = await Promise.all([brain.projects(), brain.overflow().catch(() => overflow)]);
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
      overflow = await brain.overflow().catch(() => overflow);
      const text = await brain.read(meta.path);
      if (selected === meta.path && !dirty) raw = text;
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
    // A conflict copy only goes away by keeping one version.
    if (!n || from === to || n.conflict_of || to === CONFLICTS) return;
    if (!copy && from !== UNUSED && viaLink(n, from)) {
      toast("This note links to that project, so it stays there. Remove the [[link]] to move it.");
      return;
    }
    await flush();
    try {
      if (to !== UNUSED) await brain.map(path, to, true);
      if (!copy && from !== UNUSED) await brain.map(path, from, false);
      await refresh();
      if (selected === path) {
        const text = await brain.read(path);
        if (selected === path && !dirty) raw = text;
      }
      if (to !== UNUSED) toggle(to, true);
    } catch (e) {
      toastError(e);
    }
  }

  function noteAt(x: number, y: number): string | null {
    return document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-note]")?.dataset.note ?? null;
  }

  type NotePress = { path: string; from: string; title: string; x: number; y: number };

  // A press on a note: a click opens it, a drag onto a folder moves it
  // there (Ctrl copies). A sync conflict copy only opens.
  const noteDrag = (n: NoteMeta, from: string): PressDrag<NotePress> => ({
    press: (e) => (press = { path: n.path, from, title: n.title, x: e.clientX, y: e.clientY }),
    starts: (p, e) =>
      !byPath.get(p.path)?.conflict_of && noteMoveStarts(Math.hypot(e.clientX - p.x, e.clientY - p.y), noteAt(e.clientX, e.clientY), p.path),
    start: () => (dragging = true),
    drag(_, e) {
      dragX = e.clientX;
      dragY = e.clientY;
      const hit = document.elementFromPoint(e.clientX, e.clientY)?.closest<HTMLElement>("[data-folder]");
      dropOn = hit?.dataset.folder ?? "";
    },
    release(p, e, dragged) {
      const target = dropOn;
      cancelPress();
      const upPath = (e.target instanceof Element ? e.target.closest<HTMLElement>("[data-note]") : null)?.dataset.note ?? null;
      const result = noteRelease(p, dragged, target, upPath, e.ctrlKey || e.metaKey);
      if (result.kind === "move") void moveNote(result.path, result.from, result.to, result.copy);
      else if (result.kind === "open") void openNote(result.path);
    },
    cancel: cancelPress,
  });

  function cancelPress() {
    press = null;
    dragging = false;
    dropOn = "";
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

  async function settle(keepCopy: boolean) {
    if (!note?.conflict_of) return;
    const { path, conflict_of: original } = note;
    const question = keepCopy ? `Replace ${original} with this copy? The other version is deleted.` : `Delete this copy and keep ${original}?`;
    const ok = await ask(question, { title: keepCopy ? "Keep this copy" : "Keep original", kind: "warning" }).catch(() => false);
    if (!ok) return;
    await flush();
    try {
      const kept = await brain.resolveConflict(path, keepCopy);
      selected = "";
      raw = "";
      dirty = false;
      await refresh();
      await openNote(kept);
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
    const href = a.getAttribute("href") ?? "";
    if (href.startsWith("parzi:note/")) {
      e.preventDefault();
      const path = resolve(safeDecode(href.slice("parzi:note/".length)));
      if (path) void openNote(path);
      else toast("That note doesn't exist yet");
      return;
    }
    // Web, file and other links take the app-wide path (confirm first).
    void handleLinkClick(e);
  }

  onMount(() => {
    void refresh();
    brain.obsidian().then((s) => (obsidian = s)).catch(() => {});
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onVisible);
    const unTarget = targetBrainNote.subscribe((path) => {
      if (path) {
        targetBrainNote.set("");
        void openNote(path);
      }
    });
    return () => {
      unTarget();
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onVisible);
      void flush();
    };
  });
</script>

<div class="brain" class:dragging>
  <div class="bar page-bar">
    <div class="seg page-tabs" role="tablist" aria-label="Brain">
      <button aria-pressed={!note} on:click={showList}>Notes</button>
      {#if note}
        <button aria-pressed={!editing} on:click={() => (editing = false)}>Read</button>
        <button aria-pressed={editing} on:click={() => (editing = true)}>Edit</button>
      {/if}
    </div>
    {#if note}
      <h2 class="note-title" title={noteProject ? noteProject.folder : note.path}>{note.title}</h2>
      {#if copyOf}<span class="badge">Conflict</span>{/if}
    {:else}
      <div class="page-search">
        <Icon name="search" />
        <input bind:value={query} placeholder="Search" aria-label="Search notes" spellcheck="false" />
      </div>
    {/if}
    {#if note}
      {#if !noteProject && !copyOf}
        <button
          class="back-btn"
          class:pinned={note.pinned}
          class:over={tooBig.length > 0}
          aria-label="Pin note"
          aria-pressed={note.pinned}
          use:tooltip={pinTip(note, tooBig)}
          on:click={togglePin}
        >
          <Icon name="pin" />
        </button>
      {/if}
      {#if obsidian !== "missing"}
        <button class="back-btn" use:tooltip={"Open in Obsidian"} on:click={() => openOutside(note?.path ?? "", "obsidian")}><Icon name="arrowRight" /></button>
      {/if}
      <button class="back-btn" use:tooltip={"Open the file"} on:click={() => openOutside(note?.path ?? "", "file")}><Icon name="file" /></button>
      <button class="back-btn danger" use:tooltip={noteProject ? "Remove project (its notes stay)" : "Delete note"} on:click={removeNote}><Icon name="trash" /></button>
    {:else}
      <button class="link" on:click={newProject}>New project</button>
    {/if}
  </div>

  {#if note}
      {#if copyOf}
        <div class="conflict">
          <p>
            This note changed on two devices before they synced. This is the version from {device(note)}; agents skip it until you keep one.
          </p>
          <div class="actions">
            <button class="btn" on:click={() => openNote(copyOf)}>Open original</button>
            <button class="btn" on:click={() => settle(false)}>Keep original</button>
            <button class="btn primary" on:click={() => settle(true)}>Keep this copy</button>
          </div>
        </div>
      {:else if copies.length}
        <div class="conflict">
          <p>Another device saved a different version of this note. Open it to keep one.</p>
          <div class="actions">
            <button class="btn" on:click={() => openNote(copies[0].path)}>Open the other version</button>
          </div>
        </div>
      {/if}
      <div class="content">
        {#if editing}
          <textarea bind:value={raw} aria-label="Note text" on:input={onEdit} on:blur={flush} spellcheck="true" />
        {:else}
          <!-- svelte-ignore a11y-click-events-have-key-events a11y-no-static-element-interactions -->
          <div class="msg doc" on:click={onPreviewClick}>{@html mdHtml(withLinks(body(raw)), $richReady, false)}</div>
        {/if}
      </div>
    {:else}
      <div class="list">
        {#each folders as f (f.key)}
          {@const fixed = f.key === UNUSED || f.key === CONFLICTS}
          {@const drop = f.key === CONFLICTS ? undefined : f.key}
          {#if !fixed || f.items.length || !query.trim()}
            <div class="group" data-folder={drop} class:drop={dragging && dropOn === f.key}>
              <div class="group-head">
                <h3 title={f.project?.folder ?? ""}>{f.title}</h3>
                {#if !fixed}
                  <button class="add" use:tooltip={`New note in ${f.title}`} on:click={() => startNote(f.key)}><Icon name="plus" size={12} /></button>
                {/if}
              </div>
              {#each f.items as n (n.path)}
                <div class="row">
                  <button
                    class="open"
                    data-note={n.path}
                    title={n.conflict_of ? `Sync conflict copy of ${n.conflict_of}` : n.summary || n.path}
                    use:pressDrag={noteDrag(n, f.key)}
                    on:keydown={(e) => e.key === "Enter" && openNote(n.path)}
                  >
                    <span class="title">{n.title}</span>
                    {#if twin(n, f)}<span class="url">{twin(n, f)}</span>{/if}
                    {#if n.pinned && !n.conflict_of}
                      <span class="pin" class:over={!!overflow[n.path]} title={overflow[n.path] ? "Too big to send in full, listed instead" : "Always included in full"}><Icon name="pin" size={12} /></span>
                    {/if}
                  </button>
                </div>
              {/each}
              {#if creatingIn === f.key}
                <form class="draft" on:submit|preventDefault={createNote}>
                  <input bind:this={draftField} bind:value={draftName} placeholder="Note name" aria-label="Note name" on:blur={createNote} />
                </form>
              {:else if !f.items.length}
                <p class="empty">{fixed ? "Nothing here." : "Empty. Drag notes here."}</p>
              {/if}
            </div>
          {/if}
        {/each}
      </div>
    {/if}

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
    flex-direction: column;
  }
  .brain.dragging,
  .brain.dragging * {
    cursor: grabbing !important;
    user-select: none;
  }
  .link {
    flex: none;
    padding: var(--space) calc(var(--space) * 2);
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .link:hover {
    color: var(--text);
  }
  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    width: 100%;
    padding: 0 calc(var(--space) * 4) calc(var(--space) * 8);
  }
  .group.drop {
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }
  .group-head {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    margin: calc(var(--space) * 4) calc(var(--space) * 3) calc(var(--space) * 2);
  }
  h3 {
    flex: 1;
    min-width: 0;
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-md);
    font-weight: 600;
    color: var(--muted);
  }
  .add {
    width: 22px;
    height: 22px;
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
  .add:hover {
    color: var(--text);
  }
  .group:hover .add,
  .add:focus-visible {
    opacity: 1;
  }
  .row {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    border-radius: var(--radius);
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
    height: 34px;
    padding: 0 calc(var(--space) * 3);
    background: none;
    border: none;
    color: var(--text);
    font-size: var(--fs-md);
    text-align: left;
    cursor: pointer;
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
  .pin {
    flex: none;
    display: inline-flex;
    color: var(--accent);
  }
  .pin.over {
    color: var(--warn);
  }
  .empty {
    margin: 0 0 calc(var(--space) * 2) calc(var(--space) * 3);
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .draft {
    padding: 0 calc(var(--space) * 3) calc(var(--space) * 2);
  }
  .draft input {
    width: 100%;
    height: 34px;
    padding: 0 calc(var(--space) * 3);
    background: var(--bg);
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--fs-md);
  }
  .note-title {
    flex: 1;
    margin: 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-md);
    font-weight: 600;
    color: var(--text);
  }
  .badge {
    flex: none;
    color: var(--warn);
    font-size: var(--fs-md);
    font-weight: 600;
  }
  .back-btn.pinned { color: var(--accent); }
  .back-btn.pinned.over { color: var(--warn); }
  .back-btn.danger:hover { color: var(--bad); }
  .conflict {
    flex: none;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: calc(var(--space) * 3) calc(var(--space) * 4);
    margin: calc(var(--space) * 3) calc(var(--space) * 6) 0;
    padding: calc(var(--space) * 3) calc(var(--space) * 4);
    border: 1px solid var(--warn);
    border-radius: var(--radius-lg);
  }
  .conflict p {
    flex: 1 1 280px;
    margin: 0;
    font-size: var(--fs-md);
    line-height: 1.5;
    color: var(--text);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: calc(var(--space) * 2);
  }
  .seg {
    flex: none;
  }
  .content {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
  .doc {
    max-width: 760px;
    padding: calc(var(--space) * 6) calc(var(--space) * 8) calc(var(--space) * 8);
    font-size: var(--fs-base);
    user-select: text;
  }
  textarea {
    width: 100%;
    height: 100%;
    padding: calc(var(--space) * 6) calc(var(--space) * 8);
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
  .ghost {
    position: fixed;
    z-index: 900;
    max-width: 240px;
    padding: var(--space) calc(var(--space) * 3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    background: var(--panel);
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--fs-md);
    pointer-events: none;
  }
</style>
