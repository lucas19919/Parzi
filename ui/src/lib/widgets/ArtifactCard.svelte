<script lang="ts">
  import { onMount } from "svelte";
  import { renderMarkdown } from "../md";
  import { handleLinkClick } from "../links";

  export let data: any;
  export let fallbackId = "";
  export let fallbackTitle = "";
  export let fallbackKind = "text";

  const d = data ?? {};
  const id: string = String(d.id ?? fallbackId ?? "artifact");
  const title: string = String(d.title ?? fallbackTitle ?? id);
  const kind: string = String(d.kind ?? fallbackKind ?? "text");
  const language: string = String(d.language ?? "");
  const content: string = String(d.content ?? "");

  const LOCAL_ONLY = `<meta http-equiv="Content-Security-Policy" content="img-src data: blob:; media-src data: blob:; connect-src 'none'">`;
  const RESIZE = `<script>function tell(){var h=document.documentElement.scrollHeight;parent.postMessage({parziArt:1,height:h},"*")}addEventListener("load",tell);setTimeout(tell,400);<\/script>`;
  const lines = content ? content.split("\n").length : 0;
  const bad = !content.trim();
  let expanded = false;
  let copied = false;
  $: canRender = (kind === "html" || kind === "svg") && !!content.trim();
  let showCode = false;
  let frame: HTMLIFrameElement | null = null;

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
    a.download = `${id}.${ext}`;
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 2000);
  }

  onMount(() => {
    const onMsg = (e: MessageEvent) => {
      if (!frame || e.source !== frame.contentWindow) return;
      const h = (e.data as { parziArt?: unknown; height?: unknown } | null)?.height;
      if (typeof h === "number" && h > 0) frame.style.height = `${Math.max(160, Math.round(h))}px`;
    };
    window.addEventListener("message", onMsg);
    return () => window.removeEventListener("message", onMsg);
  });
</script>

<div class="artifact-card" class:bad title={title}>
  <div class="art-tools">
    {#if canRender}
      <button class="mini-btn" title={showCode ? "Show preview" : "Show source"} on:click={() => (showCode = !showCode)}>
        {showCode ? "preview" : "source"}
      </button>
    {/if}
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
    <iframe bind:this={frame} class="art-render" {title} sandbox="allow-scripts" srcdoc={LOCAL_ONLY + RESIZE + content}></iframe>
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
    position: relative;
    background: transparent;
    border: none;
    border-radius: 0;
    overflow: visible;
    display: flex;
    flex-direction: column;
  }
  .artifact-card.bad { border: 1px solid var(--bad); border-radius: 12px; }
  .art-tools {
    position: absolute;
    top: 6px;
    right: 6px;
    z-index: 5;
    display: flex;
    gap: 4px;
    opacity: 0;
    transition: opacity 140ms ease;
  }
  .artifact-card:hover .art-tools,
  .art-tools:focus-within {
    opacity: 1;
  }
  .mini-btn {
    background: color-mix(in srgb, var(--bg) 82%, transparent);
    border: 1px solid var(--line);
    border-radius: 6px;
    color: var(--muted);
    font-size: 10px;
    padding: 3px 8px;
    cursor: pointer;
    backdrop-filter: blur(6px);
  }
  .mini-btn:hover { color: var(--text); }
  .art-md { padding: 2px 0; font-size: 13px; }
  .art-render { width: 100%; min-height: 320px; border: none; background: transparent; display: block; }
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
