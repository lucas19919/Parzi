<script lang="ts" context="module">
  export { picturesIn } from "./pictures";
</script>

<script lang="ts">
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
        <!-- A picture that can't load (moved, deleted) is left out, not announced. -->
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
