<script lang="ts" context="module">
  import type { ArtifactDoc } from "./tabs";

  export const FILE_MIN = 220;
  export const FILE_MAX = 640;
  const MD = /\.(md|markdown)$/i;
  const ABSOLUTE = /^([a-zA-Z]:[\\/]|\/|\\\\)/;

  /** A file read from disk as a card document: markdown renders, the rest stays plain. */
  export function fileDoc(id: string, content: string): ArtifactDoc {
    const name = id.split(/[\\/]/).pop() || id;
    const md = MD.test(name);
    return { id, title: md ? name.replace(MD, "") || name : name, kind: md ? "markdown" : "text", content, version: 1 };
  }
</script>

<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { cubicOut } from "svelte/easing";
  import Icon from "./Icon.svelte";
  import ResizeGrip from "./ResizeGrip.svelte";
  import { api, brain } from "./api";
  import { saveFile } from "./saveFile";
  import { codeBlockClick, mdHtml, richReady } from "./md";
  import { handleLinkClick } from "./links";
  import { toast, toastError } from "./toast";
  import { tooltip } from "./tooltip";

  /** The side bar's second column: a file or note opened from the Files
   *  card, the thread or an agent, read at a width of its own beside the
   *  cards. Closing it folds the side bar back to one column. */
  export let doc: ArtifactDoc;
  export let folder = "";
  export let width = 420;

  const dispatch = createEventDispatcher<{ close: void; resize: { width: number } }>();

  let copied = false;

  /** Opens and closes by its width, as the side bar does, so the thread
   *  narrows smoothly instead of jumping. */
  function grow(node: HTMLElement) {
    const w = node.offsetWidth;
    const gap = parseFloat(getComputedStyle(node).marginRight) || 0;
    return {
      duration: 260,
      easing: cubicOut,
      css: (t: number) => `width: ${t * w}px; margin-right: ${t * gap}px; opacity: ${t}; overflow: hidden;`,
    };
  }

  $: markdown = !!doc && (doc.kind === "markdown" || doc.kind === "md");
  // A file's id is its absolute path, a note's id its vault path.
  $: revealable = !!doc && /[\\/]/.test(doc.id);
  $: onDisk = !!doc && ABSOLUTE.test(doc.id);
  $: isNote = !!doc && !onDisk && MD.test(doc.id);
  // Relative links resolve beside the open file or note, not at the project root.
  $: linkFolder = onDisk ? doc!.id.replace(/[\\/][^\\/]*$/, "") : folder;
  $: noteDir = isNote ? doc!.id.replace(/\\/g, "/").replace(/\/?[^/]*$/, "") : null;

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
    void saveFile(markdown ? `${doc.title || "note"}.md` : doc.title || "file.txt", doc.content);
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

<aside class="reader" aria-label="File" style="width: {width}px" transition:grow>
  <ResizeGrip label="Resize file" min={FILE_MIN} max={FILE_MAX} on:resize />
  <section class="card">
    <div class="bar">
      <span class="title t-section" use:tooltip={doc.id}>{doc.title}</span>
      <span class="actions">
        <button class="icon-btn" aria-label={copied ? "Copied" : "Copy"} use:tooltip={copied ? "Copied" : "Copy"} on:click={copy}><Icon name={copied ? "check" : "copy"} /></button>
        <button class="icon-btn" aria-label="Save as file" use:tooltip={"Save as file"} on:click={save}><Icon name="download" /></button>
        {#if revealable}
          <button class="icon-btn" aria-label="Open in file explorer" use:tooltip={"Open in file explorer"} on:click={reveal}><Icon name="folder" /></button>
        {/if}
        <button class="icon-btn" aria-label="Close file" use:tooltip={"Close"} on:click={() => dispatch("close")}><Icon name="close" /></button>
      </span>
    </div>
    <!-- svelte-ignore a11y-no-static-element-interactions a11y-click-events-have-key-events -->
    <div class="doc msg" on:click={onDocClick}>
      {#key `${doc.id}:${doc.version}:${$richReady}`}
        {#if markdown}
          {@html mdHtml(doc.content, $richReady)}
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
  .card {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: color-mix(in srgb, var(--panel) var(--card-fill), transparent);
    border: 1px solid color-mix(in srgb, var(--line) var(--card-edge), transparent);
    -webkit-backdrop-filter: var(--card-blur);
    backdrop-filter: var(--card-blur);
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
</style>
