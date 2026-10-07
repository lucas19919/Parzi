<script lang="ts">
  import { PREVIEWS } from "./previewRegistry";

  export let which = "omnibar";

  $: entries = which === "all" ? Object.values(PREVIEWS) : Object.values(PREVIEWS).filter((e) => e.name === which);
</script>

<div class="preview">
  {#each entries as entry (entry.name)}
    {#each entry.variants as v (v.n)}
      <section>
        <header><b>{v.n} · {v.label}</b><span>{entry.title} — {v.blurb}</span></header>
        <svelte:component this={entry.component} {...entry.props(v.n, {})} />
      </section>
    {/each}
  {:else}
    <p class="hint">No preview named "{which}" yet.</p>
  {/each}
  <p class="hint">Open this gallery with <span class="mono">Ctrl+P → Component previews</span>. Tell Lucas which number wins and what to change.</p>
</div>

<style>
  .preview {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 28px;
    padding: 28px 24px 40px;
    align-items: center;
  }
  section {
    width: min(720px, 100%);
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  header {
    display: flex;
    align-items: baseline;
    gap: 10px;
    padding: 0 4px;
  }
  header b {
    font-size: 13px;
    color: var(--text);
  }
  header span {
    font-size: 12px;
    color: var(--faint);
  }
  .hint {
    font-size: 12px;
    color: var(--faint);
  }
  .mono {
    font-family: var(--mono);
    color: var(--muted);
  }
</style>
