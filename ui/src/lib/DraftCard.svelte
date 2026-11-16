<script lang="ts">
  import { createEventDispatcher } from "svelte";

  /** A draft session: work written down for later. Start sends the brief as
   *  the first message; the plan below it carries into the run. */
  export let title = "";
  export let brief = "";
  export let starting = false;

  const dispatch = createEventDispatcher<{ start: void; remove: void }>();
</script>

<section class="draft" aria-label="Draft">
  <span class="kind t-hint">Draft</span>
  <span class="title t-section">{title || "Untitled draft"}</span>
  {#if brief}<p class="brief">{brief}</p>{/if}
  <div class="actions">
    <button class="btn primary" disabled={starting} on:click={() => dispatch("start")}>{starting ? "Starting…" : "Start"}</button>
    <button class="btn" on:click={() => dispatch("remove")}>Delete</button>
  </div>
</section>

<style>
  .draft {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    margin: var(--space) 0;
    padding: calc(var(--space) * 3) calc(var(--space) * 4);
    background: var(--surface);
    border: 1px solid transparent;
    -webkit-backdrop-filter: var(--blur);
    backdrop-filter: var(--blur);
    border-radius: var(--radius);
  }
  .kind {
    margin-bottom: calc(var(--space) * -1);
  }
  .brief {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-md);
    line-height: 1.5;
    white-space: pre-wrap;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    margin-top: var(--space);
  }
  button {
    height: var(--h);
    padding: 0 calc(var(--space) * 3);
    font-size: var(--fs-sm);
  }
</style>
