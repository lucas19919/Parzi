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
  import { open as pickPath } from "@tauri-apps/plugin-dialog";
  import InfoCard, { unfold } from "./InfoCard.svelte";
  import Icon from "./Icon.svelte";
  import ResizeGrip from "./ResizeGrip.svelte";
  import { api, brain } from "./api";
  import { saveFile } from "./saveFile";
  import { codeBlockClick, mdHtml, richReady } from "./md";
  import { handleLinkClick } from "./links";
  import { toast, toastError } from "./toast";
  import { tooltip } from "./tooltip";

  export let doc: ArtifactDoc | null = null;
  export let folder = "";
  export let width = 420;
  export let shown = true;

  const dispatch = createEventDispatcher<{
    close: void;
    pick: ArtifactDoc;
  }>();

  let copied = false;
  let picker: HTMLInputElement | null = null;

  $: markdown = !!doc && (doc.kind === "markdown" || doc.kind === "md");
  // A file's id is its absolute path, a note's id its vault path.
  $: revealable = !!doc && /[\\/]/.test(doc.id);
  $: onDisk = !!doc && ABSOLUTE.test(doc.id);
  $: isNote = !!doc && !onDisk && MD.test(doc.id);
  // Relative links resolve beside the open file or note, not at the project root.
  $: linkFolder = onDisk ? doc!.id.replace(/[\\/][^\\/]*$/, "") : folder;
  $: noteDir = isNote ? doc!.id.replace(/\\/g, "/").replace(/\/?[^/]*$/, "") : null;
  $: if (doc) unfold("reader");

  const show = (id: string, content: string) => dispatch("pick", fileDoc(id, content));

  async function browse() {
    // The dev mock has no system dialog, so it uses the browser's picker.
    if (location.pathname.endsWith("mock.html")) return picker?.click();
    try {
      const picked = await pickPath({ multiple: false, directory: false, defaultPath: folder || undefined, title: "Open file" });
      if (typeof picked === "string" && picked) show(picked, await api.readTextFile(picked));
    } catch (err) {
      toast(`Couldn't open that file: ${err}`, true);
    }
  }

  async function onPick(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    if (file.size > 1_000_000) return toast("That file is too large to open here", true);
    try {
      show(file.name, await file.text());
    } catch (err) {
      toast(`Couldn't read that file: ${err}`, true);
    }
  }

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

<aside class="files" class:shut={!shown} class:reading={!!doc} aria-label="Files" style="width: {shown ? width : 0}px" inert={!shown}>
  <ResizeGrip label="Resize files" min={FILE_MIN} max={FILE_MAX} on:resize />
  <div class="col">
  <InfoCard id="reader" title={doc?.title || "File"}>
    <svelte:fragment slot="actions">
      {#if doc}
        <button class="icon-btn" aria-label="Open file" use:tooltip={"Open file"} on:click={browse}><Icon name="file" /></button>
        <button class="icon-btn" aria-label={copied ? "Copied" : "Copy"} use:tooltip={copied ? "Copied" : "Copy"} on:click={copy}><Icon name={copied ? "check" : "copy"} /></button>
        <button class="icon-btn" aria-label="Save as file" use:tooltip={"Save as file"} on:click={save}><Icon name="download" /></button>
        {#if revealable}
          <button class="icon-btn" aria-label="Open in file explorer" use:tooltip={"Open in file explorer"} on:click={reveal}><Icon name="folder" /></button>
        {/if}
        <button class="icon-btn" aria-label="Close file" use:tooltip={"Close file"} on:click={() => dispatch("close")}><Icon name="close" /></button>
      {/if}
    </svelte:fragment>
    {#if doc}
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
    {:else}
      <button class="btn" type="button" on:click={browse}>Select a file</button>
    {/if}
  </InfoCard>
  </div>
  <input class="pick" bind:this={picker} type="file" tabindex="-1" aria-hidden="true" on:change={onPick} />
</aside>

<style>
  /* The column itself must not clip: the resize grip sits just left of it. */
  .files {
    position: relative;
    flex: none;
    align-self: stretch;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    margin: 0 var(--gap) 0 0;
    background: none;
    transition: width 300ms var(--ease-out), margin-right 300ms var(--ease-out);
  }
  .col {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    padding-bottom: calc(var(--space) * 3);
    overflow-x: hidden;
    overflow-y: auto;
    scrollbar-width: none;
  }
  .files.shut {
    margin-right: 0;
  }
  .files :global(.card) {
    min-height: 0;
    min-width: 0;
    max-width: 100%;
    display: flex;
    flex-direction: column;
  }
  .files.reading :global(.card.open) {
    flex: 1;
  }
  .files.reading :global(.card.open > .body) {
    flex: 1;
    min-width: 0;
    min-height: 0;
    max-width: 100%;
    overflow: auto;
  }
  .files :global(.bar .title) {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .files :global(.aside:empty) {
    display: none;
  }
  .doc {
    min-width: 0;
    max-width: 100%;
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
  .pick {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: 0;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }
  .btn {
    align-self: flex-start;
  }
</style>
