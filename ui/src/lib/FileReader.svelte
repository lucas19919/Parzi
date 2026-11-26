<script lang="ts" context="module">
  import type { ArtifactDoc } from "./tabs";

  export const FILE_MIN = 220;
  export const FILE_MAX = 960;
  const MD = /\.(md|markdown)$/i;
  const IMAGE = /\.(png|jpe?g|gif|webp|bmp|svg|avif|ico)$/i;
  const ABSOLUTE = /^([a-zA-Z]:[\\/]|\/|\\\\)/;
  const LANG_BY_EXT: Record<string, string> = {
    rs: "rust", ts: "typescript", mts: "typescript", cts: "typescript", tsx: "tsx",
    js: "javascript", mjs: "javascript", cjs: "javascript", jsx: "jsx",
    svelte: "svelte", html: "html", htm: "html", xml: "xml", svg: "svg",
    css: "css", scss: "scss", py: "python", json: "json", jsonc: "jsonc",
    toml: "toml", ini: "ini", cfg: "ini", yaml: "yaml", yml: "yaml",
    sh: "bash", bash: "bash", zsh: "zsh", ps1: "powershell",
    sql: "sql", go: "go", c: "c", h: "c", cpp: "cpp", cc: "cpp", hpp: "cpp",
    diff: "diff", patch: "diff",
  };

  export function langFor(id: string): string {
    const leaf = id.split(/[\\/]/).pop() ?? "";
    const dot = leaf.lastIndexOf(".");
    if (dot <= 0) return "";
    return LANG_BY_EXT[leaf.slice(dot + 1).toLowerCase()] ?? "";
  }

  /** A file read from disk as a card document: markdown renders, images
   *  show, known code files syntax-highlight, the rest stays plain. */
  export function fileDoc(id: string, content: string): ArtifactDoc {
    const name = id.split(/[\\/]/).pop() || id;
    const md = MD.test(name);
    const language = md ? "" : langFor(name);
    const kind = md ? "markdown" : language === "diff" ? "diff" : language ? "code" : "text";
    return { id, title: md ? name.replace(MD, "") || name : name, kind, language: language || undefined, content, version: 1 };
  }

  export function isImagePath(id: string): boolean {
    return IMAGE.test(id.split(/[\\/]/).pop() || id);
  }

  export function imageDoc(id: string): ArtifactDoc {
    const name = id.split(/[\\/]/).pop() || id;
    return { id, title: name, kind: "image", content: id, version: 1 };
  }
</script>

<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { cubicOut } from "svelte/easing";
  import Icon from "./Icon.svelte";
  import ResizeGrip from "./ResizeGrip.svelte";
  import { api, brain } from "./api";
  import { saveFile } from "./saveFile";
  import { codeBlockClick, fenced, mdHtml, richReady } from "./md";
  import { handleLinkClick } from "./links";
  import { toast, toastError } from "./toast";
  import { tooltip } from "./tooltip";
  import { viewImage } from "./imageViewer";

  /** The side bar's second column: a file or note opened from the Files
   *  card, the thread or an agent, read at a width of its own beside the
   *  cards. Closing it folds the side bar back to one column. */
  export let doc: ArtifactDoc;
  export let folder = "";
  export let width = 420;
  /** Full screen over the thread (a pin or a link opened it): no grip, and
   *  the back arrow above closes it instead of its own button. */
  export let full = false;

  const dispatch = createEventDispatcher<{ close: void; resize: { width: number } }>();

  let copied = false;

  /** Opens and closes by its width, as the side bar does, so the thread
   *  narrows smoothly instead of jumping. */
  function grow(node: HTMLElement) {
    if (full) return { duration: 0 };
    const w = node.offsetWidth;
    const gap = parseFloat(getComputedStyle(node).marginRight) || 0;
    return {
      duration: 260,
      easing: cubicOut,
      css: (t: number) => `width: ${t * w}px; margin-right: ${t * gap}px; opacity: ${t}; overflow: hidden;`,
    };
  }

  $: markdown = !!doc && (doc.kind === "markdown" || doc.kind === "md");
  $: image = !!doc && doc.kind === "image";
  $: diff = !!doc && doc.kind === "diff";
  // A file's id is its absolute path, a note's id its vault path.
  $: revealable = !!doc && /[\\/]/.test(doc.id);
  $: onDisk = !!doc && ABSOLUTE.test(doc.id);
  $: isNote = !!doc && !onDisk && MD.test(doc.id);
  // Relative links resolve beside the open file or note, not at the project root.
  $: linkFolder = onDisk ? doc!.id.replace(/[\\/][^\\/]*$/, "") : folder;
  $: noteDir = isNote ? doc!.id.replace(/\\/g, "/").replace(/\/?[^/]*$/, "") : null;
  $: imgKey = image ? `${doc!.id}:${doc!.version}` : "";
  $: if (imgKey && imgKey !== imgLoaded) {
    imgLoaded = imgKey;
    imgUrl = null;
    imgErr = "";
    void api
      .readImageDataUrl(doc!.content || doc!.id, linkFolder)
      .then((u) => {
        if (imgKey === imgLoaded) imgUrl = u;
      })
      .catch((e) => {
        if (imgKey === imgLoaded) imgErr = String(e);
      });
  }
  let imgUrl: string | null = null;
  let imgErr = "";
  let imgLoaded = "";

  function copy() {
    if (!doc) return;
    navigator.clipboard.writeText(doc.content).then(
      () => {
        copied = true;
        setTimeout(() => (copied = false), 1200);
      },
      (e) => toast(`Couldn't copy: ${e}`, true),
    );
  }

  function save() {
    if (!doc) return;
    // Markdown titles drop ".md"; other files keep their own name.
    const name = markdown ? `${doc.title || "note"}.md` : diff ? `${doc.title || "changes"}.diff` : doc.title || "file.txt";
    void saveFile(name, doc.content);
  }

  function reveal() {
    if (!doc) return;
    if (onDisk) void api.openFilePath(linkFolder || doc.id).catch(toastError);
    else void brain.open(doc.id, "folder").catch(toastError);
  }

  function onDocClick(e: MouseEvent) {
    void (async () => {
      if (await handleLinkClick(e, linkFolder, noteDir)) return;
      codeBlockClick(e.target as HTMLElement);
    })();
  }
</script>

<aside class="reader" class:full aria-label="File" style={full ? "" : `width: ${width}px`} transition:grow>
  {#if !full}<ResizeGrip label="Resize file" min={FILE_MIN} max={FILE_MAX} on:resize />{/if}
  <section class="card">
    <div class="bar">
      <span class="title t-section" use:tooltip={doc.id}>{doc.title}</span>
      <span class="actions">
        {#if !image}
          <button class="icon-btn" aria-label={copied ? "Copied" : "Copy"} use:tooltip={copied ? "Copied" : "Copy"} on:click={copy}><Icon name={copied ? "check" : "copy"} /></button>
          <button class="icon-btn" aria-label="Save as file" use:tooltip={"Save as file"} on:click={save}><Icon name="download" /></button>
        {/if}
        {#if revealable}
          <button class="icon-btn" aria-label="Open in file explorer" use:tooltip={"Open in file explorer"} on:click={reveal}><Icon name="folder" /></button>
        {/if}
        {#if !full}
          <button class="icon-btn" aria-label="Close file" use:tooltip={"Close"} on:click={() => dispatch("close")}><Icon name="close" /></button>
        {/if}
      </span>
    </div>
    <!-- svelte-ignore a11y-no-static-element-interactions a11y-click-events-have-key-events -->
    <div class="doc msg" on:click={onDocClick}>
      {#key `${doc.id}:${doc.version}:${$richReady}`}
        {#if image}
          {#if imgUrl}
            <!-- svelte-ignore a11y-click-events-have-key-events a11y-no-noninteractive-element-interactions -->
            <img class="photo" src={imgUrl} alt={doc.title} title={"View full size"} on:click={() => viewImage(imgUrl ?? "", doc.title)} />
          {:else if imgErr}
            <p class="empty">Couldn't show that image: {imgErr}</p>
          {:else}
            <p class="empty">Loading image…</p>
          {/if}
        {:else if markdown}
          {@html mdHtml(doc.content, $richReady)}
        {:else if diff}
          {@html mdHtml(fenced(doc.content, "diff"), $richReady)}
        {:else if doc.language || langFor(doc.id)}
          {@html mdHtml(fenced(doc.content, doc.language || langFor(doc.id)), $richReady)}
        {:else}
          <pre class="plain">{doc.content}</pre>
        {/if}
      {/key}
    </div>
  </section>
</aside>

<style>
  /* Sits between the thread and the cards, the side bar's own frame: the
     grip just left of it, a full-height card. */
  .reader {
    position: relative;
    flex: none;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    margin: 0 var(--gap) calc(var(--space) * 3) 0;
  }
  /* Full screen the session's own card is the frame: the file reads in
     the thread's column. */
  .reader.full {
    flex: 1;
    margin: 0;
  }
  .reader.full .card {
    background: none;
    border: none;
    -webkit-backdrop-filter: none;
    backdrop-filter: none;
  }
  .reader.full .doc {
    width: min(var(--read), 100%);
    margin: 0 auto;
    padding-bottom: calc(var(--dock) + var(--space) * 8);
  }
  .card {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: var(--surface);
    border: 1px solid var(--edge);
    -webkit-backdrop-filter: var(--blur);
    backdrop-filter: var(--blur);
    border-radius: var(--radius);
  }
  .bar {
    flex: none;
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    height: var(--h-row);
    padding: 0 var(--space) 0 calc(var(--space) * 3);
  }
  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-sm);
  }
  .actions {
    flex: none;
    display: inline-flex;
    gap: var(--space);
  }
  .doc {
    flex: 1;
    min-width: 0;
    min-height: 0;
    overflow: auto;
    padding: 0 calc(var(--space) * 4) calc(var(--space) * 4);
    overflow-wrap: break-word;
    word-break: break-word;
  }
  .doc :global(.codeblock),
  .doc :global(.md-table),
  .doc :global(pre) {
    max-width: 100%;
  }
  .plain {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font: inherit;
    font-size: var(--fs-md);
  }
  .photo {
    display: block;
    max-width: 100%;
    height: auto;
    border-radius: var(--radius-in);
    cursor: zoom-in;
  }
  .empty {
    margin: calc(var(--space) * 2) 0;
    color: var(--muted);
    font-size: var(--fs-sm);
  }
</style>
