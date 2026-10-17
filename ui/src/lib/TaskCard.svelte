<script lang="ts">
  // Work an agent proposed: Start runs it as a side task beside this
  // session, Dismiss puts the card away. Both are remembered.
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
        <button on:click={() => state?.state === "started" && dispatch("open", { id: state.session })}>Open</button>
      {:else}
        <button class="primary" disabled={starting} on:click={start}>{starting ? "Starting…" : "Start"}</button>
        <button on:click={() => t.id && markCard(t.id, { state: "dismissed" })}>Dismiss</button>
      {/if}
    </div>
  </div>
{/if}

<style>
  .task {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 4px 0;
    padding: 12px 14px;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
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
    padding: 0 7px;
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
    gap: 8px;
  }
  button {
    height: 28px;
    padding: 0 12px;
    background: var(--hover);
    border: var(--glass-hairline);
    border-radius: var(--radius);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
    font-weight: 600;
    cursor: pointer;
  }
  button:hover:not(:disabled) {
    background: var(--selected);
  }
  button.primary {
    background: var(--accent);
    border-color: transparent;
    color: var(--bg);
  }
  button:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .done {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--ok);
    font-size: var(--fs-sm);
    font-weight: 600;
  }
</style>
