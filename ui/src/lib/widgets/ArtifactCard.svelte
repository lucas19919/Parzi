<script lang="ts">
  import { renderMarkdown } from "../md";
  import { handleLinkClick } from "../links";

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

  const LOCAL_ONLY = `<meta http-equiv="Content-Security-Policy" content="img-src data: blob:; media-src data: blob:; connect-src 'none'">`;
  const lines = content ? content.split("\n").length : 0;
  const bad = !content.trim();
  let expanded = false;
  let copied = false;
  $: canRender = (kind === "html" || kind === "svg") && !!content.trim();
  let showCode = false;

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
  </div>
  {#if bad}
    <div class="art-error">Couldn't render artifact — showing source.</div>
    <pre class="art-source">{JSON.stringify(d, null, 2)?.slice(0, 4000)}</pre>
  {:else if kind === "markdown"}
    <!-- svelte-ignore a11y-no-static-element-interactions a11y-click-events-have-key-events -->
    <div class="art-md" on:click={(e) => void handleLinkClick(e)}>{@html renderMarkdown(content)}</div>
  {:else if canRender && !showCode}
    <iframe class="art-render" {title} sandbox="allow-scripts" srcdoc={LOCAL_ONLY + content}></iframe>
    <div class="art-foot">
      <button class="expand-btn" on:click={() => (showCode = true)}>show source</button>
    </div>
  {:else}
    <div class="art-code" class:clamped={!expanded && lines > 30}>
      {@html renderMarkdown("```" + (language || kind) + "\n" + content.slice(0, 60000) + "\n```")}
    </div>
    {#if lines > 30}
      <button class="expand-btn" on:click={() => (expanded = !expanded)}>
        {expanded ? "collapse" : `show full (${lines} lines)`}
      </button>
    {/if}
    {#if canRender && showCode}
      <button class="expand-btn" on:click={() => (showCode = false)}>show preview</button>
    {/if}
  {/if}
</div>

<style>
  .artifact-card {
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 12px;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .artifact-card.bad { border-color: var(--bad); }
  .art-head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--line);
    font-size: 12px;
  }
  .art-ico { display: flex; color: var(--muted); }
  .art-title { font-weight: 600; color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 260px; }
  .art-badge, .art-ver, .art-lines {
    font-family: var(--mono), ui-monospace, monospace;
    font-size: 10px;
    color: var(--muted);
    background: var(--line);
    border-radius: 4px;
    padding: 2px 6px;
    white-space: nowrap;
  }
  .art-ver { color: var(--accent); }
  .spacer { flex: 1; }
  .mini-btn {
    background: var(--line);
    border: none;
    border-radius: 4px;
    color: var(--muted);
    font-size: 10px;
    padding: 3px 8px;
    cursor: pointer;
  }
  .mini-btn:hover { color: var(--text); background: var(--line); }
  .art-md { padding: 12px 14px; font-size: 13px; }
  .art-render { width: 100%; min-height: 320px; border: none; background: transparent; }
  .art-foot { border-top: 1px solid var(--line); }
  .art-code { font-size: 11.5px; }
  .art-code.clamped :global(.codeblock.clamped) { max-height: 420px; }
  .art-error { padding: 8px 12px; font-size: 11px; color: var(--bad); }
  .art-source { margin: 0 12px 12px; padding: 8px; font-size: 10px; overflow: auto; background: var(--bg); border-radius: 6px; }
  .expand-btn {
    background: transparent;
    border: none;
    border-top: 1px solid var(--line);
    color: var(--accent);
    font-size: 11px;
    padding: 6px;
    cursor: pointer;
  }
</style>
