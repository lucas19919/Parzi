<script lang="ts">
  import { onMount, tick } from "svelte";
  import { fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { api } from "../api";
  import { codeBlockClick, mdHtml, richReady } from "../md";
  import { handleLinkClick } from "../links";
  import { popover, placeAbove } from "../popover";
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
  export let superseded = false;
  export let reader = false;

  const d = data ?? {};
  const id: string = String(d.id ?? fallbackId ?? "artifact");
  const title: string = String(d.title || fallbackTitle || id);
  const kind: string = String(d.kind ?? fallbackKind ?? "text");
  const language: string = String(d.language ?? "");
  const content: string = String(d.content ?? "");
  const version: number = Number(d.version ?? 1) || 1;

  const LOCAL_ONLY = `<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; media-src data: blob:; font-src data:">`;
  const RESIZE = `<script>function tell(){try{var h=document.documentElement.scrollHeight;parent.postMessage({parziArt:1,height:h},"*")}catch(e){}}addEventListener("load",tell);setTimeout(tell,400);setTimeout(tell,1500);<\/script>`;
  const MAX_FRAME = 4000;
  const FOLD = 360;
  const bad = !content.trim() && kind !== "preview";

  const ICONS: Record<string, IconName> = {
    markdown: "file",
    text: "file",
    code: "file",
    diff: "file",
    json: "chart",
    csv: "chart",
    svg: "image",
    image: "image",
    html: "browser",
    preview: "spark",
  };
  const icon = ICONS[kind] ?? "file";

  $: preview = kind === "preview" ? previewSpecOf(content) : null;
  $: previewProps = preview ? preview.entry.props(preview.variant, preview.extra) : {};
  $: canRender = (kind === "html" || kind === "svg") && !!content.trim();
  $: svgUrl = kind === "svg" && content.trim() ? svgImage(content) : "";
  $: imageUrl = kind === "image" && content.trim() ? api.readImageDataUrl(content, "") : null;
  let showCode = false;
  let frame: HTMLIFrameElement | null = null;
  let svgBroken = false;
  let open = reader || !superseded;
  let copied = false;

  let body: HTMLDivElement | null = null;
  let tall = false;
  let all = false;
  let menuOpen = false;
  let menuBtn: HTMLButtonElement | null = null;
  let menuStyle = "";

  function toggleMenu() {
    menuOpen = !menuOpen;
    if (menuOpen && menuBtn) menuStyle = placeAbove(menuBtn, 220);
  }

  function act(fn: () => void) {
    menuOpen = false;
    fn();
  }
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
    if (await handleLinkClick(e, "")) return;
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
      {#if superseded && !open}<span class="meta">updated below</span>{/if}
    </button>
    <span class="tools">
      <button class="tool" aria-label="More actions" use:tooltip={"More actions"} on:click={toggleMenu} bind:this={menuBtn}><Icon name="more" size={12} /></button>
    </span>
  </div>
  {#if menuOpen}
    <div class="menu-pop" data-ui-list style={menuStyle} use:popover={{ anchor: menuBtn, close: () => (menuOpen = false) }} transition:fly={{ y: 6, duration: 140, easing: cubicOut }}>
      {#if canRender && open}
        <button class="opt" on:click={() => act(() => (showCode = !showCode))}><Icon name="file" size={12} /><span class="name">{showCode ? "Preview" : "Source"}</span></button>
      {/if}
      {#if !reader && !bad}
        <button class="opt" on:click={() => act(openToRead)}><Icon name="external" size={12} /><span class="name">Open to read</span></button>
      {/if}
      <button class="opt" on:click={() => act(copy)}><Icon name={copied ? "check" : "copy"} size={12} /><span class="name">{copied ? "Copied" : "Copy"}</span></button>
      <button class="opt" on:click={() => act(download)}><Icon name="download" size={12} /><span class="name">Save as file</span></button>
    </div>
  {/if}
  {#if open}
    <div class="body" class:folded={tall && !all} bind:this={body}>
      {#if bad}
        <div class="err">Couldn't render this. Showing what came.</div>
        <pre class="source">{JSON.stringify(d, null, 2)?.slice(0, 4000)}</pre>
      {:else if kind === "preview" && preview}
        <div class="preview" style={preview.style}>
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
    padding: calc(var(--space) * 2) calc(var(--space) * 3) 0 calc(var(--space) * 6);
    border-bottom: none;
  }
  .reader .doc {
    padding: var(--space) calc(var(--space) * 6) calc(var(--space) * 8);
    font-size: var(--fs-md);
    line-height: 1.65;
  }
  .reader .code {
    padding: calc(var(--space) * 2) calc(var(--space) * 3) calc(var(--space) * 8);
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
    gap: calc(var(--space) * 2);
    min-height: 40px;
    padding: 0 calc(var(--space) * 2) 0 calc(var(--space) * 4);
  }
  .art:not(.old):not(.reader) .head {
    border-bottom: 1px solid var(--line);
  }
  .name {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
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
    gap: calc(var(--space) * 2);
  }
  .tool {
    min-width: 26px;
    height: 26px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0 calc(var(--space) * 2);
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
  .menu-pop {
    width: 220px;
    padding: var(--space);
    overflow-y: auto;
  }
  .opt {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    width: 100%;
    height: 32px;
    padding: 0 calc(var(--space) * 2);
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--text);
    text-align: left;
    cursor: pointer;
  }
  .opt:hover {
    background: var(--hover);
  }
  .opt .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-md);
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
  .doc {
    padding: calc(var(--space) * 4) calc(var(--space) * 4) calc(var(--space) * 4);
    font-size: var(--fs-base);
  }
  .doc :global(:first-child) {
    margin-top: 0;
  }
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
    padding: calc(var(--space) * 3) 0 var(--space);
  }
  .zoom {
    display: block;
    width: 100%;
    padding: calc(var(--space) * 3);
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
    padding: calc(var(--space) * 3) calc(var(--space) * 4) 0;
    font-size: var(--fs-sm);
    color: var(--bad);
  }
  .source {
    margin: calc(var(--space) * 2) calc(var(--space) * 4) calc(var(--space) * 4);
    padding: calc(var(--space) * 2);
    overflow: auto;
    background: var(--bg);
    border-radius: var(--radius);
    font-size: var(--fs-xs);
  }
  .more {
    padding: calc(var(--space) * 2);
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
