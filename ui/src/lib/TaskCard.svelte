<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import Icon from "./Icon.svelte";
  import { taskCards, markCard } from "./taskCards";

  export let payload: unknown;

  type Task = { id: string; title: string; brief: string; project?: string };
  $: t = (payload ?? {}) as Partial<Task>;
  $: ok = !!(t.id && t.title && t.brief);
  $: state = t.id ? $taskCards[t.id] : undefined;

  let more = false;
  let starting = false;

  const dispatch = createEventDispatcher<{ start: { task: Task; done: (session: string | null) => void }; open: { id: string } }>();

  function start() {
    if (!ok || starting) return;
    starting = true;
    dispatch("start", {
      task: t as Task,
      done: (session) => {
        starting = false;
        if (session && t.id) markCard(t.id, { state: "started", session });
      },
    });
  }
</script>

{#if ok && state?.state !== "dismissed"}
  <div class="task" class:started={state?.state === "started"}>
    <div class="head">
      <Icon name="fork" size={12} />
      <span class="title">{t.title}</span>
      {#if t.project}<span class="proj">{t.project}</span>{/if}
    </div>
    <!-- svelte-ignore a11y-click-events-have-key-events a11y-no-static-element-interactions -->
    <div class="brief" class:open={more} on:click={() => (more = !more)}>{t.brief}</div>
    <div class="actions">
      {#if state?.state === "started"}
        <span class="done"><Icon name="check" size={12} />Started</span>
        <button class="btn" on:click={() => state?.state === "started" && dispatch("open", { id: state.session })}>Open</button>
      {:else}
        <button class="btn primary" disabled={starting} on:click={start}>{starting ? "Starting…" : "Start"}</button>
        <button class="btn" on:click={() => t.id && markCard(t.id, { state: "dismissed" })}>Dismiss</button>
      {/if}
    </div>
  </div>
{/if}

<style>
  .task {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    margin: var(--space) 0;
    padding: calc(var(--space) * 3) calc(var(--space) * 4);
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
  }
  .head {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    min-width: 0;
  }
  .head :global(svg) {
    flex: none;
    color: var(--muted);
  }
  .title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-md);
    font-weight: 600;
    color: var(--text);
  }
  .proj {
    flex: none;
    padding: 0 calc(var(--space) * 2);
    border-radius: 999px;
    background: var(--hover);
    color: var(--muted);
    font-size: var(--fs-xs);
  }
  .brief {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    overflow: hidden;
    color: var(--muted);
    font-size: var(--fs-sm);
    line-height: 1.5;
    white-space: pre-wrap;
    cursor: pointer;
  }
  .brief.open {
    display: block;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
  }
  button {
    height: 28px;
    padding: 0 calc(var(--space) * 3);
    font-size: var(--fs-sm);
  }
  .done {
    display: inline-flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    color: var(--ok);
    font-size: var(--fs-sm);
    font-weight: 600;
  }
</style>
