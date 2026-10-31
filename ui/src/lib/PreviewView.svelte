<script lang="ts">
  import { PREVIEWS } from "./previewRegistry";

  export let which = "omnibar";

  $: entries = which === "all" ? Object.values(PREVIEWS) : Object.values(PREVIEWS).filter((e) => e.name === which);
</script>

<div class="preview">
  {#each entries as entry (entry.name)}
    {#each entry.variants as v (v.n)}
      <section>
        <header><b class="t-section">{v.n} · {v.label}</b><span>{entry.title}: {v.blurb}</span></header>
        <svelte:component this={entry.component} {...entry.props(v.n, {})} />
      </section>
    {/each}
  {:else}
    <p class="hint">No preview named "{which}" yet.</p>
  {/each}
  <p class="hint">Dev builds open this gallery with <span class="mono">Ctrl+P → Component previews</span>.</p>
</div>

<style>
  .preview {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 8);
    padding: calc(var(--space) * 8) calc(var(--space) * 6) calc(var(--space) * 8);
    align-items: center;
  }
  section {
    width: min(720px, 100%);
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 3);
  }
  header {
    display: flex;
    align-items: baseline;
    gap: calc(var(--space) * 3);
    padding: 0 var(--space);
  }
  header span {
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .hint {
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .mono {
    font-family: var(--mono);
    color: var(--muted);
  }
</style>
