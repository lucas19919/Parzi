<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "../api";
  import { mdHtml, richReady } from "../md";
  import { handleLinkClick } from "../links";
  import { previewSpecOf, previewProblem } from "../previewRegistry";
  import { svgDataUrl } from "../svgImage";
  import { toast } from "../toast";

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

  // The frame stays sandboxed (scripts, no same-origin). Its own CSP
  // keeps it offline; if the height script is still blocked the frame
  // scrolls instead of cutting tall artifacts off.
  const LOCAL_ONLY = `<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; media-src data: blob:; font-src data:">`;
  const RESIZE = `<script>function tell(){try{var h=document.documentElement.scrollHeight;parent.postMessage({parziArt:1,height:h},"*")}catch(e){}}addEventListener("load",tell);setTimeout(tell,400);setTimeout(tell,1500);<\/script>`;
  const MAX_FRAME = 4000;
  const lines = content ? content.split("\n").length : 0;
  const bad = !content.trim() && kind !== "preview";
  let expanded = false;
  let copied = false;
  // Inline design previews: { "component": "omnibar", "variant": 2 }.
  // Components resolve through previewRegistry — one entry per component,
  // no hardcoded branches here. Extra keys merge over demo defaults.
  $: preview = kind === "preview" ? previewSpecOf(content) : null;
  $: previewProps = preview ? preview.entry.props(preview.variant, preview.extra) : {};
  $: canRender = (kind === "html" || kind === "svg") && !!content.trim();
  // SVG renders as an image, never inline: inline markup keeps its style
  // element, which would restyle the whole app. The browser's lenient
  // HTML parse + XML serialize turns sloppy agent SVG into a valid file.
  // HTML stays in the sandboxed frame since it can carry live scripts.
  $: svgUrl = kind === "svg" && content.trim() ? svgImage(content) : "";
  $: imageUrl = kind === "image" && content.trim() ? api.readImageDataUrl(content, "") : null;
  let showCode = false;
  let frame: HTMLIFrameElement | null = null;

  let svgBroken = false;

  function svgImage(src: string): string {
    const svg = new DOMParser().parseFromString(src, "text/html").querySelector("svg");
    return svg ? svgDataUrl(new XMLSerializer().serializeToString(svg)) : "";
  }

  function copy() {
    navigator.clipboard.writeText(content).then(
      () => {
        copied = true;
        setTimeout(() => (copied = false), 1200);
      },
      (e) => toast(`Couldn't copy: ${e}`, true),
    );
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
      if (typeof h === "number" && h > 0) frame.style.height = `${Math.min(MAX_FRAME, Math.max(160, Math.round(h)))}px`;
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
  {:else if kind === "preview" && preview}
    <div class="art-preview">
      <svelte:component this={preview.entry.component} {...previewProps} />
    </div>
  {:else if kind === "preview"}
    <div class="art-error">Unknown preview — {previewProblem(content)}.</div>
  {:else if kind === "markdown"}
    <!-- svelte-ignore a11y-no-static-element-interactions a11y-click-events-have-key-events -->
    <div class="art-md" on:click={(e) => void handleLinkClick(e)}>{@html mdHtml(content, $richReady)}</div>
  {:else if kind === "svg" && !showCode && svgUrl && !svgBroken}
    <img class="art-svg" src={svgUrl} alt={title} on:error={() => (svgBroken = true)} />
  {:else if kind === "image" && imageUrl}
    {#await imageUrl then url}
      <img class="art-img" src={url} alt={title} />
    {:catch e}
      <div class="art-error">Couldn't load the image: {e}</div>
    {/await}
  {:else if kind === "html" && canRender && !showCode}
    <iframe bind:this={frame} class="art-render" {title} sandbox="allow-scripts" scrolling="auto" srcdoc={LOCAL_ONLY + RESIZE + content}></iframe>
  {:else}
    {#if kind === "svg" && !showCode}
      <div class="art-error">Couldn't draw this SVG. Showing its source.</div>
    {/if}
    <div class="art-code" class:clamped={!expanded && lines > 30}>
      {@html mdHtml("```" + (language || kind) + "\n" + content.slice(0, 60000) + "\n```", $richReady)}
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
  .art-preview {
    width: 100%;
    max-width: 720px;
    margin: 0 auto;
    padding: 12px 0 4px;
  }
  .art-svg { width: 100%; height: auto; display: block; }
  .art-img { width: 100%; height: auto; display: block; border-radius: 8px; }
  .art-render { width: 100%; height: 320px; min-height: 160px; border: none; background: transparent; display: block; overflow: auto; }
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
