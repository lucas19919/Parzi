<script lang="ts">
  import { onMount } from "svelte";
  import { api, type ProjectView, type DocEntry } from "../api";
  import Icon from "../Icon.svelte";
  import "./shared.css";

  export let notify: (msg: string) => void = () => {};
  export let currentProject = "default";

  const I = {
    copy: "M8 5H6a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-1M8 5a2 2 0 0 0 2 2h2a2 2 0 0 0 2-2M8 5a2 2 0 0 1 2-2h2a2 2 0 0 1 2 2m0 0h2a2 2 0 0 1 2 2v3m2 4H10m0 0l3-3m-3 3l3 3",
    refresh: "M21 12a9 9 0 1 1-2.6-6.4M21 3v6h-6",
  };

  let projects: ProjectView[] = [];
  let sel = "";
  let projErr = "";
  let docs: DocEntry[] = [];
  let docsNote = "";
  let loadingDocs = false;
  let activePath = "";
  let activeSource = "";
  let content = "";
  let draft = "";
  let contentErr = "";
  let loadingFile = false;
  let saving = false;

  let newFile = "";
  let newFileErr = "";
  let showSystemComposer = false;
  let systemDraft = "";

  onMount(async () => {
    try {
      projects = await api.listProjects();
    } catch (e) {
      projErr = String(e);
      return;
    }
    sel = projects.some((p) => p.name === currentProject)
      ? currentProject
      : (projects[0]?.name ?? "");
    if (sel) void loadDocs();
  });

  async function loadDocs() {
    const proj = projects.find((p) => p.name === sel);
    loadingDocs = true;
    docsNote = "";
    docs = [];
    activePath = "";
    activeSource = "";
    content = "";
    draft = "";
    contentErr = "";
    showSystemComposer = false;
    try {
      docs = await api.listProjectDocs(sel, proj?.root ?? "");
      if (!docs.length) docsNote = "No context files found — no SYSTEM.md and no markdown in the workspace root.";
    } catch (e) {
      docsNote = String(e);
    } finally {
      loadingDocs = false;
    }
  }

  function selectProject(name: string) {
    if (name === sel) return;
    if (dirty && !confirm("Discard unsaved context edits?")) return;
    sel = name;
    void loadDocs();
  }

  async function openDoc(d: DocEntry) {
    if (d.path === activePath && content) return;
    if (dirty && !confirm("Discard unsaved context edits?")) return;
    activePath = d.path;
    activeSource = d.source;
    content = "";
    draft = "";
    contentErr = "";
    loadingFile = true;
    try {
      content = await api.readTextFile(d.path);
      draft = content;
    } catch (e) {
      contentErr = String(e);
    } finally {
      loadingFile = false;
    }
  }

  $: dirty = draft !== content;

  async function saveDoc() {
    if (!activePath || !dirty || saving) return;
    saving = true;
    try {
      if (activeSource === "system") {
        const p = await api.saveProjectSystem(sel, draft);
        activePath = p;
      } else {
        await api.writeTextFile(activePath, draft);
      }
      content = draft;
      await loadDocsKeepSelection();
      notify("Context saved");
    } catch (e) {
      notify(`Save failed: ${e}`);
    } finally {
      saving = false;
    }
  }

  /** Refresh the doc list without dropping the open editor. */
  async function loadDocsKeepSelection() {
    const proj = projects.find((p) => p.name === sel);
    try {
      docs = await api.listProjectDocs(sel, proj?.root ?? "");
    } catch {}
  }

  async function createSystem() {
    if (!systemDraft.trim() || saving) return;
    saving = true;
    try {
      const p = await api.saveProjectSystem(sel, systemDraft.trim() + "\n");
      systemDraft = "";
      showSystemComposer = false;
      await loadDocs();
      const created = docs.find((d) => d.path === p);
      if (created) void openDoc(created);
      notify("SYSTEM.md created");
    } catch (e) {
      notify(`Create failed: ${e}`);
    } finally {
      saving = false;
    }
  }

  async function createMemoryFile() {
    newFileErr = "";
    const proj = projects.find((p) => p.name === sel);
    const root = (proj?.root ?? "").trim();
    if (!root) {
      newFileErr = "This workspace has no folder yet.";
      return;
    }
    let name = newFile.trim().replace(/\.md$/i, "");
    if (!/^[a-z0-9][a-z0-9 _-]{0,60}$/i.test(name)) {
      newFileErr = "File name: letters, numbers, spaces, dashes.";
      return;
    }
    const fileName = `${name}.md`;
    const sep = root.includes("/") && !root.includes("\\") ? "/" : "\\";
    const full = `${root.replace(/[/\\]+$/, "")}${sep}${fileName}`;
    if (docs.some((d) => d.path.toLowerCase() === full.toLowerCase())) {
      newFileErr = "A file with that name already exists.";
      return;
    }
    try {
      await api.writeTextFile(full, `# ${name}\n\n`);
      newFile = "";
      await loadDocsKeepSelection();
      const created = docs.find((d) => d.path.toLowerCase() === full.toLowerCase());
      if (created) void openDoc(created);
      notify(`${fileName} created`);
    } catch (e) {
      newFileErr = String(e);
    }
  }

  function copyText(t: string, msg: string) {
    navigator.clipboard.writeText(t).then(
      () => notify(msg),
      () => notify("Copy failed"),
    );
  }

  $: activeDoc = docs.find((d) => d.path === activePath) ?? null;
  $: sizeLabel = content
    ? `${(content.length / 1024).toFixed(1)} KiB · ~${Math.max(1, Math.round(content.length / 4))} tokens`
    : "";
</script>

{#if projErr}
  <div class="load-err"><span>{projErr}</span></div>
{:else}
  <div class="pref-section">
    <div class="section-head-with-action">
      <div>
        <h3 class="section-title">Context</h3>
        <p class="section-desc">What feeds project memory: the project <b>SYSTEM.md</b> plus workspace markdown. Edit in place, or add new memory files.</p>
      </div>
      <button class="sbtn" on:click={loadDocs} disabled={!sel || loadingDocs}>
        <Icon d={I.refresh} size={12} />
        <span>Refresh</span>
      </button>
    </div>

    {#if projects.length > 1}
      <div class="proj-row">
        {#each projects as p (p.name)}
          <button class="seg-btn" class:active={p.name === sel} on:click={() => selectProject(p.name)}>
            {p.name === "default" ? "Inbox" : p.name}
          </button>
        {/each}
      </div>
    {/if}

    {#if loadingDocs}
      <div class="skel tall" />
    {:else if (docsNote && !docs.length)}
      <div class="field-card">
        <div class="field-info">
          <span class="field-label">{sel || "No project"}</span>
          <span class="field-hint">{docsNote}</span>
        </div>
      </div>
    {:else}
      <div class="doc-list">
        {#each docs as d (d.path)}
          <button class="doc-row" class:on={d.path === activePath} on:click={() => openDoc(d)}>
            <span class="doc-meta">
              <span class="doc-label">{d.label}</span>
              <span class="doc-path">{d.path}</span>
            </span>
            <span class="status-badge {d.source === 'system' ? 'ok' : 'missing'}">{d.source === "system" ? "system" : "workspace"}</span>
          </button>
        {/each}
      </div>
    {/if}
  </div>

  {#if activePath}
    <div class="pref-section">
      <div class="section-head-with-action">
        <div>
          <h3 class="section-title">{activeDoc?.label ?? activePath}{#if dirty} <span class="dirty-dot" title="Unsaved edits">•</span>{/if}</h3>
          <p class="section-desc">{activePath}{#if sizeLabel} · {sizeLabel}{/if}</p>
        </div>
        <div class="btn-row">
          {#if content}
            <button class="sbtn" on:click={() => copyText(draft, "Context copied to clipboard")}>
              <Icon d={I.copy} size={12} />
              <span>Copy</span>
            </button>
          {/if}
          {#if dirty}
            <button class="sbtn" on:click={() => (draft = content)}><span>Discard</span></button>
            <button class="sbtn primary" on:click={saveDoc} disabled={saving}><span>{saving ? "Saving…" : "Save"}</span></button>
          {/if}
        </div>
      </div>
      {#if loadingFile}
        <div class="skel tall" />
      {:else if (contentErr)}
        <div class="load-err"><span>{contentErr}</span></div>
      {:else}
        <textarea class="editor" rows="18" bind:value={draft} spellcheck="false" />
      {/if}
    </div>
  {/if}

  <div class="pref-section">
    <h3 class="section-title">Add memory</h3>
    {#if !docs.some((d) => d.source === "system")}
      {#if showSystemComposer}
        <div class="field-card col">
          <div class="field-info">
            <span class="field-label">Project SYSTEM.md</span>
            <span class="field-hint">The standing prompt for {sel || "this project"} — personality, conventions, hard rules.</span>
          </div>
          <textarea class="editor small" rows="6" placeholder="e.g. You are the senior engineer on this repo. Prefer small diffs…" bind:value={systemDraft} />
          <div class="btn-row">
            <button class="sbtn primary" on:click={createSystem} disabled={!systemDraft.trim() || saving}><span>{saving ? "Creating…" : "Create SYSTEM.md"}</span></button>
            <button class="sbtn" on:click={() => (showSystemComposer = false)}><span>Cancel</span></button>
          </div>
        </div>
      {:else}
        <div class="field-card">
          <div class="field-info">
            <span class="field-label">No SYSTEM.md yet</span>
            <span class="field-hint">Give {sel || "this project"} a standing memory.</span>
          </div>
          <button class="sbtn" on:click={() => (showSystemComposer = true)}><span>Write one</span></button>
        </div>
      {/if}
    {/if}
    <div class="new-row">
      <input
        class="txt"
        placeholder="New memory file — e.g. DECISIONS"
        bind:value={newFile}
        on:keydown={(e) => e.key === "Enter" && createMemoryFile()}
      />
      <button class="sbtn primary" on:click={createMemoryFile} disabled={!newFile.trim()}><span>Add file</span></button>
    </div>
    {#if newFileErr}
      <span class="form-err">{newFileErr}</span>
    {/if}
  </div>
{/if}

<style>
  .proj-row { display: flex; flex-wrap: wrap; gap: 6px; }
  .seg-btn {
    background: var(--surface-1); border: 1px solid var(--line-2); border-radius: var(--radius-pill);
    color: var(--text-3); font: inherit; font-size: 12px; font-weight: 500;
    padding: 5px 13px; cursor: pointer;
  }
  .seg-btn:hover { color: var(--text); }
  .seg-btn.active { background: var(--accent-soft); border-color: var(--accent-line); color: var(--text); }
  .doc-list { display: flex; flex-direction: column; gap: 4px; max-height: 300px; overflow-y: auto; }
  .doc-row {
    display: flex; align-items: center; gap: 10px; width: 100%;
    background: var(--surface-1); border: 1px solid transparent; border-radius: var(--radius-2);
    color: var(--text-2); text-align: left; padding: 7px 12px; cursor: pointer; font: inherit;
  }
  .doc-row:hover { background: var(--surface-2); color: var(--text); }
  .doc-row.on { background: var(--surface-2); border-color: var(--accent-line); color: var(--text); }
  .doc-meta { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .doc-label { font-size: 12.5px; font-weight: 500; color: var(--text); }
  .doc-path { font-family: var(--parzi-mono), ui-monospace, monospace; font-size: 10.5px; color: var(--text-4); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dirty-dot { color: var(--warn); }
  .btn-row { display: flex; gap: 8px; flex: none; }
  .editor {
    width: 100%; background: var(--code); border: 1px solid var(--line-2);
    border-radius: var(--radius-3); color: var(--text); caret-color: var(--accent);
    font-family: var(--parzi-mono), ui-monospace, monospace; font-size: 12px; line-height: 1.6;
    padding: 12px 14px; resize: vertical; min-height: 280px; user-select: text;
  }
  .editor.small { min-height: 120px; }
  .editor:focus { border-color: var(--accent); box-shadow: 0 0 0 2px var(--accent-mid); outline: none; }
  .field-card.col { flex-direction: column; align-items: stretch; }
  .new-row { display: flex; gap: 8px; }
  .new-row .txt { flex: 1; }
  .txt {
    background: var(--input); border: 1px solid var(--line-2); border-radius: var(--radius-2);
    color: var(--text); font: inherit; font-size: 12.5px; padding: 7px 10px; width: 100%;
  }
  .txt::placeholder { color: var(--text-4); }
  .form-err { font-size: 12px; color: var(--bad); }
</style>
