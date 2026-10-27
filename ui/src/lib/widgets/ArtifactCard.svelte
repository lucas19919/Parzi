<script lang="ts">
  // What an agent hands over to keep (a document, code, a diagram, a
  // page): a quiet card in the thread. Its name, kind and version on top
  // with Copy and Save; the content below in the thread's own type, a long
  // one folded until asked for. An older version of the same artifact
  // folds to its header line. Open shows it in its own tab to read
  // (`reader`): the whole of it, in a centred column.
  import { onMount, tick } from "svelte";
  import { api } from "../api";
  import { codeBlockClick, mdHtml, richReady } from "../md";
  import { handleLinkClick } from "../links";
  import { previewSpecOf, previewProblem } from "../previewRegistry";
  import { saveFile } from "../saveFile";
  import { svgDataUrl } from "../svgImage";
  import { toast } from "../toast";
  import { tooltip } from "../tooltip";
  import { viewImage } from "../imageViewer";
  import Icon from "../Icon.svelte";
  import type { IconName } from "../icons";

  export let data: any;
  export let fallbackId = "";
  export let fallbackTitle = "";
  export let fallbackKind = "text";
  // A newer version follows further down: this one folds to its header.
  export let superseded = false;
  // Shown on its own in a tab: nothing folds, no card frame.
  export let reader = false;

  const d = data ?? {};
  const id: string = String(d.id ?? fallbackId ?? "artifact");
  const title: string = String(d.title || fallbackTitle || id);
  const kind: string = String(d.kind ?? fallbackKind ?? "text");
  const language: string = String(d.language ?? "");
  const content: string = String(d.content ?? "");
  const version: number = Number(d.version ?? 1) || 1;

  // The frame stays sandboxed (scripts, no same-origin). Its own CSP
  // keeps it offline; if the height script is still blocked the frame
  // scrolls instead of cutting tall artifacts off.
  const LOCAL_ONLY = `<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; media-src data: blob:; font-src data:">`;
  const RESIZE = `<script>function tell(){try{var h=document.documentElement.scrollHeight;parent.postMessage({parziArt:1,height:h},"*")}catch(e){}}addEventListener("load",tell);setTimeout(tell,400);setTimeout(tell,1500);<\/script>`;
  const MAX_FRAME = 4000;
  // Taller than this, a document or code folds behind Show all.
  const FOLD = 360;
  const bad = !content.trim() && kind !== "preview";

  const KINDS: Record<string, [string, IconName]> = {
    markdown: ["Document", "file"],
    text: ["Text", "file"],
    code: ["Code", "file"],
    diff: ["Changes", "file"],
    json: ["Data", "chart"],
    csv: ["Data", "chart"],
    svg: ["Diagram", "image"],
    image: ["Image", "image"],
    html: ["Page", "browser"],
    preview: ["Preview", "spark"],
  };
  const [kindWord, icon] = KINDS[kind] ?? ["File", "file"];
  const label = kind === "code" && language ? language : kindWord;

  // Inline design previews: { "component": "omnibar", "variant": 2 }.
  $: preview = kind === "preview" ? previewSpecOf(content) : null;
  $: previewProps = preview ? preview.entry.props(preview.variant, preview.extra) : {};
  $: canRender = (kind === "html" || kind === "svg") && !!content.trim();
  // SVG renders as an image, never inline: inline markup keeps its style
  // element, which would restyle the whole app.
  $: svgUrl = kind === "svg" && content.trim() ? svgImage(content) : "";
  $: imageUrl = kind === "image" && content.trim() ? api.readImageDataUrl(content, "") : null;
  let showCode = false;
  let frame: HTMLIFrameElement | null = null;
  let svgBroken = false;
  let open = reader || !superseded;
  let copied = false;

  // Folding: measured once the content is in.
  let body: HTMLDivElement | null = null;
  let tall = false;
  let all = false;
  // Code folds in its own block; a document folds here.
  $: foldable = !reader && kind === "markdown";
  async function measure() {
    await tick();
    tall = foldable && !!body && body.scrollHeight > FOLD + 40;
  }
  $: if (open && $richReady !== undefined) void measure();

  function svgImage(src: string): string {
    const svg = new DOMParser().parseFromString(src, "text/html").querySelector("svg");
    return svg ? svgDataUrl(new XMLSerializer().serializeToString(svg)) : "";
  }

  function openToRead() {
    document.dispatchEvent(
      new CustomEvent("parzi:open-artifact", { detail: { id, title, kind, language, content, version } }),
    );
  }

  function nameClick() {
    if (superseded && !reader) open = !open;
    else if (!reader && !bad) openToRead();
  }

  function codeClick(e: MouseEvent) {
    if (reader) codeBlockClick(e.target as HTMLElement);
  }

  async function docClick(e: MouseEvent) {
    if (await handleLinkClick(e, "", { pin: true })) return;
    // In the thread, the thread handles code blocks; alone, this does.
    if (reader) codeBlockClick(e.target as HTMLElement);
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
    const ext = kind === "code" && language ? language : kind === "markdown" ? "md" : kind === "svg" ? "svg" : kind === "html" ? "html" : kind === "json" ? "json" : kind === "csv" ? "csv" : "txt";
    void saveFile(`${title.replace(/\.[a-z0-9]{1,8}$/i, "") || id}.${ext}`, content);
  }

  onMount(() => {
    const onMsg = (e: MessageEvent) => {
      const el = frame;
      if (!el || e.source !== el.contentWindow) return;
      const h = (e.data as { parziArt?: unknown; height?: unknown } | null)?.height;
      if (typeof h === "number" && h > 0) el.style.height = `${Math.min(MAX_FRAME, Math.max(160, Math.round(h)))}px`;
    };
    window.addEventListener("message", onMsg);
    return () => window.removeEventListener("message", onMsg);
  });
</script>

<div class="art" class:bad class:reader class:old={superseded && !open}>
  <div class="head">
    <button class="name" aria-expanded={superseded ? open : undefined} disabled={reader || bad} on:click={nameClick}>
      <Icon name={icon} size={12} />
      {#if !reader}<span class="title">{title}</span>{/if}
      <span class="meta">{label}{version > 1 ? ` · v${version}` : ""}{superseded && !open ? " · updated below" : ""}</span>
    </button>
    <span class="tools">
      {#if canRender && open}
        <button class="tool text" on:click={() => (showCode = !showCode)}>{showCode ? "Preview" : "Source"}</button>
      {/if}
      {#if !reader && !bad}
        <button class="tool" aria-label="Open" use:tooltip={"Open to read"} on:click={openToRead}><Icon name="external" size={12} /></button>
      {/if}
      <button class="tool" aria-label="Copy" use:tooltip={copied ? "Copied" : "Copy"} on:click={copy}><Icon name={copied ? "check" : "copy"} size={12} /></button>
      <button class="tool" aria-label="Save" use:tooltip={"Save as a file"} on:click={download}><Icon name="download" size={12} /></button>
    </span>
  </div>
  {#if open}
    <div class="body" class:folded={tall && !all} bind:this={body}>
      {#if bad}
        <div class="err">Couldn't render this. Showing what came.</div>
        <pre class="source">{JSON.stringify(d, null, 2)?.slice(0, 4000)}</pre>
      {:else if kind === "preview" && preview}
        <div class="preview">
          <svelte:component this={preview.entry.component} {...previewProps} />
        </div>
      {:else if kind === "preview"}
        <div class="err">Unknown preview: {previewProblem(content)}.</div>
      {:else if kind === "markdown"}
        <!-- svelte-ignore a11y-no-static-element-interactions a11y-click-events-have-key-events -->
        <div class="doc msg" on:click={(e) => void docClick(e)}>{@html mdHtml(content, $richReady)}</div>
      {:else if kind === "svg" && !showCode && svgUrl && !svgBroken}
        <button class="zoom" aria-label="View {title} bigger" on:click={() => svgUrl && viewImage(svgUrl, title)}>
          <img src={svgUrl} alt={title} on:error={() => (svgBroken = true)} />
        </button>
      {:else if kind === "image" && imageUrl}
        {#await imageUrl then url}
          <button class="zoom" aria-label="View {title} bigger" on:click={() => viewImage(url, title)}>
            <img src={url} alt={title} />
          </button>
        {:catch e}
          <div class="err">Couldn't load the image: {e}</div>
        {/await}
      {:else if kind === "html" && canRender && !showCode}
        <iframe bind:this={frame} class="page" {title} sandbox="allow-scripts" scrolling="auto" srcdoc={LOCAL_ONLY + RESIZE + content}></iframe>
      {:else}
        {#if kind === "svg" && !showCode}
          <div class="err">Couldn't draw this SVG. Showing its source.</div>
        {/if}
        <!-- svelte-ignore a11y-no-static-element-interactions a11y-click-events-have-key-events -->
        <div class="code" on:click={codeClick}>
          {@html mdHtml("```" + (language || kind) + "\n" + content.slice(0, 60000) + "\n```", $richReady)}
        </div>
      {/if}
    </div>
    {#if tall}
      <button class="more" on:click={() => (all = !all)}>{all ? "Show less" : "Show all"}</button>
    {/if}
  {/if}
</div>

<style>
  .art {
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
  }
  /* Alone in a tab: the page scrolls, the content sits in a reading
     column, no frame around it. */
  .art.reader {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    background: none;
    border: none;
    border-radius: 0;
  }
  .reader .head,
  .reader .body {
    width: 100%;
    max-width: 820px;
    margin: 0 auto;
  }
  .art.reader .head {
    padding: 6px 12px 0 24px;
    border-bottom: none;
  }
  .reader .doc {
    padding: 4px 24px 64px;
    font-size: var(--fs-md);
    line-height: 1.65;
  }
  .reader .code {
    padding: 8px 12px 48px;
  }
  .reader .code :global(.codeblock) {
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--bg);
  }
  .art.bad {
    border-color: color-mix(in srgb, var(--bad) 50%, var(--line));
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 40px;
    padding: 0 8px 0 14px;
  }
  .art:not(.old) .head {
    border-bottom: 1px solid var(--line);
  }
  .name {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0;
    background: none;
    border: none;
    color: var(--text);
    font: inherit;
    text-align: left;
  }
  .name:not(:disabled) {
    cursor: pointer;
  }
  .name :global(svg) {
    flex: none;
    color: var(--muted);
  }
  .title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-md);
    font-weight: 600;
  }
  .meta {
    flex: none;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .tools {
    display: inline-flex;
    flex: none;
    gap: 8px;
  }
  .tool {
    min-width: 26px;
    height: 26px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0 6px;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .tool:hover {
    background: var(--hover);
    color: var(--text);
  }
  .body {
    position: relative;
  }
  .body.folded {
    max-height: 360px;
    overflow: hidden;
  }
  .body.folded::after {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 72px;
    background: linear-gradient(transparent, var(--panel));
    pointer-events: none;
  }
  /* A document reads like a reply, with room around it. */
  .doc {
    padding: 14px 18px 16px;
    font-size: var(--fs-base);
  }
  .doc :global(:first-child) {
    margin-top: 0;
  }
  /* Code sits flush in the card: no second frame inside the first. */
  .code :global(.codeblock) {
    margin: 0;
    border: none;
    border-radius: 0;
    background: transparent;
  }
  .preview {
    width: 100%;
    max-width: 720px;
    margin: 0 auto;
    padding: 12px 0 4px;
  }
  .zoom {
    display: block;
    width: 100%;
    padding: 12px;
    background: none;
    border: none;
    cursor: zoom-in;
  }
  .zoom img {
    display: block;
    width: 100%;
    height: auto;
    border-radius: var(--radius);
  }
  .page {
    display: block;
    width: 100%;
    height: 320px;
    min-height: 160px;
    border: none;
    background: transparent;
    overflow: auto;
  }
  .err {
    padding: 10px 14px 0;
    font-size: var(--fs-sm);
    color: var(--bad);
  }
  .source {
    margin: 8px 14px 14px;
    padding: 8px;
    overflow: auto;
    background: var(--bg);
    border-radius: var(--radius);
    font-size: var(--fs-xs);
  }
  .more {
    padding: 8px;
    background: none;
    border: none;
    border-top: 1px solid var(--line);
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .more:hover {
    background: var(--hover);
    color: var(--text);
  }
</style>
