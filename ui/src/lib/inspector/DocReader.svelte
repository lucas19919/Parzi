<script lang="ts">
  import { createEventDispatcher, tick } from "svelte";
  import { renderMarkdown } from "../md";
  import type { InspectorArtifact, InspectorDoc, DocEntry } from "../api";

  /** Artifact currently shown (null = document mode / empty). */
  export let artifact: InspectorArtifact | null = null;
  /** Every artifact version of the active thread; used for the version menu. */
  export let artifacts: InspectorArtifact[] = [];
  /** Project document currently shown. */
  export let doc: InspectorDoc | null = null;
  /** Quick-tab candidates (SYSTEM.md, PLAN.md, …). */
  export let docs: DocEntry[] = [];
  /** True when a thread is open — enables the transcript tab. */
  export let hasThread = false;
  export let loading = false;

  const dispatch = createEventDispatcher<{
    openDoc: { entry: DocEntry };
    openTranscript: void;
    openArtifact: { artifact: InspectorArtifact };
    pickFile: void;
  }>();

  let view: "preview" | "raw" = "preview";
  let copied = false;
  let body: HTMLElement | null = null;
  let showAllDocs = false;

  $: mode = artifact ? "artifact" : doc ? "doc" : "empty";
  $: content = artifact ? artifact.content : doc ? doc.content : "";
  $: title = artifact ? artifact.title : doc ? doc.title : "";
  $: lines = content ? content.split("\n").length : 0;
  $: isMarkdown = artifact ? artifact.kind === "markdown" : true;
  $: lang = artifact ? (artifact.language || (artifact.kind === "diff" ? "diff" : artifact.kind === "code" ? "" : artifact.kind)) : "markdown";
  /** HTML/SVG artifacts preview as a live page, not as code. Sandboxed with
      an opaque origin: scripts run, but reach neither the parent DOM nor IPC. */
  $: canRender = !!artifact && (artifact.kind === "html" || artifact.kind === "svg") && !!content.trim();
  $: showRender = canRender && view === "preview";
  $: versions = artifact ? artifacts.filter((a) => a.id === artifact.id).sort((a, b) => a.version - b.version) : [];
  $: docFamilies = (() => {
    const seen = new Map<string, InspectorArtifact>();
    for (const a of artifacts) {
      const cur = seen.get(a.id);
      if (!cur || a.version > cur.version) seen.set(a.id, a);
    }
    return [...seen.values()];
  })();
  $: quickDocs = showAllDocs ? docs : docs.slice(0, 4);

  // Preview HTML: markdown renders as prose; live pages (html/svg) render
  // in a sandboxed iframe (see showRender); code/diff render through the
  // fenced-block pipeline so highlighting and diff tinting match the thread.
  $: html = (() => {
    if (!content) return "";
    if (view === "raw") return renderMarkdown("```" + (isMarkdown ? "markdown" : lang || "text") + "\n" + content.slice(0, 200000) + "\n```");
    if (isMarkdown) return renderMarkdown(content);
    if (canRender) return "";
    return renderMarkdown("```" + (lang || "text") + "\n" + content.slice(0, 200000) + "\n```");
  })();

  /** Table of contents from markdown headings (preview of markdown only). */
  $: toc = (() => {
    if (!isMarkdown || view !== "preview" || !content) return [] as { level: number; text: string; idx: number }[];
    const out: { level: number; text: string; idx: number }[] = [];
    let inFence = false;
    for (const raw of content.split("\n")) {
      const line = raw.trimEnd();
      if (/^(```|~~~)/.test(line.trim())) inFence = !inFence;
      if (inFence) continue;
      const m = /^(#{1,4})\s+(.+?)\s*#*\s*$/.exec(line);
      if (m) out.push({ level: m[1].length, text: m[2].replace(/[*_`]/g, ""), idx: out.length });
    }
    return out;
  })();

  // Rendered headings get positional ids so the TOC can jump to them.
  $: if (body && html) tagHeadings();
  async function tagHeadings() {
    await tick();
    if (!body) return;
    const hs = body.querySelectorAll("h1, h2, h3, h4");
    hs.forEach((h, i) => h.setAttribute("id", `deck-h-${i}`));
  }
  function jump(i: number) {
    body?.querySelector(`#deck-h-${i}`)?.scrollIntoView({ block: "start", behavior: "smooth" });
  }

  function copy() {
    navigator.clipboard.writeText(content);
    copied = true;
    setTimeout(() => (copied = false), 1200);
  }

  function download() {
    let name: string;
    if (artifact) {
      const ext = artifact.kind === "code" && artifact.language ? artifact.language : artifact.kind === "markdown" ? "md" : artifact.kind === "svg" ? "svg" : artifact.kind === "html" ? "html" : artifact.kind === "diff" ? "diff" : "txt";
      name = `${artifact.id}-v${artifact.version}.${ext}`;
    } else {
      name = (doc?.path?.split(/[\\/]/).pop() || doc?.title || "document").replace(/[^\w.-]+/g, "_") + (doc?.path ? "" : ".md");
    }
    const blob = new Blob([content], { type: "text/plain" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = name;
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 2000);
  }

  function pickVersion(e: Event) {
    const v = Number((e.target as HTMLSelectElement).value);
    const hit = versions.find((a) => a.version === v);
    if (hit) dispatch("openArtifact", { artifact: hit });
  }

  /** Fenced-block chrome (copy / expand) rendered by md.ts is inert HTML; wire it here. */
  function onBodyClick(e: MouseEvent) {
    const el = e.target as HTMLElement;
    const copyBtn = el.closest("[data-copy]") as HTMLElement | null;
    if (copyBtn) {
      const code = copyBtn.closest(".codeblock")?.querySelector("code")?.innerText ?? "";
      navigator.clipboard.writeText(code);
      copyBtn.textContent = "copied";
      setTimeout(() => (copyBtn.textContent = "copy"), 1200);
    }
  }

  // Reset scroll + view when the document changes.
  let lastKey = "";
  $: {
    const key = artifact ? `a:${artifact.id}@${artifact.version}` : doc ? `d:${doc.path ?? doc.title}` : "";
    if (key !== lastKey) {
      lastKey = key;
      if (body) body.scrollTop = 0;
    }
  }
</script>

<div class="reader">
  <!-- One tab row: project docs, transcript, artifacts, file picker -->
  <div class="quick">
    {#each quickDocs as d (d.path)}
      <button class="qt" class:on={doc?.path === d.path} title={d.path} on:click={() => dispatch("openDoc", { entry: d })}>
        {d.label}
      </button>
    {/each}
    {#if docs.length > 4}
      <button class="qt more" on:click={() => (showAllDocs = !showAllDocs)}>{showAllDocs ? "less" : `+${docs.length - 4}`}</button>
    {/if}
    {#if hasThread}
      <button class="qt" class:on={doc?.title === "session.md"} title="Formatted transcript of this thread" on:click={() => dispatch("openTranscript")}>session.md</button>
    {/if}
    {#each docFamilies as a (a.id)}
      <button class="qt art" class:on={artifact?.id === a.id} title={`${a.id} · v${a.version}`} on:click={() => dispatch("openArtifact", { artifact: a })}>
        {a.title}
      </button>
    {/each}
    <button class="qt pick" title="Open any markdown file from the workspace" on:click={() => dispatch("pickFile")}>
      <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 5v14M5 12h14" /></svg>
      file
    </button>
  </div>

  {#if mode === "empty"}
    <div class="empty">
      {#if loading}
        <span class="empty-title">Loading…</span>
      {:else}
        <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z" /><path d="M14 3v6h6M8 13h8M8 17h5" /></svg>
        <span class="empty-title">Nothing open</span>
        <span class="empty-sub">Pick a project document above, or open an artifact from the thread.</span>
      {/if}
    </div>
  {:else}
    <!-- Sticky document toolbar -->
    <div class="bar">
      <span class="bar-title" title={artifact ? artifact.id : doc?.path ?? ""}>{title}</span>
      {#if artifact}
        <span class="badge">{artifact.kind}{artifact.language ? ` · ${artifact.language}` : ""}</span>
        {#if versions.length > 1}
          <select class="ver" value={String(artifact.version)} on:change={pickVersion} title="Version">
            {#each versions as v (v.version)}
              <option value={String(v.version)}>v{v.version}</option>
            {/each}
          </select>
        {:else}
          <span class="badge ver-badge">v{artifact.version}</span>
        {/if}
      {:else if doc?.path}
        <span class="badge">markdown</span>
      {/if}
      <span class="badge dim">{lines} lines</span>
      <span class="spacer" />
      <div class="seg" role="tablist">
        <button class="seg-btn" class:on={view === "preview"} on:click={() => (view = "preview")}>Preview</button>
        <button class="seg-btn" class:on={view === "raw"} on:click={() => (view = "raw")}>Raw</button>
      </div>
      <button class="mini" on:click={copy}>{copied ? "copied" : "copy"}</button>
      <button class="mini" on:click={download}>save</button>
    </div>

    <div class="split">
      {#if toc.length > 2}
        <nav class="toc">
          {#each toc as h (h.idx)}
            <button class="toc-row l{h.level}" on:click={() => jump(h.idx)} title={h.text}>{h.text}</button>
          {/each}
        </nav>
      {/if}
      <!-- Delegated click for the copy chrome inside rendered fences; not a control itself. -->
      <!-- svelte-ignore a11y-no-static-element-interactions a11y-click-events-have-key-events -->
      <div class="body" bind:this={body} on:click={onBodyClick}>
        {#if showRender && artifact}
          <iframe class="render" title={artifact.title} sandbox="allow-scripts" srcdoc={artifact.content}></iframe>
        {:else}
          <div class="prose" class:code={!isMarkdown || view === "raw"}>{@html html}</div>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .reader { display: flex; flex-direction: column; min-height: 0; height: 100%; }
  .quick {
    display: flex; flex-wrap: wrap; align-items: center; gap: 4px;
    padding: 8px 10px 4px; flex: none;
  }
  .qt {
    background: var(--surface-1, rgba(255,255,255,0.04)); border: 1px solid transparent;
    border-radius: var(--radius-pill, 999px); color: var(--text-3, var(--parzi-text-dim, #94a3b8));
    font: inherit; font-size: 11px; padding: 3px 9px; cursor: pointer; max-width: 160px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    display: inline-flex; align-items: center; gap: 4px;
  }
  .qt:hover { color: var(--text, #f1f5f9); background: var(--surface-2, rgba(255,255,255,0.07)); }
  .qt.on { background: var(--accent-soft, rgba(124,140,255,0.14)); border-color: var(--accent-line, rgba(124,140,255,0.4)); color: var(--text, #fff); }
  .qt.more, .qt.pick { color: var(--text-4, #5d636f); background: transparent; border-style: dashed; border-color: var(--line-2, rgba(255,255,255,0.08)); }
  .qt.art.on { border-color: var(--accent-line, rgba(124,140,255,0.4)); }

  .empty {
    flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center;
    gap: 6px; color: var(--text-4, #5d636f); padding: 24px; text-align: center;
  }
  .empty-title { font-size: 13px; color: var(--text-3, #94a3b8); font-weight: 500; }
  .empty-sub { font-size: 11.5px; max-width: 240px; line-height: 1.5; }

  .bar {
    display: flex; align-items: center; gap: 6px; flex: none;
    padding: 6px 10px; margin: 4px 8px 0;
    background: var(--surface-1, rgba(255,255,255,0.04));
    border: 1px solid var(--line-2, rgba(255,255,255,0.08));
    border-radius: var(--radius-3, 10px);
    font-size: 12px;
  }
  .bar-title {
    font-weight: 600; color: var(--text, #f1f5f9); overflow: hidden; text-overflow: ellipsis;
    white-space: nowrap; min-width: 0; flex: 0 1 auto;
  }
  .badge {
    font-family: var(--parzi-mono, monospace); font-size: 10px; color: var(--text-3, #94a3b8);
    background: var(--surface-2, rgba(255,255,255,0.05)); border-radius: 4px; padding: 2px 6px; white-space: nowrap; flex: none;
  }
  .badge.dim { opacity: 0.7; }
  .ver-badge { color: var(--accent, #7c8cff); }
  .ver {
    font-family: var(--parzi-mono, monospace); font-size: 10px; color: var(--accent, #7c8cff);
    background: var(--surface-2, rgba(255,255,255,0.05)); border: none; border-radius: 4px; padding: 2px 4px; cursor: pointer;
  }
  .spacer { flex: 1; min-width: 4px; }
  .seg {
    display: inline-flex; background: var(--surface-1, rgba(255,255,255,0.04));
    border: 1px solid var(--line-2, rgba(255,255,255,0.08)); border-radius: var(--radius-1, 6px); padding: 1px; flex: none;
  }
  .seg-btn {
    background: transparent; border: none; border-radius: 5px; color: var(--text-3, #94a3b8);
    font: inherit; font-size: 11px; padding: 2px 8px; cursor: pointer;
  }
  .seg-btn.on { background: var(--surface-3, rgba(255,255,255,0.11)); color: var(--text, #fff); }
  .mini {
    background: var(--surface-2, rgba(255,255,255,0.06)); border: none; border-radius: 4px;
    color: var(--text-3, #94a3b8); font: inherit; font-size: 10px; padding: 3px 8px; cursor: pointer; flex: none;
  }
  .mini:hover { color: var(--text, #fff); background: var(--surface-3, rgba(255,255,255,0.12)); }

  .split { flex: 1; display: flex; min-height: 0; }
  .toc {
    flex: none; width: 128px; overflow-y: auto; padding: 10px 4px 10px 10px;
    border-right: 1px solid var(--line-2, rgba(255,255,255,0.06)); display: flex; flex-direction: column; gap: 1px;
  }
  .toc-row {
    background: transparent; border: none; border-radius: 4px; color: var(--text-3, #94a3b8);
    font: inherit; font-size: 11px; text-align: left; padding: 3px 6px; cursor: pointer;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap; line-height: 1.3;
  }
  .toc-row:hover { color: var(--text, #fff); background: var(--surface-1, rgba(255,255,255,0.04)); }
  .toc-row.l1 { color: var(--text, #e4e5ec); font-weight: 600; }
  .toc-row.l2 { padding-left: 10px; }
  .toc-row.l3 { padding-left: 16px; font-size: 10.5px; }
  .toc-row.l4 { padding-left: 22px; font-size: 10.5px; opacity: 0.8; }
  .body { flex: 1; min-width: 0; overflow: auto; padding: 12px 14px 40px; user-select: text; display: flex; flex-direction: column; }
  .render {
    flex: 1; width: 100%; min-height: 480px; border: 1px solid var(--line-2, rgba(255,255,255,0.08));
    border-radius: 8px; background: transparent;
  }
  .prose { font-size: 13px; line-height: 1.6; color: var(--text-2, #d8dbe3); }
  .prose :global(h1) { font-size: 20px; margin: 0.2em 0 0.5em; letter-spacing: -0.3px; color: var(--text, #fff); }
  .prose :global(h2) { font-size: 16px; margin: 1.3em 0 0.4em; color: var(--text, #fff); }
  .prose :global(h3) { font-size: 14px; margin: 1.1em 0 0.3em; color: var(--text, #fff); }
  .prose :global(h4) { font-size: 13px; margin: 1em 0 0.3em; color: var(--text, #fff); }
  .prose :global(p) { margin: 0 0 0.8em; }
  .prose :global(a) { color: var(--accent, #7c8cff); }
  .prose :global(ul), .prose :global(ol) { padding-left: 1.4em; margin: 0 0 0.8em; }
  .prose :global(li) { margin: 0.15em 0; }
  .prose :global(blockquote) {
    margin: 0 0 0.8em; padding: 6px 12px; border-left: 3px solid var(--accent-line, rgba(124,140,255,0.4));
    background: var(--surface-1, rgba(255,255,255,0.04)); border-radius: 0 6px 6px 0;
  }
  .prose :global(table) { border-collapse: collapse; font-size: 12px; margin: 0 0 0.8em; max-width: 100%; }
  .prose :global(th), .prose :global(td) { border: 1px solid var(--line-2, rgba(255,255,255,0.1)); padding: 4px 8px; text-align: left; }
  .prose :global(th) { background: var(--surface-1, rgba(255,255,255,0.04)); }
  .prose :global(code) { font-size: 0.92em; }
  .prose :global(p code), .prose :global(li code) {
    background: var(--surface-2, rgba(255,255,255,0.07)); padding: 1px 5px; border-radius: 4px;
  }
  .prose :global(hr) { border: none; border-top: 1px solid var(--line-2, rgba(255,255,255,0.1)); margin: 1.2em 0; }
  .prose :global(img) { max-width: 100%; border-radius: 8px; }
  /* Whole document view: never clamp fenced blocks here. */
  .prose :global(.codeblock.clamped pre) { max-height: none; overflow: auto; }
  .prose :global(.codeblock.clamped pre::after) { display: none; }
  .prose :global(.code-expand) { display: none; }
  .prose.code :global(.codeblock) { border: none; background: transparent; }
  .prose.code :global(.codeblock .code-head) { display: none; }
  .prose.code :global(.codeblock pre) { padding: 0; }
  /* Deck codeblocks sit on the panel background, so the gutter must not
     paint its own solid boxes (that is the boxed-numbers look). Static
     positioning: numbers scroll with the code instead of overlaying it. */
  .prose.code :global(.cl-no) { background: transparent; position: static; }
</style>
