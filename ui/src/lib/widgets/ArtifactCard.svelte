<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { renderMarkdown } from "../md";
  import type { InspectorArtifact } from "../api";

  const dispatch = createEventDispatcher<{ openInDeck: { artifact: InspectorArtifact } }>();

  export let data: any;
  export let fallbackId = "";
  export let fallbackTitle = "";
  export let fallbackKind = "text";
  export let fallbackVersion = 1;

  const d = data ?? {};
  const id: string = String(d.id ?? fallbackId ?? "artifact");
  const title: string = String(d.title ?? fallbackTitle ?? id);
  const kind: string = String(d.kind ?? fallbackKind ?? "text");
  const version: number = Number(d.version ?? fallbackVersion ?? 1);
  const language: string = String(d.language ?? "");
  const content: string = String(d.content ?? "");

  const lines = content ? content.split("\n").length : 0;
  const bad = !content.trim();
  let expanded = false;
  let copied = false;

  function copy() {
    navigator.clipboard.writeText(content);
    copied = true;
    setTimeout(() => (copied = false), 1200);
  }

  function download() {
    const ext = kind === "code" && language ? language : kind === "markdown" ? "md" : kind === "svg" ? "svg" : kind === "html" ? "html" : "txt";
    const blob = new Blob([content], { type: "text/plain" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = `${id}-v${version}.${ext}`;
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 2000);
  }

  function openInDeck() {
    dispatch("openInDeck", { artifact: { id, title, kind, language, content, version } });
  }

  function kindIcon(k: string): string {
    if (k === "code") return "file";
    if (k === "markdown") return "md";
    if (k === "html" || k === "svg") return "eye";
    if (k === "diff") return "diff";
    return "doc";
  }
</script>

<div class="artifact-card" class:bad>
  <div class="art-head">
    <span class="art-ico {kindIcon(kind)}">
      {#if kindIcon(kind) === "file"}
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M14 3H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z" /><path d="M14 3v6h6" /></svg>
      {:else if kindIcon(kind) === "diff"}
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M12 3v18M3 12h18" /></svg>
      {:else if kindIcon(kind) === "eye"}
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7z" /><circle cx="12" cy="12" r="3" /></svg>
      {:else}
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M4 4h16v16H4z" /><path d="M8 9h8M8 13h8M8 17h5" /></svg>
      {/if}
    </span>
    <span class="art-title" title={id}>{title}</span>
    <span class="art-badge">{kind}{language ? ` · ${language}` : ""}</span>
    <span class="art-ver">v{version}</span>
    <span class="art-lines">{lines} lines</span>
    <span class="spacer" />
    <button class="mini-btn" on:click={copy}>{copied ? "copied" : "copy"}</button>
    <button class="mini-btn" on:click={download}>save</button>
    {#if !bad}
      <button class="mini-btn deck" title="Read side-by-side in the inspector deck" on:click={openInDeck}>open in deck ↗</button>
    {/if}
  </div>
  {#if bad}
    <div class="art-error">Couldn't render artifact — showing source.</div>
    <pre class="art-source">{JSON.stringify(d, null, 2)?.slice(0, 4000)}</pre>
  {:else if kind === "markdown"}
    <div class="art-md">{@html renderMarkdown(content)}</div>
  {:else}
    <div class="art-code" class:clamped={!expanded && lines > 30}>
      {@html renderMarkdown("```" + (language || kind) + "\n" + content.slice(0, 60000) + "\n```")}
    </div>
    {#if lines > 30}
      <button class="expand-btn" on:click={() => (expanded = !expanded)}>
        {expanded ? "collapse" : `show full (${lines} lines)`}
      </button>
    {/if}
  {/if}
</div>

<style>
  .artifact-card {
    background: var(--surface-1);
    border: 1px solid var(--line-2);
    border-radius: 12px;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .artifact-card.bad { border-color: var(--bad-line); }
  .art-head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--line-2);
    font-size: 12px;
  }
  .art-ico { display: flex; color: var(--text-3); }
  .art-title { font-weight: 600; color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 260px; }
  .art-badge, .art-ver, .art-lines {
    font-family: var(--parzi-mono), ui-monospace, monospace;
    font-size: 10px;
    color: var(--text-3);
    background: var(--surface-2);
    border-radius: 4px;
    padding: 2px 6px;
    white-space: nowrap;
  }
  .art-ver { color: var(--accent); }
  .spacer { flex: 1; }
  .mini-btn {
    background: var(--surface-2);
    border: none;
    border-radius: 4px;
    color: var(--text-3);
    font-size: 10px;
    padding: 3px 8px;
    cursor: pointer;
  }
  .mini-btn:hover { color: var(--text); background: var(--surface-3); }
  .mini-btn.deck { color: var(--accent); background: var(--accent-soft); white-space: nowrap; }
  .mini-btn.deck:hover { background: var(--accent-mid); color: var(--text); }
  .art-md { padding: 12px 14px; font-size: 13px; }
  .art-code { font-size: 11.5px; }
  .art-code.clamped :global(.codeblock.clamped) { max-height: 420px; }
  .art-error { padding: 8px 12px; font-size: 11px; color: var(--bad); }
  .art-source { margin: 0 12px 12px; padding: 8px; font-size: 10px; overflow: auto; background: var(--input); border-radius: 6px; }
  .expand-btn {
    background: transparent;
    border: none;
    border-top: 1px solid var(--line-2);
    color: var(--accent);
    font-size: 11px;
    padding: 6px;
    cursor: pointer;
  }
</style>
