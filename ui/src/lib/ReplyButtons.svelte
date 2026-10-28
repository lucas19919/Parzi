<script lang="ts">
  // The replies an agent offered at the end of its turn: a click sends one
  // as the person's next message. The first is the agent's pick.
  import { createEventDispatcher } from "svelte";

  export let payload: unknown;

  type Reply = { label: string; message: string };
  $: p = (payload ?? {}) as { question?: string; replies?: Reply[] };
  $: replies = (p.replies ?? []).filter((r) => r && typeof r.label === "string" && r.label.trim()).slice(0, 3);

  const dispatch = createEventDispatcher<{ reply: { text: string } }>();
</script>

{#if replies.length}
  <div class="replies">
    {#if p.question}<div class="q">{p.question}</div>{/if}
    <div class="row">
      {#each replies as r, i (i)}
        <button class="btn" class:primary={i === 0} on:click={() => dispatch("reply", { text: r.message || r.label })}>{r.label}</button>
      {/each}
    </div>
  </div>
{/if}

<style>
  .replies {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    margin: var(--space) 0 var(--space);
  }
  .q {
    font-size: var(--fs-md);
    font-weight: 600;
    color: var(--text);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: calc(var(--space) * 2);
  }
  button {
    height: 30px;
    padding: 0 calc(var(--space) * 4);
    font-size: var(--fs-sm);
  }
</style>
