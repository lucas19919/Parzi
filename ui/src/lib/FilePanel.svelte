<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { open as pickPath } from "@tauri-apps/plugin-dialog";
  import InfoCard, { unfold } from "./InfoCard.svelte";
  import Icon from "./Icon.svelte";
  import { api, brain } from "./api";
  import { saveFile } from "./saveFile";
  import { codeBlockClick, mdHtml, richReady } from "./md";
  import { handleLinkClick } from "./links";
  import { toast, toastError } from "./toast";
  import { tooltip } from "./tooltip";
  import type { ArtifactDoc } from "./tabs";

  export let doc: ArtifactDoc | null = null;
  export let folder = "";
  export let width = 420;
  export let shown = true;

  const dispatch = createEventDispatcher<{
    close: void;
    resize: { width: number };
    pick: ArtifactDoc;
  }>();

  let dragging = false;
  let copied = false;
  let picker: HTMLInputElement | null = null;

  $: onDisk = !!doc && /[\\/]/.test(doc.id);
  $: markdown = !!doc && (doc.kind === "markdown" || doc.kind === "md");
  $: if (doc) unfold("reader");

  function startResize(e: PointerEvent) {
    dragging = true;
    (e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId);
    e.preventDefault();
  }
  function onResize(e: PointerEvent) {
    if (!dragging) return;
    const aside = (e.currentTarget as HTMLElement).closest(".files") as HTMLElement | null;
    const rect = aside?.getBoundingClientRect();
    const next = rect ? rect.right - e.clientX : window.innerWidth - e.clientX;
    dispatch("resize", { width: Math.round(Math.min(640, Math.max(220, next))) });
  }
  function endResize() {
    dragging = false;
  }

  function fromFile(id: string, name: string, content: string) {
    const isMd = /\.(md|markdown)$/i.test(name);
    const title = name.replace(/\.(md|markdown)$/i, "") || name;
    dispatch("pick", {
      id,
      title,
      kind: isMd ? "markdown" : "text",
      content,
      version: 1,
    });
  }

  async function loadPath(path: string) {
    let content = "";
    try {
      content = await api.readTextFile(path);
    } catch (err) {
      toast(String(err), true);
      return;
    }
    const name = path.split(/[\\/]/).pop() || path;
    fromFile(path, name, content);
  }

  async function browse() {
    if (location.pathname.endsWith("mock.html")) {
      picker?.click();
      return;
    }
    try {
      const picked = await pickPath({
        multiple: false,
        directory: false,
        defaultPath: folder || undefined,
        title: "Open file",
      });
      if (typeof picked === "string" && picked) await loadPath(picked);
    } catch (err) {
      toast(`Couldn't open that file: ${err}`, true);
    }
  }

  async function onPick(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    if (file.size > 1_000_000) {
      toast("That file is too large to open here", true);
      return;
    }
    let content = "";
    try {
      content = await file.text();
    } catch (err) {
      toast(`Couldn't read that file: ${err}`, true);
      return;
    }
    fromFile(file.name, file.name, content);
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
    const base = doc.title.replace(/\.[a-z0-9]{1,8}$/i, "") || "note";
    void saveFile(`${base}.md`, doc.content);
  }

  function reveal() {
    if (!doc) return;
    const abs = /^[a-zA-Z]:[\\/]/.test(doc.id) || doc.id.startsWith("/") || doc.id.startsWith("\\\\");
    if (abs) {
      const dir = doc.id.replace(/[\\/][^\\/]*$/, "") || doc.id;
      void api.openFilePath(dir).catch(toastError);
      return;
    }
    void brain.open(doc.id, "folder").catch(toastError);
  }

  function onDocClick(e: MouseEvent) {
    void (async () => {
      if (await handleLinkClick(e, folder)) return;
      codeBlockClick(e.target as HTMLElement);
    })();
  }
</script>

<aside class="files" class:shut={!shown} class:reading={!!doc} aria-label="Files" style="width: {shown ? width : 0}px" inert={!shown}>
  <div
    class="grip"
    role="separator"
    aria-orientation="vertical"
    aria-label="Resize files"
    title="Drag to resize"
    on:pointerdown={startResize}
    on:pointermove={onResize}
    on:pointerup={endResize}
    on:pointercancel={endResize}
  />
  <InfoCard id="reader" title={doc?.title || "File"}>
    <svelte:fragment slot="actions">
      {#if doc}
        <button class="icon-btn" aria-label="Open file" use:tooltip={"Open file"} on:click={browse}><Icon name="file" /></button>
        <button class="icon-btn" aria-label={copied ? "Copied" : "Copy"} use:tooltip={copied ? "Copied" : "Copy"} on:click={copy}><Icon name={copied ? "check" : "copy"} /></button>
        <button class="icon-btn" aria-label="Save as file" use:tooltip={"Save as file"} on:click={save}><Icon name="download" /></button>
        {#if onDisk}
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
  <input class="pick" bind:this={picker} type="file" tabindex="-1" aria-hidden="true" on:change={onPick} />
</aside>

<style>
  .files {
    position: relative;
    flex: none;
    align-self: stretch;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    margin: 0 var(--gap) 0 0;
    padding-bottom: calc(var(--space) * 3);
    overflow-x: hidden;
    overflow-y: auto;
    scrollbar-width: none;
    background: none;
    transition: width 300ms var(--ease-out), margin-right 300ms var(--ease-out);
  }
  .files.shut {
    margin-right: 0;
  }
  .grip {
    position: absolute;
    left: calc(-1 * var(--gap));
    top: 0;
    bottom: 0;
    width: var(--gap);
    cursor: ew-resize;
    touch-action: none;
  }
  .grip::after {
    content: "";
    position: absolute;
    left: 3px;
    top: 12px;
    bottom: 12px;
    width: 2px;
    border-radius: var(--radius-sm);
    background: transparent;
    transition: background 120ms ease;
  }
  .grip:hover::after {
    background: color-mix(in srgb, var(--text) 18%, transparent);
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
