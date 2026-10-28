<script lang="ts" context="module">
  // Image files a step's tools say they made: a JSON `path` (Grok's
  // image_gen and image_edit answer that way), or a full path to an image
  // in the text.
  const IMAGE = /\.(png|jpe?g|webp|gif)$/i;
  const FULL = /(?:[A-Za-z]:[\\/]|\/)[^\s"'`<>|*?]+?\.(?:png|jpe?g|webp|gif)\b/gi;

  export function picturesIn(outputs: string[]): string[] {
    const out: string[] = [];
    const add = (p: string) => {
      const clean = p.trim();
      if (IMAGE.test(clean) && !out.includes(clean)) out.push(clean);
    };
    for (const text of outputs) {
      if (!text) continue;
      try {
        const v = JSON.parse(text);
        const path = v && typeof v === "object" ? (v.path ?? v.output?.path) : null;
        if (typeof path === "string") {
          add(path);
          continue;
        }
      } catch {
        /* not JSON: look in the text */
      }
      for (const m of text.matchAll(FULL)) add(m[0]);
    }
    return out.slice(0, 12);
  }
</script>

<script lang="ts">
  // The pictures an agent's tools made, shown where they were made: tiles
  // that open full size.
  import { api } from "./api";
  import { viewImage } from "./imageViewer";

  export let session = "";
  export let paths: string[] = [];

  const name = (p: string) => p.split(/[\\/]/).pop() ?? p;
</script>

{#if paths.length && session}
  <div class="pics">
    {#each paths as p (p)}
      {#await api.toolImage(session, p) then url}
        <button class="pic" aria-label="View {name(p)}" title={name(p)} on:click={() => viewImage(url, name(p))}>
          <img src={url} alt={name(p)} draggable="false" />
        </button>
      {:catch}
        <!-- Gone or unreadable: nothing to show. -->
      {/await}
    {/each}
  </div>
{/if}

<style>
  .pics {
    display: flex;
    flex-wrap: wrap;
    gap: calc(var(--space) * 2);
    margin: var(--space) 0;
  }
  .pic {
    padding: 0;
    background: none;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    overflow: hidden;
    cursor: zoom-in;
  }
  .pic img {
    display: block;
    height: 132px;
    width: auto;
    max-width: 236px;
    object-fit: cover;
  }
</style>
